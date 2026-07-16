#[cfg(test)]
use yelang_ast::ImplItemKind;
use yelang_ast::{
    AssignEqExpr, AssignOpExpr, Attribute, AttributeArgs, BinaryExpr, BlockExpr, Codegen, Expr,
    ExprKind, IfExpr, Item, ItemKind, MacroInvocation, MemberAccess, NamedArg, Pattern,
    PatternKind, Program, Stmt, StmtKind, TokenKind, Type, TypeKind, UnaryExpr,
};

use yelang_interner::Interner;

use std::path::Path;

use crate::builtin_decorators::{BuiltinDecorator, apply_decorator};
use crate::builtin_macros::expand_builtin_macro;
use crate::eager::{
    CfgOptions, EagerBuiltin, EagerContext, EnvProvider, FileLoader, StdEnvProvider, StdFileLoader,
    expand_eager_macros_in_stream,
};
use crate::error::ExpandError;
use crate::matcher::{MacroKind, try_match_matcher, try_match_rule};
use crate::proc_macro::{
    InProcessExecutor, InProcessProcMacro, ProcMacroRuntime, core_to_wire,
    wire_diagnostics_to_errors, wire_to_core,
};
use crate::resolver::MacroResolver;
use crate::transcribe::transcribe;
use yelang_macro_core::{
    CrateId, ExpnData, ExpnKind, HygieneData, MacroDefId, TokenStream, Transparency,
};
use yelang_proc_macro::Diagnostic;
use yelang_proc_macro_bridge::sandbox::Limits;

const MAX_EXPANSIONS: usize = 1000;
const MAX_RECURSION_DEPTH: usize = 128;

/// Result of expanding a program.
pub struct ExpandResult {
    pub program: Program,
    pub errors: Vec<ExpandError>,
}

/// An identifier for a macro on the expansion stack. Declarative macros are
/// identified by their arena key; procedural macros are identified by name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MacroFrameId {
    Declarative(MacroDefId),
    ProcMacro(String),
}

/// A frame on the macro expansion stack, used for cycle detection and
/// diagnostic backtraces.
#[derive(Debug, Clone)]
struct ExpansionFrame {
    name: String,
    span: yelang_lexer::Span,
    frame_id: MacroFrameId,
}

/// Result of matching a set of attribute/derive rules against an invocation.
enum RuleMatches {
    /// Exactly one rule matched the invocation.
    Matched {
        rule: crate::matcher::types::MacroRule,
        bindings: crate::matcher::bindings::Bindings,
    },
    /// No rule matched.
    NoMatch { unsafe_available: bool },
    /// More than one rule matched.
    Ambiguous,
}

impl RuleMatches {
    fn from_vec(
        mut matches: Vec<(
            &crate::matcher::types::MacroRule,
            crate::matcher::bindings::Bindings,
        )>,
    ) -> Self {
        match matches.len() {
            0 => RuleMatches::NoMatch {
                unsafe_available: false,
            },
            1 => RuleMatches::Matched {
                rule: matches[0].0.clone(),
                bindings: std::mem::take(&mut matches[0].1),
            },
            _ => RuleMatches::Ambiguous,
        }
    }
}

/// The main macro expansion engine.
///
/// Walks the AST, expands macro invocations, and applies decorators.
/// Operates iteratively until no more macro invocations remain.
pub struct MacroExpander<'a> {
    interner: &'a Interner,
    /// Errors accumulated during expansion.
    errors: Vec<ExpandError>,
    /// Declarative macro definitions collected before expansion.
    resolver: MacroResolver,
    /// Hygiene context allocation.
    hygiene: HygieneData,
    /// Stack of macro invocations currently being expanded (loop detection).
    expansion_stack: Vec<ExpansionFrame>,
    /// Stack of active hygiene contexts. The root context is always present so
    /// nested macro expansions parent their marks to the current context.
    hygiene_stack: Vec<yelang_macro_core::SyntaxContextId>,
    /// Total number of macro expansions performed.
    expansion_count: usize,
    /// File-system abstraction for `include!` and friends.
    file_loader: &'a dyn FileLoader,
    /// Environment-variable abstraction for `env!` and `option_env!`.
    env_provider: &'a dyn EnvProvider,
    /// Active `cfg` options for `cfg!`.
    cfg_options: CfgOptions,
    /// Path of the source file being expanded, if known.
    current_file: Option<&'a Path>,
    /// In-process procedural macro executor for testing and bootstrapping.
    in_process_executor: Option<std::sync::Arc<InProcessExecutor>>,
    /// Out-of-process procedural macro runtime (server connection + resolver).
    proc_macro_runtime: Option<ProcMacroRuntime>,
}

impl<'a> MacroExpander<'a> {
    pub fn new(interner: &'a Interner) -> Self {
        Self {
            interner,
            errors: vec![],
            resolver: MacroResolver::new(),
            hygiene: HygieneData::new(),
            expansion_stack: vec![],
            hygiene_stack: vec![yelang_macro_core::SyntaxContextId::default()],
            expansion_count: 0,
            file_loader: &StdFileLoader,
            env_provider: &StdEnvProvider,
            cfg_options: CfgOptions::new(),
            current_file: None,
            in_process_executor: None,
            proc_macro_runtime: None,
        }
    }

    pub fn with_local_crate(interner: &'a Interner, local_crate: CrateId) -> Self {
        Self {
            interner,
            errors: vec![],
            resolver: MacroResolver::with_local_crate(local_crate),
            hygiene: HygieneData::new(),
            expansion_stack: vec![],
            hygiene_stack: vec![yelang_macro_core::SyntaxContextId::default()],
            expansion_count: 0,
            file_loader: &StdFileLoader,
            env_provider: &StdEnvProvider,
            cfg_options: CfgOptions::new(),
            current_file: None,
            in_process_executor: None,
            proc_macro_runtime: None,
        }
    }

    pub fn with_file_loader(mut self, loader: &'a dyn FileLoader) -> Self {
        self.file_loader = loader;
        self
    }

    pub fn with_env_provider(mut self, provider: &'a dyn EnvProvider) -> Self {
        self.env_provider = provider;
        self
    }

    pub fn with_cfg_options(mut self, cfg: CfgOptions) -> Self {
        self.cfg_options = cfg;
        self
    }

    pub fn with_current_file(mut self, path: &'a Path) -> Self {
        self.current_file = Some(path);
        self
    }

    pub fn with_in_process_proc_macros(mut self, executor: InProcessExecutor) -> Self {
        self.in_process_executor = Some(std::sync::Arc::new(executor));
        self
    }

    pub fn with_proc_macro_runtime(mut self, runtime: ProcMacroRuntime) -> Self {
        self.proc_macro_runtime = Some(runtime);
        self
    }

    fn eager_context(&self) -> EagerContext<'a> {
        EagerContext {
            interner: self.interner,
            file_loader: self.file_loader,
            env_provider: self.env_provider,
            current_file: self.current_file,
            cfg_options: self.cfg_options.clone(),
        }
    }

    /// Expand all macros in a program.
    pub fn expand(&mut self, program: &Program) -> ExpandResult {
        let mut program = program.clone();
        let collect_errors = self
            .resolver
            .collect_from_program(&mut program, self.interner);
        self.errors.extend(collect_errors);

        // Iterative expansion: expanded output may contain new macro invocations.
        // We loop until no more changes are made (or max iterations reached to prevent infinite loops).
        let mut items = program.items;
        let mut iterations = 0;
        const MAX_ITERATIONS: usize = 100;
        loop {
            iterations += 1;
            if iterations > MAX_ITERATIONS {
                self.errors.push(
                    ExpandError::expansion_loop(
                        "(expansion loop)".to_string(),
                        yelang_lexer::Span::default(),
                    )
                    .with_backtrace(self.backtrace()),
                );
                break;
            }

            let mut changed = false;
            let mut new_items = vec![];
            for item in items {
                let original = item.clone();
                match self.expand_item(item) {
                    Ok(expanded) => {
                        // expand_item returns the original item unchanged when an error occurs,
                        // but it still reports the error. We consider a change only if the
                        // returned item actually differs from the input.
                        changed |= expanded.len() > 1 || item_differs(&expanded, &original);
                        new_items.extend(expanded);
                    }
                    Err(e) => {
                        self.errors.push(e);
                        new_items.push(original);
                    }
                }
            }
            items = new_items;

            if !changed {
                break;
            }
        }

        let mut program = Program {
            items,
            span: yelang_lexer::Span::default(),
        };

        // Generate C ABI wrappers and the export table for functions annotated
        // with `#[yelang_proc_macro::macro_export]` (and friends). This pass
        // runs after all other macro expansion so that the signatures it sees
        // are final.
        let export_result =
            crate::proc_macro::export::expand_proc_macro_exports(&program, self.interner);
        program = export_result.program;
        for e in export_result.errors {
            self.errors.push(ExpandError::decorator_error(
                e,
                yelang_lexer::Span::default(),
            ));
        }

        ExpandResult {
            program,
            errors: self.errors.clone(),
        }
    }

    /// Expand a single item, applying decorators and any top-level macros.
    ///
    /// Returns a vec because decorators such as `@derive` may generate
    /// additional items (e.g. `impl` blocks) alongside the original item.
    pub fn expand_item(&mut self, item: Item) -> Result<Vec<Item>, ExpandError> {
        let (primary, side_items) = self.expand_item_attributes(item)?;

        // Deeply expand the primary item. It may expand to multiple items if it
        // is an item-position macro invocation.
        let (expanded_primary, _) = self.expand_item_deep(primary);

        // Deeply expand side items (they keep any attributes they were generated
        // with and are not further attribute-expanded in this phase).
        let mut expanded_items = expanded_primary;
        for side in side_items {
            let (expanded, _) = self.expand_item_deep(side);
            expanded_items.extend(expanded);
        }
        Ok(expanded_items)
    }

    /// Process all attributes on a single item, returning the transformed primary
    /// item and any side-items generated by derives or attribute macros.
    fn expand_item_attributes(&mut self, mut item: Item) -> Result<(Item, Vec<Item>), ExpandError> {
        let mut decorator_errors = vec![];
        let mut side_items = vec![];

        while let Some(attr) = item.attributes.first().cloned() {
            let attr_name = attr
                .path
                .first()
                .map(|id| self.interner.resolve(&id.symbol).to_string())
                .unwrap_or_default();
            item.attributes.remove(0);

            let expanded: Vec<Item> = if attr_name == "unsafe" {
                match peel_unsafe_attribute(&attr, self.interner) {
                    Some((inner_attr, is_unsafe)) => {
                        if self.is_user_attribute_macro(&inner_attr) {
                            self.expand_user_attribute_macro(&inner_attr, &item, is_unsafe)
                                .unwrap_or_else(|| vec![item.clone()])
                        } else {
                            self.errors.push(
                                ExpandError::decorator_error(
                                    format!(
                                        "`unsafe(...)` wrapper does not name a known attribute macro: `{}`",
                                        inner_attr
                                            .path
                                            .first()
                                            .map(|id| self.interner.resolve(&id.symbol))
                                            .unwrap_or("")
                                    ),
                                    attr.span,
                                )
                                .with_backtrace(self.backtrace()),
                            );
                            vec![item.clone()]
                        }
                    }
                    None => {
                        self.errors.push(
                            ExpandError::decorator_error(
                                "`unsafe(...)` attribute wrapper is malformed".to_string(),
                                attr.span,
                            )
                            .with_backtrace(self.backtrace()),
                        );
                        vec![item.clone()]
                    }
                }
            } else if attr_name == "derive" {
                // `@derive(A, B, C)` is special: each name may be a user macro or a
                // built-in derive.
                self.expand_derive_attribute(&attr, &item)
            } else if self.is_user_attribute_macro(&attr) {
                self.expand_user_attribute_macro(&attr, &item, false)
                    .unwrap_or_else(|| vec![item.clone()])
            } else if let Some(decorator) = BuiltinDecorator::from_attribute(&attr, self.interner) {
                let result = apply_decorator(decorator, &attr, &item, self.interner);
                if result.items.is_empty() && !result.errors.is_empty() {
                    for err in &result.errors {
                        decorator_errors.push(
                            ExpandError::decorator_error(err.clone(), attr.span)
                                .with_backtrace(self.backtrace()),
                        );
                    }
                    vec![item.clone()]
                } else {
                    result.items
                }
            } else {
                // Unknown attribute: preserve it and stop processing this item.
                item.attributes.insert(0, attr);
                break;
            };

            let mut expanded_iter = expanded.into_iter();
            item = expanded_iter.next().unwrap_or(item);
            side_items.extend(expanded_iter);
        }

        self.errors.extend(decorator_errors);
        Ok((item, side_items))
    }

    /// Deeply expand all macro invocations inside an item.
    ///
    /// Returns a vector because an item-position macro invocation may expand to
    /// multiple items. For ordinary items the vector contains a single element.
    fn expand_item_deep(&mut self, mut item: Item) -> (Vec<Item>, bool) {
        // Item-position macro invocation: expand to items.
        if let ItemKind::MacroInvocation(inv) = &item.kind {
            match self.expand_macro_invocation(inv) {
                Ok(stream) => match parse_items_from_token_stream(
                    &stream,
                    self.interner,
                    &self.eager_context(),
                ) {
                    Ok(items) => {
                        let mut expanded = vec![];
                        for it in items {
                            let (es, _) = self.expand_item_deep(it);
                            expanded.extend(es);
                        }
                        self.after_expand();
                        return (expanded, true);
                    }
                    Err(reason) => {
                        self.after_expand();
                        self.errors.push(
                            ExpandError::malformed_macro_args(
                                format!("macro expansion did not produce valid items: {}", reason),
                                inv.span,
                            )
                            .with_backtrace(self.backtrace()),
                        );
                    }
                },
                Err(e) => self.errors.push(e),
            }
            return (vec![item], false);
        }

        let mut changed = false;

        match &mut item.kind {
            ItemKind::Fn(func) => {
                changed |= self.expand_fn_sig(&mut func.sig);
                let (new_body, body_changed) = self.expand_block_expr(&func.body);
                func.body = new_body;
                changed |= body_changed;
            }
            ItemKind::Const(c) => {
                let (new_ty, ty_changed) = self.expand_type(&c.ty);
                c.ty = new_ty;
                changed |= ty_changed;
                let (new_expr, expr_changed) = self.expand_expr(&c.value);
                c.value = new_expr;
                changed |= expr_changed;
            }
            ItemKind::Static(s) => {
                let (new_ty, ty_changed) = self.expand_type(&s.ty);
                s.ty = new_ty;
                changed |= ty_changed;
                let (new_expr, expr_changed) = self.expand_expr(&s.value);
                s.value = new_expr;
                changed |= expr_changed;
            }
            ItemKind::Impl(i) => {
                let (new_self_ty, self_ty_changed) = self.expand_type(&i.self_ty);
                i.self_ty = new_self_ty;
                changed |= self_ty_changed;
                for item in &mut i.items {
                    match &mut item.item {
                        yelang_ast::ImplItemKind::Method(m) => {
                            changed |= self.expand_fn_sig(&mut m.sig);
                            let (new_body, body_changed) = self.expand_block_expr(&m.body);
                            m.body = new_body;
                            changed |= body_changed;
                        }
                        yelang_ast::ImplItemKind::AssociatedType(at) => {
                            let (new_ty, ty_changed) = self.expand_type(&at.ty);
                            at.ty = new_ty;
                            changed |= ty_changed;
                        }
                        yelang_ast::ImplItemKind::Constant(c) => {
                            let (new_ty, ty_changed) = self.expand_type(&c.ty);
                            c.ty = new_ty;
                            changed |= ty_changed;
                            if let Some(value) = &mut c.value {
                                let (new_value, value_changed) = self.expand_expr(value);
                                *value = new_value;
                                changed |= value_changed;
                            }
                        }
                    }
                }
            }
            ItemKind::Trait(t) => {
                for item in &mut t.items {
                    match &mut item.item {
                        yelang_ast::TraitItemKind::Method(m) => {
                            changed |= self.expand_fn_sig(&mut m.sig);
                            if let Some(body) = &mut m.body {
                                let (new_body, body_changed) = self.expand_block_expr(body);
                                *body = new_body;
                                changed |= body_changed;
                            }
                        }
                        yelang_ast::TraitItemKind::AssociatedType(at) => {
                            if let Some(default) = &mut at.default {
                                let (new_ty, ty_changed) = self.expand_type(default);
                                *default = new_ty;
                                changed |= ty_changed;
                            }
                        }
                        yelang_ast::TraitItemKind::Constant(c) => {
                            let (new_ty, ty_changed) = self.expand_type(&c.ty);
                            c.ty = new_ty;
                            changed |= ty_changed;
                            if let Some(value) = &mut c.value {
                                let (new_value, value_changed) = self.expand_expr(value);
                                *value = new_value;
                                changed |= value_changed;
                            }
                        }
                    }
                }
            }
            ItemKind::Module(m) => {
                if let yelang_ast::ModKind::Inline { items: mod_items } = &mut m.kind {
                    let mut new_items = vec![];
                    for mi in std::mem::take(mod_items) {
                        let original = mi.clone();
                        match self.expand_item(mi) {
                            Ok(expanded) => {
                                new_items.extend(expanded);
                                changed = true;
                            }
                            Err(e) => {
                                self.errors.push(e);
                                new_items.push(original);
                            }
                        }
                    }
                    *mod_items = new_items;
                }
            }
            ItemKind::Struct(s) => match &mut s.fields {
                yelang_ast::StructFields::Named(fields) => {
                    for f in fields {
                        let (new_ty, ty_changed) = self.expand_type(&f.ty);
                        f.ty = new_ty;
                        changed |= ty_changed;
                    }
                }
                yelang_ast::StructFields::Tuple(types) => {
                    let old_types = std::mem::take(types);
                    let mut new_types = vec![];
                    for t in old_types {
                        let (nt, c) = self.expand_type(&t);
                        new_types.push(nt);
                        changed |= c;
                    }
                    *types = new_types;
                }
                yelang_ast::StructFields::Unit => {}
            },
            ItemKind::Enum(e) => {
                for v in &mut e.variants {
                    match &mut v.kind {
                        yelang_ast::VariantKind::Tuple(types) => {
                            let old_types = std::mem::take(types);
                            let mut new_types = vec![];
                            for t in old_types {
                                let (nt, c) = self.expand_type(&t);
                                new_types.push(nt);
                                changed |= c;
                            }
                            *types = new_types;
                        }
                        yelang_ast::VariantKind::Struct(fields) => {
                            for f in fields {
                                let (new_ty, ty_changed) = self.expand_type(&f.ty);
                                f.ty = new_ty;
                                changed |= ty_changed;
                            }
                        }
                        yelang_ast::VariantKind::Unit => {}
                    }
                    if let Some(disc) = &mut v.discriminant {
                        let (new_disc, disc_changed) = self.expand_expr(disc);
                        *disc = new_disc;
                        changed |= disc_changed;
                    }
                }
            }
            ItemKind::TypeAlias(ta) => {
                let (new_target, target_changed) = self.expand_type(&ta.target);
                ta.target = new_target;
                changed |= target_changed;
            }
            _ => {}
        }

        (vec![item], changed)
    }

    /// Expand macros in a function signature (parameter patterns/types and return type).
    /// Returns whether anything changed.
    fn expand_fn_sig(&mut self, sig: &mut yelang_ast::FnSig) -> bool {
        let mut changed = false;
        for param in &mut sig.params {
            let (new_pattern, pattern_changed) = self.expand_pattern(&param.pattern);
            param.pattern = new_pattern;
            changed |= pattern_changed;
            let (new_ty, ty_changed) = self.expand_type(&param.ty);
            param.ty = new_ty;
            changed |= ty_changed;
        }
        if let yelang_ast::FnRefType::Type(ret) = &mut sig.return_type {
            let (new_ret, ret_changed) = self.expand_type(ret);
            *ret = new_ret;
            changed |= ret_changed;
        }
        changed
    }

    /// Expand all macros in a block expression.
    fn expand_block_expr(&mut self, block: &BlockExpr) -> (BlockExpr, bool) {
        let mut new_stmts = vec![];
        let mut changed = false;

        for stmt in &block.statements {
            let (mut stmts, stmt_changed) = self.expand_stmt(stmt);
            new_stmts.append(&mut stmts);
            changed |= stmt_changed;
        }

        (
            BlockExpr {
                label: block.label.clone(),
                statements: new_stmts,
            },
            changed,
        )
    }

    /// Expand all macros in a type annotation, recursively.
    fn expand_type(&mut self, ty: &Type) -> (Type, bool) {
        match &ty.kind {
            TypeKind::MacroInvocation(inv) => {
                match self.expand_macro_invocation(inv) {
                    Ok(stream) => match parse_type_from_token_stream(
                        &stream,
                        self.interner,
                        &self.eager_context(),
                    ) {
                        Ok(expanded) => {
                            let (recursed, _) = self.expand_type(&expanded);
                            self.after_expand();
                            return (recursed, true);
                        }
                        Err(reason) => {
                            self.errors.push(
                                ExpandError::malformed_macro_args(
                                    format!(
                                        "macro expansion did not produce a valid type: {}",
                                        reason
                                    ),
                                    inv.span,
                                )
                                .with_backtrace(self.backtrace()),
                            );
                            self.after_expand();
                        }
                    },
                    Err(e) => self.errors.push(e),
                }
                (ty.clone(), false)
            }
            TypeKind::Ref { ty: inner, is_mut } => {
                let (new_inner, changed) = self.expand_type(inner);
                (
                    Type {
                        kind: TypeKind::Ref {
                            ty: Box::new(new_inner),
                            is_mut: *is_mut,
                        },
                        span: ty.span,
                    },
                    changed,
                )
            }
            TypeKind::RawPtr { ty: inner, is_mut } => {
                let (new_inner, changed) = self.expand_type(inner);
                (
                    Type {
                        kind: TypeKind::RawPtr {
                            ty: Box::new(new_inner),
                            is_mut: *is_mut,
                        },
                        span: ty.span,
                    },
                    changed,
                )
            }
            TypeKind::Tuple(types) => {
                let mut new_types = vec![];
                let mut changed = false;
                for t in types {
                    let (nt, c) = self.expand_type(t);
                    new_types.push(nt);
                    changed |= c;
                }
                (
                    Type {
                        kind: TypeKind::Tuple(new_types),
                        span: ty.span,
                    },
                    changed,
                )
            }
            TypeKind::Array(inner, size) => {
                let (new_inner, inner_changed) = self.expand_type(inner);
                let (new_size, size_changed) = self.expand_expr(size);
                (
                    Type {
                        kind: TypeKind::Array(Box::new(new_inner), Box::new(new_size)),
                        span: ty.span,
                    },
                    inner_changed || size_changed,
                )
            }
            TypeKind::Slice(inner) => {
                let (new_inner, changed) = self.expand_type(inner);
                (
                    Type {
                        kind: TypeKind::Slice(Box::new(new_inner)),
                        span: ty.span,
                    },
                    changed,
                )
            }
            TypeKind::Function(func) => {
                let mut new_params = vec![];
                let mut changed = false;
                for p in &func.params {
                    let (np, c) = self.expand_type(p);
                    new_params.push(np);
                    changed |= c;
                }
                let (new_ret, ret_changed) = self.expand_type(&func.return_type);
                changed |= ret_changed;
                (
                    Type {
                        kind: TypeKind::Function(yelang_ast::FunctionType {
                            abi: func.abi.clone(),
                            is_async: func.is_async,
                            params: new_params,
                            return_type: Box::new(new_ret),
                            is_variadic: func.is_variadic,
                        }),
                        span: ty.span,
                    },
                    changed,
                )
            }
            TypeKind::ForAll { params, ty: inner } => {
                let (new_inner, changed) = self.expand_type(inner);
                (
                    Type {
                        kind: TypeKind::ForAll {
                            params: params.clone(),
                            ty: Box::new(new_inner),
                        },
                        span: ty.span,
                    },
                    changed,
                )
            }
            TypeKind::Union(types) => {
                let mut new_types = vec![];
                let mut changed = false;
                for t in types {
                    let (nt, c) = self.expand_type(t);
                    new_types.push(nt);
                    changed |= c;
                }
                (
                    Type {
                        kind: TypeKind::Union(new_types),
                        span: ty.span,
                    },
                    changed,
                )
            }
            TypeKind::Structural(fields) => {
                let mut new_fields = vec![];
                let mut changed = false;
                for f in fields {
                    let (nt, c) = self.expand_type(&f.ty);
                    changed |= c;
                    new_fields.push(yelang_ast::StructuralField {
                        name: f.name,
                        ty: nt,
                        optional: f.optional,
                    });
                }
                (
                    Type {
                        kind: TypeKind::Structural(new_fields),
                        span: ty.span,
                    },
                    changed,
                )
            }
            TypeKind::Operator(op) => match op {
                yelang_ast::TypeOperator::TypeOf(expr) => {
                    let (new_expr, changed) = self.expand_expr(expr);
                    (
                        Type {
                            kind: TypeKind::Operator(yelang_ast::TypeOperator::TypeOf(Box::new(
                                new_expr,
                            ))),
                            span: ty.span,
                        },
                        changed,
                    )
                }
                yelang_ast::TypeOperator::ReturnType(inner)
                | yelang_ast::TypeOperator::Parameters(inner) => {
                    let (new_inner, changed) = self.expand_type(inner);
                    (
                        Type {
                            kind: TypeKind::Operator(match op {
                                yelang_ast::TypeOperator::ReturnType(_) => {
                                    yelang_ast::TypeOperator::ReturnType(Box::new(new_inner))
                                }
                                _ => yelang_ast::TypeOperator::Parameters(Box::new(new_inner)),
                            }),
                            span: ty.span,
                        },
                        changed,
                    )
                }
                yelang_ast::TypeOperator::Pick(base, keys)
                | yelang_ast::TypeOperator::Omit(base, keys) => {
                    let (new_base, base_changed) = self.expand_type(base);
                    let (new_keys, keys_changed) = self.expand_type(keys);
                    (
                        Type {
                            kind: TypeKind::Operator(match op {
                                yelang_ast::TypeOperator::Pick(_, _) => {
                                    yelang_ast::TypeOperator::Pick(
                                        Box::new(new_base),
                                        Box::new(new_keys),
                                    )
                                }
                                _ => yelang_ast::TypeOperator::Omit(
                                    Box::new(new_base),
                                    Box::new(new_keys),
                                ),
                            }),
                            span: ty.span,
                        },
                        base_changed || keys_changed,
                    )
                }
            },
            TypeKind::Named(path) | TypeKind::ImplTrait(path) | TypeKind::DynTrait(path) => {
                let (new_path, changed) = self.expand_path_type_args(path);
                (
                    Type {
                        kind: match &ty.kind {
                            TypeKind::Named(_) => TypeKind::Named(new_path),
                            TypeKind::ImplTrait(_) => TypeKind::ImplTrait(new_path),
                            _ => TypeKind::DynTrait(new_path),
                        },
                        span: ty.span,
                    },
                    changed,
                )
            }
            _ => (ty.clone(), false),
        }
    }

    /// Expand any macro invocations inside generic arguments on a path.
    fn expand_path_type_args(&mut self, path: &yelang_ast::Path) -> (yelang_ast::Path, bool) {
        let mut changed = false;
        let mut new_segments = vec![];
        for seg in &path.segments {
            let new_args = if let Some(args) = &seg.args {
                let (new_args, c) = self.expand_generic_args(args);
                changed |= c;
                Some(new_args)
            } else {
                None
            };
            new_segments.push(yelang_ast::PathSegment {
                ident: seg.ident,
                args: new_args,
            });
        }
        (
            yelang_ast::Path {
                segments: new_segments,
                qself: path.qself.clone(),
                is_absolute: path.is_absolute,
                span: path.span,
            },
            changed,
        )
    }

    /// Expand any macro invocations inside generic arguments.
    fn expand_generic_args(
        &mut self,
        args: &yelang_ast::GenericArgs,
    ) -> (yelang_ast::GenericArgs, bool) {
        match args {
            yelang_ast::GenericArgs::AngleBracketed(ab) => {
                let mut new_args = vec![];
                let mut changed = false;
                for arg in &ab.args {
                    let (new_arg, c) = match arg {
                        yelang_ast::AngleBracketedArg::Type(t) => {
                            let (nt, c) = self.expand_type(t);
                            (yelang_ast::AngleBracketedArg::Type(nt), c)
                        }
                        yelang_ast::AngleBracketedArg::Const(e) => {
                            let (ne, c) = self.expand_expr(e);
                            (yelang_ast::AngleBracketedArg::Const(ne), c)
                        }
                        yelang_ast::AngleBracketedArg::AssociatedType { name, ty } => {
                            let (nt, c) = self.expand_type(ty);
                            (
                                yelang_ast::AngleBracketedArg::AssociatedType {
                                    name: *name,
                                    ty: nt,
                                },
                                c,
                            )
                        }
                    };
                    new_args.push(new_arg);
                    changed |= c;
                }
                (
                    yelang_ast::GenericArgs::AngleBracketed(yelang_ast::AngleBracketedArgs {
                        args: new_args,
                        span: ab.span,
                    }),
                    changed,
                )
            }
            yelang_ast::GenericArgs::Parenthesized(pa) => {
                let mut new_ins = vec![];
                let mut changed = false;
                for t in &pa.ins {
                    let (nt, c) = self.expand_type(t);
                    new_ins.push(nt);
                    changed |= c;
                }
                let (new_out, out_changed) = if let Some(out) = &pa.out {
                    let (no, c) = self.expand_type(out);
                    (Some(no), c)
                } else {
                    (None, false)
                };
                changed |= out_changed;
                (
                    yelang_ast::GenericArgs::Parenthesized(yelang_ast::ParenthesizedArgs {
                        ins: new_ins,
                        out: new_out,
                        span: pa.span,
                    }),
                    changed,
                )
            }
        }
    }

    /// Expand all macros in a pattern, recursively.
    fn expand_pattern(&mut self, pat: &Pattern) -> (Pattern, bool) {
        match &pat.pattern {
            PatternKind::MacroInvocation(inv) => {
                match self.expand_macro_invocation(inv) {
                    Ok(stream) => match parse_pattern_from_token_stream(
                        &stream,
                        self.interner,
                        &self.eager_context(),
                    ) {
                        Ok(expanded) => {
                            let (recursed, _) = self.expand_pattern(&expanded);
                            self.after_expand();
                            return (recursed, true);
                        }
                        Err(reason) => {
                            self.errors.push(
                                ExpandError::malformed_macro_args(
                                    format!(
                                        "macro expansion did not produce a valid pattern: {}",
                                        reason
                                    ),
                                    inv.span,
                                )
                                .with_backtrace(self.backtrace()),
                            );
                            self.after_expand();
                        }
                    },
                    Err(e) => self.errors.push(e),
                }
                (pat.clone(), false)
            }
            PatternKind::Binding {
                name,
                mutability,
                subpattern,
            } => {
                let (new_sub, changed) = if let Some(sub) = subpattern {
                    let (ns, c) = self.expand_pattern(sub);
                    (Some(Box::new(ns)), c)
                } else {
                    (None, false)
                };
                (
                    Pattern {
                        pattern: PatternKind::Binding {
                            name: *name,
                            mutability: mutability.clone(),
                            subpattern: new_sub,
                        },
                        span: pat.span,
                    },
                    changed,
                )
            }
            PatternKind::Tuple { patterns }
            | PatternKind::Slice { patterns }
            | PatternKind::Or(patterns) => {
                let mut new_patterns = vec![];
                let mut changed = false;
                for p in patterns {
                    let (np, c) = self.expand_pattern(p);
                    new_patterns.push(np);
                    changed |= c;
                }
                let kind = match &pat.pattern {
                    PatternKind::Tuple { .. } => PatternKind::Tuple {
                        patterns: new_patterns,
                    },
                    PatternKind::Slice { .. } => PatternKind::Slice {
                        patterns: new_patterns,
                    },
                    _ => PatternKind::Or(new_patterns),
                };
                (
                    Pattern {
                        pattern: kind,
                        span: pat.span,
                    },
                    changed,
                )
            }
            PatternKind::Struct { path, fields, rest } => {
                let mut new_fields = vec![];
                let mut changed = false;
                for f in fields {
                    let (np, c) = self.expand_pattern(&f.pattern);
                    changed |= c;
                    new_fields.push(yelang_ast::FieldPattern {
                        name: f.name,
                        pattern: np,
                        is_shorthand: f.is_shorthand,
                        is_placeholder: f.is_placeholder,
                    });
                }
                (
                    Pattern {
                        pattern: PatternKind::Struct {
                            path: path.clone(),
                            fields: new_fields,
                            rest: *rest,
                        },
                        span: pat.span,
                    },
                    changed,
                )
            }
            PatternKind::Record { fields, rest } => {
                let mut new_fields = vec![];
                let mut changed = false;
                for f in fields {
                    let (np, c) = self.expand_pattern(&f.pattern);
                    changed |= c;
                    new_fields.push(yelang_ast::FieldPattern {
                        name: f.name,
                        pattern: np,
                        is_shorthand: f.is_shorthand,
                        is_placeholder: f.is_placeholder,
                    });
                }
                (
                    Pattern {
                        pattern: PatternKind::Record {
                            fields: new_fields,
                            rest: *rest,
                        },
                        span: pat.span,
                    },
                    changed,
                )
            }
            PatternKind::TupleStruct { path, patterns } => {
                let mut new_patterns = vec![];
                let mut changed = false;
                for p in patterns {
                    let (np, c) = self.expand_pattern(p);
                    new_patterns.push(np);
                    changed |= c;
                }
                (
                    Pattern {
                        pattern: PatternKind::TupleStruct {
                            path: path.clone(),
                            patterns: new_patterns,
                        },
                        span: pat.span,
                    },
                    changed,
                )
            }
            PatternKind::Ref { pattern, is_mut } => {
                let (new_pat, changed) = self.expand_pattern(pattern);
                (
                    Pattern {
                        pattern: PatternKind::Ref {
                            pattern: Box::new(new_pat),
                            is_mut: *is_mut,
                        },
                        span: pat.span,
                    },
                    changed,
                )
            }
            PatternKind::Grouped(inner) => {
                let (new_inner, changed) = self.expand_pattern(inner);
                (
                    Pattern {
                        pattern: PatternKind::Grouped(Box::new(new_inner)),
                        span: pat.span,
                    },
                    changed,
                )
            }
            PatternKind::Range(range) => {
                let (new_start, start_changed) = if let Some(start) = &range.start {
                    let (ns, c) = self.expand_expr(start);
                    (Some(ns), c)
                } else {
                    (None, false)
                };
                let (new_end, end_changed) = if let Some(end) = &range.end {
                    let (ne, c) = self.expand_expr(end);
                    (Some(ne), c)
                } else {
                    (None, false)
                };
                (
                    Pattern {
                        pattern: PatternKind::Range(yelang_ast::RangeExpr {
                            start: new_start.map(Box::new),
                            op: range.op.clone(),
                            end: new_end.map(Box::new),
                        }),
                        span: pat.span,
                    },
                    start_changed || end_changed,
                )
            }
            _ => (pat.clone(), false),
        }
    }

    /// Expand all macros in a statement.
    ///
    /// Returns a vector because a statement-position macro invocation may expand
    /// to zero, one, or many statements.
    fn expand_stmt(&mut self, stmt: &Stmt) -> (Vec<Stmt>, bool) {
        match &stmt.kind {
            StmtKind::MacroInvocation(inv) => {
                // Built-in macros expand to expressions; place them back into
                // statement position as a discarded expression statement.
                if let Some(expanded) = expand_builtin_macro(inv, self.interner) {
                    let (recursed, _) = self.expand_expr(&expanded);
                    return (
                        vec![Stmt {
                            kind: StmtKind::TermExpr(Box::new(recursed)),
                            span: stmt.span,
                        }],
                        true,
                    );
                }

                match self.expand_macro_invocation(inv) {
                    Ok(stream) => match parse_stmts_from_token_stream(
                        &stream,
                        self.interner,
                        &self.eager_context(),
                    ) {
                        Ok(stmts) => {
                            let mut expanded = vec![];
                            for s in stmts {
                                let (es, _) = self.expand_stmt(&s);
                                expanded.extend(es);
                            }
                            self.after_expand();
                            return (expanded, true);
                        }
                        Err(reason) => {
                            self.errors.push(
                                ExpandError::malformed_macro_args(
                                    format!(
                                        "macro expansion did not produce valid statements: {}",
                                        reason
                                    ),
                                    inv.span,
                                )
                                .with_backtrace(self.backtrace()),
                            );
                            self.after_expand();
                        }
                    },
                    Err(e) => self.errors.push(e),
                }
                (vec![stmt.clone()], false)
            }
            StmtKind::Expr(expr) => {
                let (new_expr, changed) = self.expand_expr(expr);
                (
                    vec![Stmt {
                        kind: StmtKind::Expr(Box::new(new_expr)),
                        span: stmt.span,
                    }],
                    changed,
                )
            }
            StmtKind::TermExpr(expr) => {
                let (new_expr, changed) = self.expand_expr(expr);
                (
                    vec![Stmt {
                        kind: StmtKind::TermExpr(Box::new(new_expr)),
                        span: stmt.span,
                    }],
                    changed,
                )
            }
            StmtKind::Let(let_stmt) => {
                let (new_pattern, pattern_changed) = self.expand_pattern(&let_stmt.pattern);
                let (new_ty, ty_changed) = if let Some(ty) = &let_stmt.ty {
                    let (nt, c) = self.expand_type(ty);
                    (Some(nt), c)
                } else {
                    (None, false)
                };
                let (new_init, init_changed) = if let Some(init) = &let_stmt.init {
                    let (e, c) = self.expand_expr(init);
                    (Some(Box::new(e)), c)
                } else {
                    (None, false)
                };
                (
                    vec![Stmt {
                        kind: StmtKind::Let(Box::new(yelang_ast::LetStmt {
                            pattern: Box::new(new_pattern),
                            ty: new_ty.map(Box::new),
                            init: new_init,
                            span: let_stmt.span,
                            attrs: let_stmt.attrs.clone(),
                        })),
                        span: stmt.span,
                    }],
                    pattern_changed || ty_changed || init_changed,
                )
            }
            StmtKind::Item(item) => {
                match self.expand_item(*item.clone()) {
                    Ok(expanded) => {
                        if expanded.len() > 1 {
                            // Decorators that generate side-items are not supported
                            // inside statement position.  Emit an error and keep only
                            // the primary item.
                            self.errors.push(
                                ExpandError::decorator_error(
                                    "decorator produced multiple items in statement position"
                                        .to_string(),
                                    stmt.span,
                                )
                                .with_backtrace(self.backtrace()),
                            );
                        }
                        let primary = expanded.into_iter().next().unwrap_or_else(|| *item.clone());
                        (
                            vec![Stmt {
                                kind: StmtKind::Item(Box::new(primary)),
                                span: stmt.span,
                            }],
                            true,
                        )
                    }
                    Err(e) => {
                        self.errors.push(e);
                        (vec![stmt.clone()], false)
                    }
                }
            }
            StmtKind::Empty => (vec![stmt.clone()], false),
        }
    }

    /// Expand all macros in an expression, recursively.
    /// Returns (expanded_expr, whether_anything_changed).
    fn expand_expr(&mut self, expr: &Expr) -> (Expr, bool) {
        match &expr.kind {
            ExprKind::MacroInvocation(inv) => {
                if let Some(expanded) = expand_builtin_macro(inv, self.interner) {
                    let (recursed, _) = self.expand_expr(&expanded);
                    return (recursed, true);
                }
                match self.expand_macro_invocation(inv) {
                    Ok(stream) => match parse_expr_from_token_stream(
                        &stream,
                        self.interner,
                        &self.eager_context(),
                    ) {
                        Ok(expanded) => {
                            let (recursed, _) = self.expand_expr(&expanded);
                            self.after_expand();
                            return (recursed, true);
                        }
                        Err(reason) => {
                            self.errors.push(
                                ExpandError::malformed_macro_args(
                                    format!(
                                        "macro expansion did not produce a valid expression: {}",
                                        reason
                                    ),
                                    inv.span,
                                )
                                .with_backtrace(self.backtrace()),
                            );
                            self.after_expand();
                        }
                    },
                    Err(e) => self.errors.push(e),
                }
                (expr.clone(), false)
            }
            ExprKind::Binary(bin) => {
                let (left, left_changed) = self.expand_expr(&bin.left);
                let (right, right_changed) = self.expand_expr(&bin.right);
                (
                    Expr {
                        kind: ExprKind::Binary(BinaryExpr {
                            left: Box::new(left),
                            op: bin.op,
                            right: Box::new(right),
                        }),
                        span: expr.span,
                    },
                    left_changed || right_changed,
                )
            }
            ExprKind::Unary(un) => {
                let (inner, changed) = self.expand_expr(&un.expr);
                (
                    Expr {
                        kind: ExprKind::Unary(UnaryExpr {
                            op: un.op,
                            expr: Box::new(inner),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::If(if_expr) => {
                let (cond, cond_changed) = self.expand_expr(&if_expr.condition);
                let (then_block, then_changed) = self.expand_block_expr(&if_expr.then_block);
                let (else_expr, else_changed) = if let Some(e) = &if_expr.else_expr {
                    let (exp, ch) = self.expand_expr(e);
                    (Some(exp), ch)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::If(IfExpr {
                            condition: Box::new(cond),
                            then_block,
                            else_expr: else_expr.map(Box::new),
                        }),
                        span: expr.span,
                    },
                    cond_changed || then_changed || else_changed,
                )
            }
            ExprKind::Block(block) => {
                let (new_block, changed) = self.expand_block_expr(block);
                (
                    Expr {
                        kind: ExprKind::Block(new_block),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Call(call) => {
                let (callee, callee_changed) = self.expand_expr(&call.callee);
                let mut args = vec![];
                let mut args_changed = false;
                for arg in &call.args {
                    let (new_arg, arg_changed) = match arg {
                        yelang_ast::CallArgument::Positional(e) => {
                            let (ne, nc) = self.expand_expr(e);
                            (yelang_ast::CallArgument::Positional(ne), nc)
                        }
                        yelang_ast::CallArgument::Named(id, e) => {
                            let (ne, nc) = self.expand_expr(e);
                            (yelang_ast::CallArgument::Named(*id, ne), nc)
                        }
                    };
                    args.push(new_arg);
                    args_changed |= arg_changed;
                }
                (
                    Expr {
                        kind: ExprKind::Call(yelang_ast::CallExpr {
                            callee: Box::new(callee),
                            args,
                        }),
                        span: expr.span,
                    },
                    callee_changed || args_changed,
                )
            }
            ExprKind::Match(match_expr) => {
                let (scrutinee, scrut_changed) = self.expand_expr(&match_expr.scrutinee);
                let mut arms = vec![];
                let mut arms_changed = false;
                for arm in &match_expr.arms {
                    let (pattern, pattern_changed) = self.expand_pattern(&arm.pattern);
                    let (body, body_changed) = self.expand_expr(&arm.body);
                    let (guard, guard_changed) = if let Some(g) = &arm.guard {
                        let (ng, nc) = self.expand_expr(g);
                        (Some(ng), nc)
                    } else {
                        (None, false)
                    };
                    arms.push(yelang_ast::MatchArm {
                        pattern,
                        guard: guard.map(Box::new),
                        body: Box::new(body),
                        span: arm.span,
                    });
                    arms_changed |= pattern_changed || body_changed || guard_changed;
                }
                (
                    Expr {
                        kind: ExprKind::Match(Box::new(yelang_ast::MatchExpr {
                            scrutinee: Box::new(scrutinee),
                            arms,
                        })),
                        span: expr.span,
                    },
                    scrut_changed || arms_changed,
                )
            }
            ExprKind::Lambda(lambda) => {
                let mut fn_sig = lambda.fn_sig.clone();
                let sig_changed = self.expand_fn_sig(&mut fn_sig);
                let (body, body_changed) = self.expand_expr(&lambda.body);
                (
                    Expr {
                        kind: ExprKind::Lambda(yelang_ast::LambdaExpr {
                            header_span: lambda.header_span,
                            fn_sig,
                            body: Box::new(body),
                        }),
                        span: expr.span,
                    },
                    sig_changed || body_changed,
                )
            }
            ExprKind::Return(ret) => {
                if let Some(e) = ret {
                    let (ne, changed) = self.expand_expr(e);
                    (
                        Expr {
                            kind: ExprKind::Return(Some(Box::new(ne))),
                            span: expr.span,
                        },
                        changed,
                    )
                } else {
                    (expr.clone(), false)
                }
            }
            ExprKind::Break(break_expr) => {
                let (value, changed) = if let Some(v) = &break_expr.value {
                    let (nv, nc) = self.expand_expr(v);
                    (Some(nv), nc)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::Break(yelang_ast::BreakExpr {
                            label: break_expr.label.clone(),
                            value: value.map(Box::new),
                            span: break_expr.span,
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::AssignEq(assign) => {
                let (value, changed) = self.expand_expr(&assign.value);
                (
                    Expr {
                        kind: ExprKind::AssignEq(AssignEqExpr {
                            target: Box::new(*assign.target.clone()),
                            value: Box::new(value),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::AssignOp(assign) => {
                let (value, changed) = self.expand_expr(&assign.value);
                (
                    Expr {
                        kind: ExprKind::AssignOp(AssignOpExpr {
                            target: Box::new(*assign.target.clone()),
                            value: Box::new(value),
                            op: assign.op.clone(),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Tuple(exprs) => {
                let mut new_exprs = vec![];
                let mut changed = false;
                for e in exprs {
                    let (ne, nc) = self.expand_expr(e);
                    new_exprs.push(ne);
                    changed |= nc;
                }
                (
                    Expr {
                        kind: ExprKind::Tuple(new_exprs),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Array(arr) => {
                let mut new_elements = vec![];
                let mut changed = false;
                if let Some(elements) = arr.elements() {
                    for e in elements {
                        let (ne, nc) = self.expand_expr(e);
                        new_elements.push(ne);
                        changed |= nc;
                    }
                }
                (
                    Expr {
                        kind: ExprKind::Array(yelang_ast::Array {
                            kind: yelang_ast::ArrayKind::List(new_elements),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Struct(struct_expr) => {
                let mut new_fields = vec![];
                let mut changed = false;
                for field in &struct_expr.fields {
                    let (ne, nc) = self.expand_expr(&field.value);
                    new_fields.push(yelang_ast::FieldAssign {
                        name: field.name,
                        value: ne,
                        is_shorthand: field.is_shorthand,
                        span: field.span,
                    });
                    changed |= nc;
                }
                let (rest, rest_changed) = if let Some(r) = &struct_expr.rest {
                    let (nr, nc) = self.expand_expr(r);
                    (Some(Box::new(nr)), nc)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::Struct(yelang_ast::StructExpr {
                            path: struct_expr.path.clone(),
                            fields: new_fields,
                            rest,
                        }),
                        span: expr.span,
                    },
                    changed || rest_changed,
                )
            }
            ExprKind::MemberAccess(access) => {
                let (base, changed) = self.expand_expr(access.base());
                (
                    Expr {
                        kind: ExprKind::MemberAccess(MemberAccess {
                            base: Box::new(base),
                            member: *access.member(),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::ArrayAccess(access) => {
                let (base, base_changed) = self.expand_expr(access.base());
                // For MVP, we only handle simple single indices.
                let (index, index_changed) = match access.index() {
                    yelang_ast::ArrayIndex::Single(idx) => {
                        let (ne, nc) = self.expand_expr(idx.expr());
                        (
                            yelang_ast::ArrayIndex::Single(yelang_ast::Index(Box::new(ne))),
                            nc,
                        )
                    }
                    other => (other.clone(), false),
                };
                (
                    Expr {
                        kind: ExprKind::ArrayAccess(yelang_ast::ArrayAccess {
                            base: Box::new(base),
                            index,
                        }),
                        span: expr.span,
                    },
                    base_changed || index_changed,
                )
            }
            ExprKind::MethodCall(method) => {
                let (receiver, recv_changed) = self.expand_expr(&method.receiver);
                let mut args = vec![];
                let mut args_changed = false;
                for arg in &method.arguments {
                    let (new_arg, arg_changed) = match arg {
                        yelang_ast::CallArgument::Positional(e) => {
                            let (ne, nc) = self.expand_expr(e);
                            (yelang_ast::CallArgument::Positional(ne), nc)
                        }
                        yelang_ast::CallArgument::Named(id, e) => {
                            let (ne, nc) = self.expand_expr(e);
                            (yelang_ast::CallArgument::Named(*id, ne), nc)
                        }
                    };
                    args.push(new_arg);
                    args_changed |= arg_changed;
                }
                (
                    Expr {
                        kind: ExprKind::MethodCall(yelang_ast::MethodCallExpr {
                            receiver: Box::new(receiver),
                            segment: method.segment.clone(),
                            arguments: args,
                        }),
                        span: expr.span,
                    },
                    recv_changed || args_changed,
                )
            }
            ExprKind::TypeCast(cast) => {
                let (base, base_changed) = self.expand_expr(&cast.base);
                let (new_ty, ty_changed) = self.expand_type(&cast.ty);
                (
                    Expr {
                        kind: ExprKind::TypeCast(yelang_ast::TypeCast {
                            base: Box::new(base),
                            ty: new_ty,
                        }),
                        span: expr.span,
                    },
                    base_changed || ty_changed,
                )
            }
            ExprKind::TypeAscription(asc) => {
                let (new_expr, expr_changed) = self.expand_expr(&asc.expr);
                let (new_ty, ty_changed) = self.expand_type(&asc.ty);
                (
                    Expr {
                        kind: ExprKind::TypeAscription(yelang_ast::TypeAscription {
                            expr: Box::new(new_expr),
                            ty: new_ty,
                        }),
                        span: expr.span,
                    },
                    expr_changed || ty_changed,
                )
            }
            ExprKind::IsType(is_type) => {
                let (new_expr, expr_changed) = self.expand_expr(&is_type.expr);
                let (new_ty, ty_changed) = self.expand_type(&is_type.ty);
                (
                    Expr {
                        kind: ExprKind::IsType(yelang_ast::IsTypeExpr {
                            expr: Box::new(new_expr),
                            ty: new_ty,
                        }),
                        span: expr.span,
                    },
                    expr_changed || ty_changed,
                )
            }
            ExprKind::Try(try_expr) => {
                let (base, changed) = self.expand_expr(&try_expr.base);
                (
                    Expr {
                        kind: ExprKind::Try(yelang_ast::TrySafeAccess {
                            base: Box::new(base),
                            op: try_expr.op,
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::ForLoop(for_loop) => {
                let (pat, pat_changed) = self.expand_pattern(&for_loop.pat);
                let (iter, iter_changed) = self.expand_expr(&for_loop.iter);
                let (body, body_changed) = self.expand_block_expr(&for_loop.body);
                (
                    Expr {
                        kind: ExprKind::ForLoop(yelang_ast::ForLoopExpr {
                            pat,
                            label: for_loop.label.clone(),
                            iter: Box::new(iter),
                            body,
                        }),
                        span: expr.span,
                    },
                    pat_changed || iter_changed || body_changed,
                )
            }
            ExprKind::While(while_expr) => {
                let (cond, cond_changed) = self.expand_expr(&while_expr.condition);
                let (body, body_changed) = self.expand_block_expr(&while_expr.body);
                (
                    Expr {
                        kind: ExprKind::While(yelang_ast::WhileExpr {
                            label: while_expr.label.clone(),
                            condition: Box::new(cond),
                            body,
                        }),
                        span: expr.span,
                    },
                    cond_changed || body_changed,
                )
            }
            ExprKind::Loop(loop_expr) => {
                let (body, changed) = self.expand_block_expr(&loop_expr.body);
                (
                    Expr {
                        kind: ExprKind::Loop(Box::new(yelang_ast::LoopExpr {
                            label: loop_expr.label.clone(),
                            body: Box::new(body),
                        })),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Await(e) => {
                let (inner, changed) = self.expand_expr(e);
                (
                    Expr {
                        kind: ExprKind::Await(Box::new(inner)),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Gen(e) => {
                let (inner, changed) = self.expand_expr(e);
                (
                    Expr {
                        kind: ExprKind::Gen(Box::new(inner)),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Grouped(g) => {
                let (inner, changed) = self.expand_expr(&g.expr);
                (
                    Expr {
                        kind: ExprKind::Grouped(yelang_ast::GroupedExpr {
                            expr: Box::new(inner),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Range(range) => {
                let (start, start_changed) = if let Some(s) = &range.start {
                    let (ns, nc) = self.expand_expr(s);
                    (Some(Box::new(ns)), nc)
                } else {
                    (None, false)
                };
                let (end, end_changed) = if let Some(e) = &range.end {
                    let (ne, nc) = self.expand_expr(e);
                    (Some(Box::new(ne)), nc)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::Range(yelang_ast::RangeExpr {
                            start,
                            end,
                            op: range.op.clone(),
                        }),
                        span: expr.span,
                    },
                    start_changed || end_changed,
                )
            }
            ExprKind::Let(let_expr) => {
                let (new_pattern, pattern_changed) = self.expand_pattern(&let_expr.pattern);
                let (new_expr, expr_changed) = self.expand_expr(&let_expr.expr);
                (
                    Expr {
                        kind: ExprKind::Let(yelang_ast::LetExpr {
                            pattern: new_pattern,
                            expr: Box::new(new_expr),
                        }),
                        span: expr.span,
                    },
                    pattern_changed || expr_changed,
                )
            }
            ExprKind::Comprehension(comp) => {
                let (element, elem_changed) = self.expand_expr(&comp.element);
                let mut vars = vec![];
                let mut vars_changed = false;
                for var in &comp.variables {
                    let (pattern, pattern_changed) = self.expand_pattern(&var.pattern);
                    let (source, source_changed) = self.expand_expr(&var.source);
                    vars.push(yelang_ast::ComprehensionVar {
                        pattern,
                        source: Box::new(source),
                    });
                    vars_changed |= pattern_changed || source_changed;
                }
                let (cond, cond_changed) = if let Some(c) = &comp.condition {
                    let (nc, cc) = self.expand_expr(c);
                    (Some(Box::new(nc)), cc)
                } else {
                    (None, false)
                };
                (
                    Expr {
                        kind: ExprKind::Comprehension(yelang_ast::ComprehensionExpr {
                            element: Box::new(element),
                            variables: vars,
                            condition: cond,
                        }),
                        span: expr.span,
                    },
                    elem_changed || vars_changed || cond_changed,
                )
            }
            ExprKind::Ternary(ternary) => {
                let (cond, cond_changed) = self.expand_expr(&ternary.condition);
                let (if_true, if_true_changed) = self.expand_expr(&ternary.if_true);
                let (if_false, if_false_changed) = self.expand_expr(&ternary.if_false);
                (
                    Expr {
                        kind: ExprKind::Ternary(yelang_ast::TernaryExpr {
                            condition: Box::new(cond),
                            if_true: Box::new(if_true),
                            if_false: Box::new(if_false),
                        }),
                        span: expr.span,
                    },
                    cond_changed || if_true_changed || if_false_changed,
                )
            }
            ExprKind::BindAt(bind) => {
                let (base, changed) = self.expand_expr(&bind.base);
                (
                    Expr {
                        kind: ExprKind::BindAt(yelang_ast::BindAtExpr {
                            base: Box::new(base),
                            at: bind.at,
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Async(async_expr) => {
                let (block, changed) = self.expand_block_expr(&async_expr.block);
                (
                    Expr {
                        kind: ExprKind::Async(yelang_ast::AsyncExpr {
                            block: Box::new(block),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::Object(obj) => {
                let mut new_fields = vec![];
                let mut changed = false;
                for field in obj.fields() {
                    let (val, val_changed) = self.expand_expr(field.value());
                    new_fields.push(yelang_ast::ObjectField::new(*field.key(), val));
                    changed |= val_changed;
                }
                (
                    Expr {
                        kind: ExprKind::Object(yelang_ast::Object {
                            fields: new_fields,
                            span: obj.span,
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            ExprKind::DocumentAccess(doc) => {
                let (base, base_changed) = self.expand_expr(doc.base());
                let mut new_fields = vec![];
                let mut fields_changed = false;
                for field in doc.object().fields() {
                    match field {
                        yelang_ast::DocumentField::KeyVal(kv) => {
                            let (val, val_changed) = self.expand_expr(&kv.value);
                            new_fields.push(yelang_ast::DocumentField::KeyVal(
                                yelang_ast::KeyVal {
                                    key: kv.key,
                                    value: val,
                                },
                            ));
                            fields_changed |= val_changed;
                        }
                        other => new_fields.push(other.clone()),
                    }
                }
                (
                    Expr {
                        kind: ExprKind::DocumentAccess(yelang_ast::DocumentAccess {
                            base: Box::new(base),
                            object: yelang_ast::Document {
                                fields: new_fields,
                                span: doc.object().span,
                            },
                        }),
                        span: expr.span,
                    },
                    base_changed || fields_changed,
                )
            }
            ExprKind::DestructureAssign(assign) => {
                let (value, changed) = self.expand_expr(&assign.value);
                (
                    Expr {
                        kind: ExprKind::DestructureAssign(yelang_ast::DestructureAssignExpr {
                            pattern: assign.pattern.clone(),
                            value: Box::new(value),
                        }),
                        span: expr.span,
                    },
                    changed,
                )
            }
            // Literals, paths, and other leaf nodes don't need expansion.
            _ => (expr.clone(), false),
        }
    }

    /// True if `attr` names a user-defined attribute macro (declarative,
    /// in-process procedural, or out-of-process procedural).
    fn is_user_attribute_macro(&self, attr: &Attribute) -> bool {
        let Some(name) = attr.path.first() else {
            return false;
        };
        let name_str = self.interner.resolve(&name.symbol);
        if self
            .resolver
            .resolve(name_str)
            .map(|mac| mac.rules.iter().any(|r| r.kind == MacroKind::Attribute))
            .unwrap_or(false)
        {
            return true;
        }
        if let Some(executor) = self.in_process_executor.clone()
            && let Some(mac) = executor.find(name_str)
        {
            return mac.kind() == yelang_proc_macro_bridge::protocol::ProcMacroKind::Attribute;
        }
        if let Some(runtime) = self.proc_macro_runtime.as_ref() {
            return runtime
                .resolver()
                .resolve(
                    name_str,
                    yelang_proc_macro_bridge::protocol::ProcMacroKind::Attribute,
                )
                .is_some();
        }
        false
    }

    /// Expand a user-defined attribute macro.
    ///
    /// Returns `Some(items)` if the attribute name resolves to a macro with at
    /// least one `Attribute` rule, even if expansion fails (in which case the
    /// original item is returned and an error is recorded). Returns `None` if
    /// the attribute is not a user macro, so callers can fall through to
    /// built-in decorators.
    fn expand_user_attribute_macro(
        &mut self,
        attr: &Attribute,
        item: &Item,
        require_unsafe: bool,
    ) -> Option<Vec<Item>> {
        let name = attr.path.first()?;
        let name_str = self.interner.resolve(&name.symbol).to_string();
        let span = attr.span;

        // In-process procedural attribute macros take priority over declarative rules.
        if let Some(executor) = self.in_process_executor.clone()
            && let Some(mac) = executor.find(&name_str)
            && mac.kind() == yelang_proc_macro_bridge::protocol::ProcMacroKind::Attribute
        {
            let Some(_) =
                self.before_expand(&name_str, span, MacroFrameId::ProcMacro(name_str.clone()))
            else {
                return Some(vec![item.clone()]);
            };
            let attr_args_stream = attribute_args_to_token_stream(&attr.args, self.interner)?;
            let item_stream = item_to_token_stream(item, self.interner)?;
            let proc_args =
                yelang_proc_macro::TokenStream::from_core_stream(&attr_args_stream, self.interner);
            let proc_item =
                yelang_proc_macro::TokenStream::from_core_stream(&item_stream, self.interner);
            let result = self.expand_in_process_attr(mac, proc_args, proc_item, span);
            self.after_expand();
            return Some(match result {
                Ok(stream) => {
                    parse_items_from_token_stream(&stream, self.interner, &self.eager_context())
                        .unwrap_or_else(|reason| {
                            self.errors.push(
                        ExpandError::malformed_macro_args(
                            format!(
                                "attribute macro `{}` expansion did not produce valid items: {}",
                                name_str, reason
                            ),
                            span,
                        )
                        .with_backtrace(self.backtrace()),
                    );
                            vec![item.clone()]
                        })
                }
                Err(e) => {
                    self.errors.push(e);
                    vec![item.clone()]
                }
            });
        }

        // Out-of-process procedural attribute macros take priority over declarative rules.
        if let Some(runtime) = self.proc_macro_runtime.as_ref()
            && let Some(result) = runtime.resolve(
                &name_str,
                yelang_proc_macro_bridge::protocol::ProcMacroKind::Attribute,
            )
        {
            let mac = match result {
                Ok(mac) => mac,
                Err(e) => {
                    self.errors.push(
                        ExpandError::malformed_macro_args(
                            format!("failed to load proc macro `{}`: {}", name_str, e),
                            span,
                        )
                        .with_backtrace(self.backtrace()),
                    );
                    return Some(vec![item.clone()]);
                }
            };
            let Some(_) =
                self.before_expand(&name_str, span, MacroFrameId::ProcMacro(name_str.clone()))
            else {
                return Some(vec![item.clone()]);
            };
            let attr_args_stream = attribute_args_to_token_stream(&attr.args, self.interner)?;
            let item_stream = item_to_token_stream(item, self.interner)?;
            let result = self.expand_server_attr(&mac, attr_args_stream, item_stream, span);
            self.after_expand();
            return Some(match result {
                Ok(stream) => {
                    parse_items_from_token_stream(&stream, self.interner, &self.eager_context())
                        .unwrap_or_else(|reason| {
                            self.errors.push(
                        ExpandError::malformed_macro_args(
                            format!(
                                "attribute macro `{}` expansion did not produce valid items: {}",
                                name_str, reason
                            ),
                            span,
                        )
                        .with_backtrace(self.backtrace()),
                    );
                            vec![item.clone()]
                        })
                }
                Err(e) => {
                    self.errors.push(e);
                    vec![item.clone()]
                }
            });
        }

        let mac = self.resolver.resolve(&name_str)?.clone();

        let rules: Vec<&crate::matcher::types::MacroRule> = mac
            .rules
            .iter()
            .filter(|r| r.kind == MacroKind::Attribute)
            .collect();
        if rules.is_empty() {
            return None;
        }

        let attr_args_stream = attribute_args_to_token_stream(&attr.args, self.interner)?;
        let item_stream = item_to_token_stream(item, self.interner)?;

        let Some(MacroFrameId::Declarative(def_id)) =
            self.before_expand(&name_str, span, MacroFrameId::Declarative(mac.def_id))
        else {
            return Some(vec![item.clone()]);
        };

        let matches = self.match_attribute_rules(
            &name_str,
            &rules,
            &attr_args_stream,
            &item_stream,
            span,
            require_unsafe,
        );

        let (rule, bindings) = match matches {
            RuleMatches::Matched { rule, bindings } => (rule, bindings),
            RuleMatches::NoMatch { unsafe_available } => {
                let msg = if unsafe_available && !require_unsafe {
                    format!(
                        "attribute macro `{}` has only unsafe rules; use `#[unsafe({}(...))]` or `@unsafe({}(...))`",
                        name_str, name_str, name_str
                    )
                } else {
                    "no attribute rule matched the invocation".to_string()
                };
                self.errors.push(
                    ExpandError::macro_match_error(name_str.clone(), msg, span)
                        .with_backtrace(self.backtrace()),
                );
                self.after_expand();
                return Some(vec![item.clone()]);
            }
            RuleMatches::Ambiguous => {
                self.errors.push(
                    ExpandError::ambiguous_macro(name_str.clone(), span)
                        .with_backtrace(self.backtrace()),
                );
                self.after_expand();
                return Some(vec![item.clone()]);
            }
        };

        let expanded_stream = match self.transcribe_rule(&rule, &bindings, &name_str, span, def_id)
        {
            Some(stream) => stream,
            None => {
                self.after_expand();
                return Some(vec![item.clone()]);
            }
        };

        self.after_expand();
        match parse_items_from_token_stream(&expanded_stream, self.interner, &self.eager_context())
        {
            Ok(items) => Some(items),
            Err(reason) => {
                self.errors.push(
                    ExpandError::malformed_macro_args(
                        format!(
                            "attribute macro `{}` expansion did not produce valid items: {}",
                            name_str, reason
                        ),
                        span,
                    )
                    .with_backtrace(self.backtrace()),
                );
                Some(vec![item.clone()])
            }
        }
    }

    fn match_attribute_rules(
        &mut self,
        name_str: &str,
        rules: &[&crate::matcher::types::MacroRule],
        attr_args_stream: &yelang_macro_core::token_tree::TokenStream,
        item_stream: &yelang_macro_core::token_tree::TokenStream,
        span: yelang_lexer::Span,
        require_unsafe: bool,
    ) -> RuleMatches {
        let mut safe_matches = Vec::new();
        let mut unsafe_matches = Vec::new();

        for rule in rules {
            let attr_bindings = try_match_matcher(&rule.attr_args, attr_args_stream, self.interner);
            let (delimiter, item_matcher) = item_matcher_ops(&rule.matcher);
            let wrapped_item_stream = wrap_item_stream(item_stream.clone(), delimiter);
            let item_bindings =
                try_match_matcher(item_matcher, &wrapped_item_stream, self.interner);
            if let (Ok(attr_bindings), Ok(item_bindings)) = (attr_bindings, item_bindings) {
                let mut combined = attr_bindings;
                combined.extend(item_bindings);
                if rule.is_unsafe {
                    unsafe_matches.push((*rule, combined));
                } else {
                    safe_matches.push((*rule, combined));
                }
            }
        }

        if require_unsafe {
            if !unsafe_matches.is_empty() {
                return RuleMatches::from_vec(unsafe_matches);
            }
            // Unsafe wrapper used but no unsafe rule matched: warn and fall back
            // to safe rules (the wrapper is accepted as a no-op).
            if !safe_matches.is_empty() {
                self.errors.push(
                    ExpandError::decorator_error(
                        format!(
                            "`unsafe(...)` wrapper on attribute macro `{}` is unnecessary; the macro has no unsafe rules",
                            name_str
                        ),
                        span,
                    )
                    .with_backtrace(self.backtrace()),
                );
                return RuleMatches::from_vec(safe_matches);
            }
        } else {
            if !safe_matches.is_empty() {
                return RuleMatches::from_vec(safe_matches);
            }
            if !unsafe_matches.is_empty() {
                return RuleMatches::NoMatch {
                    unsafe_available: true,
                };
            }
        }

        RuleMatches::NoMatch {
            unsafe_available: !unsafe_matches.is_empty(),
        }
    }

    /// Expand `@derive(A, B, C)`, invoking user-defined derive macros when
    /// available and falling back to built-in derives otherwise.
    fn expand_derive_attribute(&mut self, attr: &Attribute, item: &Item) -> Vec<Item> {
        let invocations =
            crate::builtin_decorators::collect_derive_invocations(&attr.args, self.interner);
        let span = attr.span;
        let mut result = vec![item.clone()];

        for (trait_name, is_unsafe) in invocations {
            if let Some(generated) =
                self.expand_user_derive_macro(trait_name.as_str(), item, span, is_unsafe)
            {
                result.extend(generated);
                continue;
            }

            // Fall back to built-in derive.
            match crate::builtin_decorators::generate_derive_impl(
                trait_name.as_str(),
                item,
                self.interner,
            ) {
                Some(impl_item) => result.push(impl_item),
                None => {
                    self.errors.push(
                        ExpandError::decorator_error(
                            format!("@derive does not support trait `{}`", trait_name),
                            span,
                        )
                        .with_backtrace(self.backtrace()),
                    );
                }
            }
        }

        result
    }

    /// Expand a single user-defined derive macro.
    fn expand_user_derive_macro(
        &mut self,
        trait_name: &str,
        item: &Item,
        span: yelang_lexer::Span,
        require_unsafe: bool,
    ) -> Option<Vec<Item>> {
        // In-process procedural derive macros take priority over declarative rules.
        if let Some(executor) = self.in_process_executor.clone()
            && let Some(mac) = executor.find(trait_name)
            && mac.kind() == yelang_proc_macro_bridge::protocol::ProcMacroKind::Derive
        {
            let Some(_) = self.before_expand(
                trait_name,
                span,
                MacroFrameId::ProcMacro(trait_name.to_string()),
            ) else {
                return Some(vec![]);
            };
            let item_stream = item_to_token_stream(item, self.interner)?;
            let proc_item =
                yelang_proc_macro::TokenStream::from_core_stream(&item_stream, self.interner);
            let result = self.expand_in_process_derive(mac, proc_item, span);
            self.after_expand();
            return Some(match result {
                Ok(stream) => {
                    parse_items_from_token_stream(&stream, self.interner, &self.eager_context())
                        .unwrap_or_else(|reason| {
                            self.errors.push(
                        ExpandError::malformed_macro_args(
                            format!(
                                "derive macro `{}` expansion did not produce valid items: {}",
                                trait_name, reason
                            ),
                            span,
                        )
                        .with_backtrace(self.backtrace()),
                    );
                            vec![]
                        })
                }
                Err(e) => {
                    self.errors.push(e);
                    vec![]
                }
            });
        }

        // Out-of-process procedural derive macros take priority over declarative rules.
        if let Some(runtime) = self.proc_macro_runtime.as_ref()
            && let Some(result) = runtime.resolve(
                trait_name,
                yelang_proc_macro_bridge::protocol::ProcMacroKind::Derive,
            )
        {
            let mac = match result {
                Ok(mac) => mac,
                Err(e) => {
                    self.errors.push(
                        ExpandError::malformed_macro_args(
                            format!("failed to load proc macro `{}`: {}", trait_name, e),
                            span,
                        )
                        .with_backtrace(self.backtrace()),
                    );
                    return Some(vec![]);
                }
            };
            let Some(_) = self.before_expand(
                trait_name,
                span,
                MacroFrameId::ProcMacro(trait_name.to_string()),
            ) else {
                return Some(vec![]);
            };
            let item_stream = item_to_token_stream(item, self.interner)?;
            let result = self.expand_server_derive(&mac, item_stream, span);
            self.after_expand();
            return Some(match result {
                Ok(stream) => {
                    parse_items_from_token_stream(&stream, self.interner, &self.eager_context())
                        .unwrap_or_else(|reason| {
                            self.errors.push(
                            ExpandError::malformed_macro_args(
                                format!(
                                    "derive macro `{}` expansion did not produce valid items: {}",
                                    trait_name, reason
                                ),
                                span,
                            )
                            .with_backtrace(self.backtrace()),
                        );
                            vec![]
                        })
                }
                Err(e) => {
                    self.errors.push(e);
                    vec![]
                }
            });
        }

        let mac = self.resolver.resolve(trait_name)?.clone();
        let rules: Vec<&crate::matcher::types::MacroRule> = mac
            .rules
            .iter()
            .filter(|r| r.kind == MacroKind::Derive)
            .collect();
        if rules.is_empty() {
            return None;
        }

        let item_stream = item_to_token_stream(item, self.interner)?;

        let Some(MacroFrameId::Declarative(def_id)) =
            self.before_expand(trait_name, span, MacroFrameId::Declarative(mac.def_id))
        else {
            return Some(vec![]);
        };

        let matches =
            self.match_derive_rules(trait_name, &rules, &item_stream, span, require_unsafe);

        let (rule, bindings) = match matches {
            RuleMatches::Matched { rule, bindings } => (rule, bindings),
            RuleMatches::NoMatch { unsafe_available } => {
                let msg = if unsafe_available && !require_unsafe {
                    format!(
                        "derive macro `{}` has only unsafe rules; use `#[derive(unsafe({}))]` or `@derive(unsafe({}))`",
                        trait_name, trait_name, trait_name
                    )
                } else {
                    "no derive rule matched the item".to_string()
                };
                self.errors.push(
                    ExpandError::macro_match_error(trait_name.to_string(), msg, span)
                        .with_backtrace(self.backtrace()),
                );
                self.after_expand();
                return Some(vec![]);
            }
            RuleMatches::Ambiguous => {
                self.errors.push(
                    ExpandError::ambiguous_macro(trait_name.to_string(), span)
                        .with_backtrace(self.backtrace()),
                );
                self.after_expand();
                return Some(vec![]);
            }
        };

        let expanded_stream = match self.transcribe_rule(&rule, &bindings, trait_name, span, def_id)
        {
            Some(stream) => stream,
            None => {
                self.after_expand();
                return Some(vec![]);
            }
        };

        self.after_expand();
        match parse_items_from_token_stream(&expanded_stream, self.interner, &self.eager_context())
        {
            Ok(items) => Some(items),
            Err(reason) => {
                self.errors.push(
                    ExpandError::malformed_macro_args(
                        format!(
                            "derive macro `{}` expansion did not produce valid items: {}",
                            trait_name, reason
                        ),
                        span,
                    )
                    .with_backtrace(self.backtrace()),
                );
                Some(vec![])
            }
        }
    }

    fn match_derive_rules(
        &mut self,
        trait_name: &str,
        rules: &[&crate::matcher::types::MacroRule],
        item_stream: &yelang_macro_core::token_tree::TokenStream,
        span: yelang_lexer::Span,
        require_unsafe: bool,
    ) -> RuleMatches {
        let mut safe_matches = Vec::new();
        let mut unsafe_matches = Vec::new();

        for rule in rules {
            let attr_bindings =
                try_match_matcher(&rule.attr_args, &TokenStream::new(), self.interner);
            let (delimiter, item_matcher) = item_matcher_ops(&rule.matcher);
            let wrapped_item_stream = wrap_item_stream(item_stream.clone(), delimiter);
            let item_bindings =
                try_match_matcher(item_matcher, &wrapped_item_stream, self.interner);
            if let (Ok(attr_bindings), Ok(item_bindings)) = (attr_bindings, item_bindings) {
                let mut combined = attr_bindings;
                combined.extend(item_bindings);
                if rule.is_unsafe {
                    unsafe_matches.push((*rule, combined));
                } else {
                    safe_matches.push((*rule, combined));
                }
            }
        }

        if require_unsafe {
            if !unsafe_matches.is_empty() {
                return RuleMatches::from_vec(unsafe_matches);
            }
            // Unsafe wrapper used but no unsafe rule matched: warn and fall back
            // to safe rules (the wrapper is accepted as a no-op).
            if !safe_matches.is_empty() {
                self.errors.push(
                    ExpandError::decorator_error(
                        format!(
                            "`unsafe(...)` wrapper on derive macro `{}` is unnecessary; the macro has no unsafe rules",
                            trait_name
                        ),
                        span,
                    )
                    .with_backtrace(self.backtrace()),
                );
                return RuleMatches::from_vec(safe_matches);
            }
        } else {
            if !safe_matches.is_empty() {
                return RuleMatches::from_vec(safe_matches);
            }
            if !unsafe_matches.is_empty() {
                return RuleMatches::NoMatch {
                    unsafe_available: true,
                };
            }
        }

        RuleMatches::NoMatch {
            unsafe_available: !unsafe_matches.is_empty(),
        }
    }

    /// Book-keeping before expanding a macro.
    ///
    /// Returns the frame identifier of the macro being expanded, or `None` if
    /// an expansion loop was detected or the maximum expansion count was reached.
    fn before_expand(
        &mut self,
        name: &str,
        span: yelang_lexer::Span,
        frame_id: MacroFrameId,
    ) -> Option<MacroFrameId> {
        self.expansion_count += 1;
        if self.expansion_count > MAX_EXPANSIONS {
            self.errors.push(
                ExpandError::expansion_loop(name.to_string(), span)
                    .with_backtrace(self.backtrace()),
            );
            return None;
        }

        if self.expansion_stack.len() >= MAX_RECURSION_DEPTH {
            self.errors.push(
                ExpandError::recursion_limit(name.to_string(), span)
                    .with_backtrace(self.backtrace()),
            );
            return None;
        }

        // Indirect recursion detection: if this macro is already on the stack,
        // we have a cycle (a → b → a).
        if self.expansion_stack.iter().any(|f| f.frame_id == frame_id) {
            self.errors.push(
                ExpandError::expansion_loop(name.to_string(), span)
                    .with_backtrace(self.backtrace()),
            );
            return None;
        }

        self.expansion_stack.push(ExpansionFrame {
            name: name.to_string(),
            span,
            frame_id: frame_id.clone(),
        });
        Some(frame_id)
    }

    /// Book-keeping after expanding a macro.
    fn after_expand(&mut self) {
        self.expansion_stack.pop();
    }

    /// Snapshot of the current expansion stack for diagnostic backtraces.
    fn backtrace(&self) -> Vec<crate::error::BacktraceFrame> {
        self.expansion_stack
            .iter()
            .map(|f| crate::error::BacktraceFrame {
                name: f.name.clone(),
                span: f.span,
            })
            .collect()
    }

    /// Transcribe a matched rule, applying hygiene.
    ///
    /// The generated context is parented to the current hygiene context so that
    /// nested macro expansions form a proper chain rather than collapsing back
    /// to the root context.
    fn transcribe_rule(
        &mut self,
        rule: &crate::matcher::types::MacroRule,
        bindings: &crate::matcher::bindings::Bindings,
        name: &str,
        span: yelang_lexer::Span,
        def_id: MacroDefId,
    ) -> Option<TokenStream> {
        let parent_ctx = *self
            .hygiene_stack
            .last()
            .unwrap_or(&self.hygiene.root_syntax_context());
        let parent_expn = self
            .hygiene
            .syntax_context_data(parent_ctx)
            .and_then(|data| data.outer_expn)
            .unwrap_or_else(|| self.hygiene.root_expn());

        let expn_id = self.hygiene.fresh_expn(ExpnData {
            parent: parent_expn,
            call_site: span,
            def_site: span,
            kind: ExpnKind::Macro,
            desc: format!("expand {}", name),
        });
        let generated_ctx = self
            .hygiene
            .apply_mark(parent_ctx, expn_id, Transparency::Opaque);

        self.hygiene_stack.push(generated_ctx);

        let defining_crate = self
            .resolver
            .macro_def_data(def_id)
            .map(|d| d.defining_crate)
            .unwrap_or_else(|| CrateId::new(1));

        let result = match transcribe(
            &rule.transcriber,
            bindings,
            self.interner,
            generated_ctx,
            defining_crate,
        ) {
            Ok(stream) => Some(stream),
            Err(reason) => {
                self.errors.push(
                    ExpandError::macro_transcribe_error(name.to_string(), reason, span)
                        .with_backtrace(self.backtrace()),
                );
                None
            }
        };

        self.hygiene_stack.pop();
        result
    }

    /// Convert diagnostics emitted by an in-process proc macro into expansion
    /// errors and push them onto the expander's error list.
    fn push_proc_macro_diagnostics(
        &mut self,
        diagnostics: Vec<Diagnostic>,
        macro_name: &str,
        span: yelang_lexer::Span,
    ) {
        for diag in diagnostics {
            self.errors.push(
                ExpandError::malformed_macro_args(
                    format!(
                        "proc macro `{}` emitted a diagnostic [{:?}]: {}",
                        macro_name, diag.level, diag.message
                    ),
                    span,
                )
                .with_backtrace(self.backtrace()),
            );
        }
    }

    /// Invoke an in-process function-like procedural macro.
    fn expand_in_process_fn_like(
        &mut self,
        mac: &dyn InProcessProcMacro,
        args: &yelang_macro_core::token_tree::TokenStream,
        span: yelang_lexer::Span,
    ) -> Result<yelang_macro_core::token_tree::TokenStream, ExpandError> {
        let proc_input = yelang_proc_macro::TokenStream::from_core_stream(args, self.interner);
        let (proc_output, diagnostics) = mac.expand_fn_like(proc_input);
        self.push_proc_macro_diagnostics(diagnostics, mac.name(), span);
        Ok(self.proc_macro_output_to_core_stream(proc_output, span))
    }

    /// Invoke an in-process attribute procedural macro.
    fn expand_in_process_attr(
        &mut self,
        mac: &dyn InProcessProcMacro,
        args: yelang_proc_macro::TokenStream,
        item: yelang_proc_macro::TokenStream,
        span: yelang_lexer::Span,
    ) -> Result<yelang_macro_core::token_tree::TokenStream, ExpandError> {
        let (proc_output, diagnostics) = mac.expand_attr(args, item);
        self.push_proc_macro_diagnostics(diagnostics, mac.name(), span);
        Ok(self.proc_macro_output_to_core_stream(proc_output, span))
    }

    /// Invoke an in-process derive procedural macro.
    fn expand_in_process_derive(
        &mut self,
        mac: &dyn InProcessProcMacro,
        item: yelang_proc_macro::TokenStream,
        span: yelang_lexer::Span,
    ) -> Result<yelang_macro_core::token_tree::TokenStream, ExpandError> {
        let (proc_output, diagnostics) = mac.expand_derive(item);
        self.push_proc_macro_diagnostics(diagnostics, mac.name(), span);
        Ok(self.proc_macro_output_to_core_stream(proc_output, span))
    }

    /// Invoke a server-based function-like procedural macro.
    fn expand_server_fn_like(
        &mut self,
        mac: &crate::proc_macro::ResolvedProcMacro,
        args: &yelang_macro_core::token_tree::TokenStream,
        span: yelang_lexer::Span,
    ) -> Result<yelang_macro_core::token_tree::TokenStream, ExpandError> {
        let (def_site, mixed_site) = self.proc_macro_sites(span);
        let runtime = self
            .proc_macro_runtime
            .as_ref()
            .expect("server macro expansion requires a runtime");
        let wire_input = core_to_wire(args, self.interner);
        let hygiene = crate::hygiene::payload_from_stream_with_spans(
            args,
            &[span, def_site, mixed_site],
            &self.hygiene,
        );
        let (wire_output, diagnostics, returned_hygiene) = runtime.expand_proc_macro(
            mac,
            Some(wire_input),
            None,
            span,
            def_site,
            hygiene,
            Limits::default(),
        )?;
        crate::hygiene::merge_payload(&self.hygiene, &returned_hygiene);
        self.push_server_diagnostics(&diagnostics, &mac.name, span);
        wire_to_core(wire_output, self.interner, span)
    }

    /// Invoke a server-based attribute procedural macro.
    fn expand_server_attr(
        &mut self,
        mac: &crate::proc_macro::ResolvedProcMacro,
        args: yelang_macro_core::token_tree::TokenStream,
        item: yelang_macro_core::token_tree::TokenStream,
        span: yelang_lexer::Span,
    ) -> Result<yelang_macro_core::token_tree::TokenStream, ExpandError> {
        let (def_site, mixed_site) = self.proc_macro_sites(span);
        let runtime = self
            .proc_macro_runtime
            .as_ref()
            .expect("server macro expansion requires a runtime");
        let wire_args = core_to_wire(&args, self.interner);
        let wire_item = core_to_wire(&item, self.interner);
        let mut combined = args.clone();
        combined.extend(item.clone());
        let hygiene = crate::hygiene::payload_from_stream_with_spans(
            &combined,
            &[span, def_site, mixed_site],
            &self.hygiene,
        );
        let (wire_output, diagnostics, returned_hygiene) = runtime.expand_proc_macro(
            mac,
            Some(wire_args),
            Some(wire_item),
            span,
            def_site,
            hygiene,
            Limits::default(),
        )?;
        crate::hygiene::merge_payload(&self.hygiene, &returned_hygiene);
        self.push_server_diagnostics(&diagnostics, &mac.name, span);
        wire_to_core(wire_output, self.interner, span)
    }

    /// Invoke a server-based derive procedural macro.
    fn expand_server_derive(
        &mut self,
        mac: &crate::proc_macro::ResolvedProcMacro,
        item: yelang_macro_core::token_tree::TokenStream,
        span: yelang_lexer::Span,
    ) -> Result<yelang_macro_core::token_tree::TokenStream, ExpandError> {
        let (def_site, mixed_site) = self.proc_macro_sites(span);
        let runtime = self
            .proc_macro_runtime
            .as_ref()
            .expect("server macro expansion requires a runtime");
        let wire_item = core_to_wire(&item, self.interner);
        let hygiene = crate::hygiene::payload_from_stream_with_spans(
            &item,
            &[span, def_site, mixed_site],
            &self.hygiene,
        );
        let (wire_output, diagnostics, returned_hygiene) = runtime.expand_proc_macro(
            mac,
            None,
            Some(wire_item),
            span,
            def_site,
            hygiene,
            Limits::default(),
        )?;
        crate::hygiene::merge_payload(&self.hygiene, &returned_hygiene);
        self.push_server_diagnostics(&diagnostics, &mac.name, span);
        wire_to_core(wire_output, self.interner, span)
    }

    /// Compute the definition-site and mixed-site spans for a procedural macro
    /// invocation.
    ///
    /// `def_site` currently falls back to the call site because the macro
    /// definition span is not yet threaded through the resolver. `mixed_site` is
    /// a fresh syntax context with mixed transparency parented to the call-site
    /// context.
    fn proc_macro_sites(
        &mut self,
        span: yelang_lexer::Span,
    ) -> (yelang_lexer::Span, yelang_lexer::Span) {
        let call_site_ctx = yelang_macro_core::SyntaxContextId::new(span.syntax_context());
        let expn_id = self.hygiene.fresh_expn(ExpnData {
            parent: self.hygiene.root_expn(),
            call_site: span,
            def_site: span,
            kind: ExpnKind::ProcMacro,
            desc: "mixed-site".to_string(),
        });
        let mixed_ctx = self
            .hygiene
            .apply_mark(call_site_ctx, expn_id, Transparency::Mixed);
        let mixed_site = span.with_syntax_context(mixed_ctx.raw());
        (span, mixed_site)
    }

    /// Convert server diagnostics into expansion errors and push them.
    fn push_server_diagnostics(
        &mut self,
        diagnostics: &[yelang_proc_macro_bridge::protocol::token::WireDiagnostic],
        macro_name: &str,
        span: yelang_lexer::Span,
    ) {
        let errors = wire_diagnostics_to_errors(diagnostics, macro_name, span, self.backtrace());
        self.errors.extend(errors);
    }

    /// Convert a procedural macro output stream back into a compiler-internal
    /// token stream, re-interning symbols in the expander's interner while
    /// preserving spans and hygiene contexts.
    fn proc_macro_output_to_core_stream(
        &self,
        output: yelang_proc_macro::TokenStream,
        _span: yelang_lexer::Span,
    ) -> yelang_macro_core::token_tree::TokenStream {
        output.into_core_stream_with_interner(self.interner)
    }

    /// Try to expand a user-defined macro invocation and return the raw expanded
    /// token stream. The caller parses the stream according to the expected
    /// syntactic category (expression, type, pattern, item, or statement).
    fn expand_macro_invocation(
        &mut self,
        inv: &MacroInvocation,
    ) -> Result<yelang_macro_core::token_tree::TokenStream, ExpandError> {
        let name = inv
            .name(self.interner)
            .unwrap_or_else(|| "(qualified)".to_string());
        let span = inv.span;

        // Eager built-in macros expand before ordinary macro resolution.
        if let Some(builtin) = EagerBuiltin::from_name(&name) {
            let ctx = self.eager_context();
            let expanded_args = expand_eager_macros_in_stream(&inv.args, &ctx)?;
            // Macro invocation arguments are stored wrapped in their delimiter
            // group; eager builtins expect the contents of that group.
            let expanded_args = unwrap_macro_args(&expanded_args);
            return expand_eager_builtin_to_stream(builtin, &expanded_args, &ctx, span);
        }

        // In-process procedural macros take priority over declarative macros.
        if let Some(executor) = self.in_process_executor.clone()
            && let Some(mac) = executor.find(&name)
        {
            let Some(_) = self.before_expand(&name, span, MacroFrameId::ProcMacro(name.clone()))
            else {
                return Err(
                    ExpandError::expansion_loop(name, span).with_backtrace(self.backtrace())
                );
            };
            let macro_args = unwrap_macro_args(&inv.args);
            let result = self.expand_in_process_fn_like(mac, &macro_args, span);
            self.after_expand();
            return result;
        }

        // Out-of-process procedural macros take priority over declarative macros.
        if let Some(runtime) = self.proc_macro_runtime.as_ref()
            && let Some(result) = runtime.resolve(
                &name,
                yelang_proc_macro_bridge::protocol::ProcMacroKind::FunctionLike,
            )
        {
            let mac = match result {
                Ok(mac) => mac,
                Err(e) => {
                    return Err(ExpandError::malformed_macro_args(
                        format!("failed to load proc macro `{}`: {}", name, e),
                        span,
                    )
                    .with_backtrace(self.backtrace()));
                }
            };
            let Some(_) = self.before_expand(&name, span, MacroFrameId::ProcMacro(name.clone()))
            else {
                return Err(
                    ExpandError::expansion_loop(name, span).with_backtrace(self.backtrace())
                );
            };
            let macro_args = unwrap_macro_args(&inv.args);
            let result = self.expand_server_fn_like(&mac, &macro_args, span);
            self.after_expand();
            return result;
        }

        let Some(mac) = self.resolver.resolve(&name) else {
            return Err(ExpandError::unknown_macro(name, span).with_backtrace(self.backtrace()));
        };
        let mac = mac.clone();

        let Some(MacroFrameId::Declarative(def_id)) =
            self.before_expand(&name, span, MacroFrameId::Declarative(mac.def_id))
        else {
            return Err(ExpandError::expansion_loop(name, span).with_backtrace(self.backtrace()));
        };

        // The invocation's `args` field preserves the delimiter group from the
        // source (`id!(...)`).  Macro rules are written to match the tokens
        // *inside* that delimiter, so unwrap one level when it is a single
        // delimited group.
        let macro_args = unwrap_macro_args(&inv.args);

        let mut matches = Vec::new();
        for rule in &mac.rules {
            if rule.kind != MacroKind::FunctionLike {
                continue;
            }
            if let Ok(bindings) = try_match_rule(rule, &macro_args, self.interner) {
                matches.push((rule, bindings));
            }
        }

        let (rule, bindings) = match matches.len() {
            0 => {
                let err = ExpandError::macro_match_error(
                    name.clone(),
                    "no rule matched the invocation".to_string(),
                    span,
                )
                .with_backtrace(self.backtrace());
                self.errors.push(err.clone());
                self.after_expand();
                return Err(err);
            }
            1 => (&matches[0].0, &matches[0].1),
            _ => {
                let err = ExpandError::ambiguous_macro(name.clone(), span)
                    .with_backtrace(self.backtrace());
                self.errors.push(err.clone());
                self.after_expand();
                return Err(err);
            }
        };

        let expanded_stream = match self.transcribe_rule(rule, bindings, &name, span, def_id) {
            Some(stream) => stream,
            None => {
                self.after_expand();
                return Err(ExpandError::macro_transcribe_error(
                    name.clone(),
                    "transcription failed".to_string(),
                    span,
                )
                .with_backtrace(self.backtrace()));
            }
        };

        // The stack frame stays active so that recursive expansion of the
        // output can detect expansion cycles (a → b → a).
        Ok(expanded_stream)
    }
}

fn item_differs(expanded: &[Item], original: &Item) -> bool {
    expanded.len() != 1 || expanded[0] != *original
}

/// Execute an eager built-in macro and return its output as a token stream.
fn expand_eager_builtin_to_stream(
    builtin: EagerBuiltin,
    args: &yelang_macro_core::token_tree::TokenStream,
    ctx: &EagerContext<'_>,
    span: yelang_lexer::Span,
) -> Result<yelang_macro_core::token_tree::TokenStream, ExpandError> {
    crate::eager::expand_eager_builtin(builtin, args, ctx, span)
}

fn parse_expr_from_token_stream(
    stream: &yelang_macro_core::token_tree::TokenStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Expr, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let rendered = stream.render(interner);
    let local_interner = interner.clone();
    let mut lex = yelang_ast::TokenKind::tokenize(&rendered, &local_interner)
        .map_err(|e| format!("tokenize: {}", e))?;
    let expr = lex.parse::<Expr>().map_err(|e| e.to_string())?;
    if !lex.is_eof() {
        return Err("trailing tokens after expression".to_string());
    }
    // Replace the local interner symbols with the original interner's symbols.
    // The parsed expression carries symbol ids from `local_interner`; since the
    // original interner was cloned, the same text gets the same ids, so the
    // expression is valid in the original interner.
    let _ = local_interner;
    Ok(expr)
}

/// Parse a token stream produced by a macro transcriber into a list of items.
fn parse_items_from_token_stream(
    stream: &yelang_macro_core::token_tree::TokenStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Vec<Item>, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let rendered = stream.render(interner);
    let local_interner = interner.clone();
    let mut lex =
        TokenKind::tokenize(&rendered, &local_interner).map_err(|e| format!("tokenize: {}", e))?;
    let program = lex.parse::<Program>().map_err(|e| e.to_string())?;
    if !lex.is_eof() {
        return Err("trailing tokens after items".to_string());
    }
    let _ = local_interner;
    Ok(program.items)
}

/// Parse a token stream produced by a macro transcriber into a sequence of statements.
fn parse_stmts_from_token_stream(
    stream: &yelang_macro_core::token_tree::TokenStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Vec<Stmt>, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let rendered = stream.render(interner);
    let local_interner = interner.clone();
    let mut lex = yelang_ast::TokenKind::tokenize(&rendered, &local_interner)
        .map_err(|e| format!("tokenize: {}", e))?;
    let mut stmts = vec![];
    while !lex.is_eof() {
        let stmt = lex.parse::<Stmt>().map_err(|e| e.to_string())?;
        stmts.push(stmt);
    }
    let _ = local_interner;
    Ok(stmts)
}

/// Parse a token stream produced by a macro transcriber into a single type.
fn parse_type_from_token_stream(
    stream: &yelang_macro_core::token_tree::TokenStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Type, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let rendered = stream.render(interner);
    let local_interner = interner.clone();
    let mut lex = yelang_ast::TokenKind::tokenize(&rendered, &local_interner)
        .map_err(|e| format!("tokenize: {}", e))?;
    let ty = lex.parse::<Type>().map_err(|e| e.to_string())?;
    if !lex.is_eof() {
        return Err("trailing tokens after type".to_string());
    }
    let _ = local_interner;
    Ok(ty)
}

/// Parse a token stream produced by a macro transcriber into a single pattern.
fn parse_pattern_from_token_stream(
    stream: &yelang_macro_core::token_tree::TokenStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Pattern, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let rendered = stream.render(interner);
    let local_interner = interner.clone();
    let mut lex = yelang_ast::TokenKind::tokenize(&rendered, &local_interner)
        .map_err(|e| format!("tokenize: {}", e))?;
    let pat = lex.parse::<Pattern>().map_err(|e| e.to_string())?;
    if !lex.is_eof() {
        return Err("trailing tokens after pattern".to_string());
    }
    let _ = local_interner;
    Ok(pat)
}

/// Peel an `unsafe(...)` attribute wrapper, returning the inner attribute and
/// a flag indicating that the wrapper was present.
///
/// `@unsafe(foo(args))` and `#[unsafe(foo(args))]` are accepted. The inner
/// attribute is reconstructed so that normal attribute macro dispatch can
/// process it.
fn peel_unsafe_attribute(attr: &Attribute, interner: &Interner) -> Option<(Attribute, bool)> {
    let first = attr.path.first()?;
    if interner.resolve(&first.symbol) != "unsafe" {
        return None;
    }
    let exprs = match &attr.args {
        AttributeArgs::Positional(exprs) => exprs,
        _ => return None,
    };
    let expr = exprs.first()?;
    let (name, args) = match &expr.kind {
        ExprKind::Path(path) if path.segments.len() == 1 => {
            (path.segments[0].ident, AttributeArgs::Empty)
        }
        ExprKind::Call(call) => {
            let callee = &call.callee;
            match &callee.kind {
                ExprKind::Path(path) if path.segments.len() == 1 => {
                    let name = path.segments[0].ident;
                    let args: Vec<Expr> = call
                        .args
                        .iter()
                        .filter_map(|arg| match arg {
                            yelang_ast::CallArgument::Positional(e) => Some(e.clone()),
                            _ => None,
                        })
                        .collect();
                    (name, AttributeArgs::Positional(args))
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    Some((
        Attribute {
            path: vec![name],
            is_absolute: false,
            args,
            span: attr.span,
        },
        true,
    ))
}

/// Convert attribute arguments back into a macro `TokenStream` so that
/// attribute macro matchers can operate on them.
fn attribute_args_to_token_stream(
    args: &AttributeArgs,
    interner: &Interner,
) -> Option<yelang_macro_core::token_tree::TokenStream> {
    let mut rendered = String::new();
    match args {
        AttributeArgs::Empty => {}
        AttributeArgs::Positional(exprs) => {
            for (i, expr) in exprs.iter().enumerate() {
                if i > 0 {
                    rendered.push_str(", ");
                }
                expr.codegen(&mut rendered, interner).ok()?;
            }
        }
        AttributeArgs::Named(named) => {
            for (i, NamedArg { name, value }) in named.iter().enumerate() {
                if i > 0 {
                    rendered.push_str(", ");
                }
                rendered.push_str(interner.resolve(&name.symbol));
                rendered.push_str(" = ");
                value.codegen(&mut rendered, interner).ok()?;
            }
        }
    }
    tokenize_and_convert(&rendered, interner)
}

/// Convert an item back into a macro `TokenStream` so that attribute/derive
/// macro matchers can operate on it.
fn item_to_token_stream(
    item: &Item,
    interner: &Interner,
) -> Option<yelang_macro_core::token_tree::TokenStream> {
    let mut rendered = String::new();
    item.codegen(&mut rendered, interner).ok()?;
    tokenize_and_convert(&rendered, interner)
}

/// Tokenize a source snippet and convert it to macro-core token trees.
fn tokenize_and_convert(
    src: &str,
    interner: &Interner,
) -> Option<yelang_macro_core::token_tree::TokenStream> {
    if src.is_empty() {
        return Some(yelang_macro_core::token_tree::TokenStream::new());
    }
    let local_interner = interner.clone();
    let mut lex = TokenKind::tokenize(src, &local_interner).ok()?;
    let tokens: Vec<_> = std::iter::from_fn(|| lex.advance().cloned()).collect();
    Some(yelang_ast::expr::convert::from_lexer_tokens(
        &tokens, interner,
    ))
}

/// Extract the matcher ops for an attribute/derive rule's item matcher.
///
/// If the matcher is a single group (the conventional `($item:item)`), strip
/// that outer group and return its delimiter and inner ops. Otherwise match
/// the ops directly against the item token stream.
fn item_matcher_ops(
    matcher: &[crate::matcher::types::MatcherOp],
) -> (
    Option<yelang_macro_core::token_tree::Delimiter>,
    &[crate::matcher::types::MatcherOp],
) {
    if let [crate::matcher::types::MatcherOp::Group { delimiter, ops }] = matcher {
        (Some(*delimiter), ops.as_slice())
    } else {
        (None, matcher)
    }
}

/// Wrap an item token stream in a delimited group when the matcher expects one.
fn wrap_item_stream(
    item_stream: yelang_macro_core::token_tree::TokenStream,
    delimiter: Option<yelang_macro_core::token_tree::Delimiter>,
) -> yelang_macro_core::token_tree::TokenStream {
    match delimiter {
        Some(delimiter) => yelang_macro_core::token_tree::TokenStream::from_vec(vec![
            yelang_macro_core::token_tree::TokenTree::Group(
                yelang_macro_core::token_tree::Group::new(
                    delimiter,
                    item_stream,
                    yelang_macro_core::token_tree::Span::default(),
                ),
            ),
        ]),
        None => item_stream,
    }
}

/// If `args` is a single delimited group, return its inner stream; otherwise
/// return it unchanged.  This matches macro_rules semantics where the matcher
/// sees the contents of `id!(...)`, not the delimiter itself.
fn unwrap_macro_args(
    args: &yelang_macro_core::token_tree::TokenStream,
) -> yelang_macro_core::token_tree::TokenStream {
    if args.trees().len() == 1
        && let Some(yelang_macro_core::token_tree::TokenTree::Group(g)) = args.trees().first()
    {
        return g.stream.clone();
    }
    args.clone()
}

/// Expand all macros and decorators in a program, returning the fully-expanded AST.
///
/// This is the primary entry point for the macro expansion phase.
/// It runs the expander iteratively until no more macro invocations remain.
pub fn expand_program(program: &Program, interner: &Interner) -> ExpandResult {
    let mut expander = MacroExpander::new(interner);
    expander.expand(program)
}

/// Expand all macros and decorators in a program with out-of-process procedural
/// macros available.
pub fn expand_program_with_proc_macros(
    program: &Program,
    interner: &Interner,
    runtime: ProcMacroRuntime,
) -> ExpandResult {
    let mut expander = MacroExpander::new(interner).with_proc_macro_runtime(runtime);
    expander.expand(program)
}

/// Expand macros and decorators on a single item.
///
/// Returns a vec because decorators such as `@derive` may generate
/// additional items (e.g. `impl` blocks) alongside the original item.
pub fn expand_item(item: &Item, interner: &Interner) -> Result<Vec<Item>, ExpandError> {
    let mut expander = MacroExpander::new(interner);
    expander.expand_item(item.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ast::TokenKind;
    use yelang_interner::Interner;

    fn parse_program(src: &str) -> (Program, Interner) {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
        let program = stream.parse::<Program>().unwrap();
        (program, interner)
    }

    #[test]
    fn expand_assert_in_function() {
        let src = r#"
            fn main() {
                assert!(true);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // assert!(true) should expand to `if !true { panic!(...) }`
        let fn_item = &result.program.items[0];
        let ItemKind::Fn(func) = &fn_item.kind else {
            panic!("expected fn")
        };
        let body = &func.body;
        assert_eq!(body.statements.len(), 1);
        let StmtKind::TermExpr(expr) = &body.statements[0].kind else {
            panic!("expected term expr stmt")
        };
        assert!(
            matches!(expr.kind, ExprKind::If(_)),
            "expected If, got {:?}",
            expr.kind
        );
    }

    #[test]
    fn expand_todo_in_function() {
        let src = r#"
            fn main() {
                todo!();
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // todo!() expands to panic!("not yet implemented")
        let fn_item = &result.program.items[0];
        let ItemKind::Fn(func) = &fn_item.kind else {
            panic!("expected fn")
        };
        let body = &func.body;
        let StmtKind::TermExpr(expr) = &body.statements[0].kind else {
            panic!("expected term expr stmt")
        };
        assert!(
            matches!(expr.kind, ExprKind::Call(_)),
            "expected Call, got {:?}",
            expr.kind
        );
    }

    #[test]
    fn expand_unknown_macro_emits_error() {
        let src = r#"
            fn main() {
                unknown_macro!(1);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(!result.errors.is_empty(), "expected at least one error");
        assert!(
            result
                .errors
                .iter()
                .any(|e| matches!(e, ExpandError::UnknownMacro { .. }))
        );
    }

    #[test]
    fn decorator_test_on_function() {
        let src = r#"
            @test
            fn my_test() {}
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // @test should be removed from attributes after processing.
        assert!(result.program.items[0].attributes.is_empty());
    }

    #[test]
    fn decorator_test_on_struct_errors() {
        let src = r#"
            @test
            struct Foo {}
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(
            !result.errors.is_empty(),
            "expected error for @test on struct"
        );
    }

    #[test]
    fn nested_macro_expansion() {
        // todo!() expands to panic!("not yet implemented"), which is then
        // expanded to a call expression in the next iteration.
        let src = r#"
            fn main() {
                todo!();
            }
        "#;
        let (program, interner) = parse_program(src);
        let mut expander = MacroExpander::new(&interner);
        let result = expander.expand(&program);
        // After two iterations, todo! → panic!(...) → call expr
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }

    #[test]
    fn expand_assert_eq_in_function() {
        let src = r#"
            fn main() {
                assert_eq!(a, b);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let fn_item = &result.program.items[0];
        let ItemKind::Fn(func) = &fn_item.kind else {
            panic!("expected fn")
        };
        let body = &func.body;
        assert_eq!(body.statements.len(), 1);
        let StmtKind::TermExpr(expr) = &body.statements[0].kind else {
            panic!("expected term expr stmt")
        };
        assert!(
            matches!(expr.kind, ExprKind::Block(_)),
            "expected Block, got {:?}",
            expr.kind
        );
    }

    #[test]
    fn expand_assert_ne_in_function() {
        let src = r#"
            fn main() {
                assert_ne!(a, b);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    }

    #[test]
    fn expand_format_in_function() {
        let src = r#"
            fn main() {
                format!("hello {}", name);
            }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let fn_item = &result.program.items[0];
        let ItemKind::Fn(func) = &fn_item.kind else {
            panic!("expected fn")
        };
        let body = &func.body;
        let StmtKind::TermExpr(expr) = &body.statements[0].kind else {
            panic!("expected term expr stmt")
        };
        assert!(
            matches!(expr.kind, ExprKind::Call(_)),
            "expected Call, got {:?}",
            expr.kind
        );
    }

    #[test]
    fn derive_generates_impl_items() {
        let src = r#"
            @derive(Clone, Copy)
            struct Point {}
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // Should have: struct Point, impl Clone for Point, impl Copy for Point
        assert_eq!(
            result.program.items.len(),
            3,
            "expected 3 items: struct + 2 impls"
        );
        let impls: Vec<_> = result
            .program
            .items
            .iter()
            .filter(|i| matches!(i.kind, ItemKind::Impl(_)))
            .collect();
        assert_eq!(impls.len(), 2, "expected 2 impl items");
    }

    #[test]
    fn derive_partial_eq_for_named_struct() {
        let src = r#"
            @derive(PartialEq)
            struct Point { x: i32, y: i32 }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.program.items.len(), 2);
        let impl_item = result
            .program
            .items
            .iter()
            .find(|i| matches!(i.kind, ItemKind::Impl(_)))
            .expect("impl");
        let ItemKind::Impl(impl_block) = &impl_item.kind else {
            unreachable!()
        };
        assert_eq!(
            impl_block.items.len(),
            1,
            "PartialEq impl should have eq method"
        );
    }

    #[test]
    fn derive_debug_for_unit_struct() {
        let src = r#"
            @derive(Debug)
            struct Unit;
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.program.items.len(), 2);
    }

    #[test]
    fn derive_unsupported_trait_errors() {
        let src = r#"
            @derive(Ord)
            struct Point {}
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(
            !result.errors.is_empty(),
            "expected error for unsupported derive trait"
        );
    }

    #[test]
    fn derive_clone_named_struct_produces_struct_literal() {
        // Verify that @derive(Clone) on a named struct generates a method
        // whose body contains `Self { field: self.field.clone(), ... }`.
        let src = r#"
            @derive(Clone)
            struct Point { x: i32, y: i32 }
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(
            result.program.items.len(),
            2,
            "expected struct + impl Clone"
        );

        let impl_item = result
            .program
            .items
            .iter()
            .find(|i| matches!(i.kind, ItemKind::Impl(_)))
            .expect("impl Clone expected");
        let ItemKind::Impl(impl_block) = &impl_item.kind else {
            unreachable!()
        };
        assert_eq!(
            impl_block.items.len(),
            1,
            "Clone impl should have clone method"
        );

        let method = &impl_block.items[0];
        let ImplItemKind::Method(fn_def) = &method.item else {
            panic!("expected method in Clone impl");
        };

        // The body should be a block with a single terminating expression.
        assert_eq!(fn_def.body.statements.len(), 1);
        let StmtKind::TermExpr(expr) = &fn_def.body.statements[0].kind else {
            panic!("expected term expr in clone body");
        };

        // The expression must be a struct literal, not just a path.
        let ExprKind::Struct(struct_expr) = &expr.kind else {
            panic!(
                "expected ExprKind::Struct in clone body, got {:?}",
                expr.kind
            );
        };

        // Path should be `Self`.
        assert_eq!(struct_expr.path.segments.len(), 1);
        assert_eq!(
            interner.resolve(&struct_expr.path.segments[0].ident.symbol),
            "Self"
        );

        // Should have exactly two field assignments.
        assert_eq!(struct_expr.fields.len(), 2, "expected 2 field assignments");
        assert_eq!(interner.resolve(&struct_expr.fields[0].name.symbol), "x");
        assert_eq!(interner.resolve(&struct_expr.fields[1].name.symbol), "y");

        // Each field value should be a method call (self.field.clone()).
        assert!(
            matches!(struct_expr.fields[0].value.kind, ExprKind::MethodCall(_)),
            "expected method call for field clone"
        );
        assert!(
            matches!(struct_expr.fields[1].value.kind, ExprKind::MethodCall(_)),
            "expected method call for field clone"
        );
    }

    #[test]
    fn derive_clone_unit_struct_uses_self_path() {
        let src = r#"
            @derive(Clone)
            struct Unit;
        "#;
        let (program, interner) = parse_program(src);
        let result = crate::expand_program(&program, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let impl_item = result
            .program
            .items
            .iter()
            .find(|i| matches!(i.kind, ItemKind::Impl(_)))
            .expect("impl Clone expected");
        let ItemKind::Impl(impl_block) = &impl_item.kind else {
            unreachable!()
        };
        let method = &impl_block.items[0];
        let ImplItemKind::Method(fn_def) = &method.item else {
            panic!("expected method");
        };

        let StmtKind::TermExpr(expr) = &fn_def.body.statements[0].kind else {
            panic!("expected term expr");
        };
        assert!(
            matches!(expr.kind, ExprKind::Path(_)),
            "unit struct clone should return Self path, got {:?}",
            expr.kind
        );
    }
}
