use yelang_arena::DefId;
use yelang_ast::item::{Enum, Struct, Trait, TypeAlias};
use yelang_ast::{
    BlockExpr, BreakExpr, CallExpr, ContinueExpr, Expr, ExprKind, FnDef, Item, ItemKind,
    MemberAccess, MethodCallExpr, ModKind, Param, Path, Pattern, PatternKind, Program, Stmt,
    StmtKind, Type, TypeKind,
};
use yelang_ast::query::{GroupByClause, LinkPath, LinkSegment};
use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::{
    error::ResolutionError,
    namespaces::Namespace,
    path::{resolve_type_path, resolve_value_path},
    rib::{Resolution, RibKind},
    scope::Resolver,
};

/// A scope that can be broken out of (loop or labeled block).
#[derive(Debug, Clone)]
struct BreakableScope {
    label: Option<Symbol>,
    is_loop: bool,
    #[allow(dead_code)]
    span: Span,
}

pub struct LateResolver<'a, 'b> {
    resolver: &'b mut Resolver<'a>,
    breakable_stack: Vec<BreakableScope>,
}

impl<'a, 'b> LateResolver<'a, 'b> {
    pub fn new(resolver: &'b mut Resolver<'a>) -> Self {
        Self {
            resolver,
            breakable_stack: Vec::new(),
        }
    }

    pub fn resolve(mut self, program: &Program) {
        self.resolve_items(&program.items);
    }

    fn resolve_items(&mut self, items: &[Item]) {
        for item in items {
            self.resolve_item(item);
        }
    }

    fn resolve_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Fn(func) => self.resolve_fn(func),
            ItemKind::Struct(s) => self.resolve_struct(s),
            ItemKind::Enum(e) => self.resolve_enum(e),
            ItemKind::TypeAlias(ta) => self.resolve_type_alias(ta),
            ItemKind::Trait(t) => self.resolve_trait(t),
            ItemKind::Module(m) => self.resolve_module(m),
            ItemKind::Const(c) => self.resolve_const(c),
            ItemKind::Static(s) => self.resolve_static(s),
            ItemKind::Impl(i) => self.resolve_impl(i),
            ItemKind::Use(_) => {}
        }
    }

    /// Add generic parameters (both type and const) to the appropriate rib stacks.
    /// Uses the `DefId`s pre-allocated during def collection so that uses of a
    /// generic parameter name resolve to a real definition.
    fn resolve_generic_params(&mut self, params: &[yelang_ast::GenericParam]) {
        // First pass: introduce all parameter names into scope so bounds can
        // reference any parameter in the list (including themselves, which is
        // allowed for bounds like `T: PartialEq<T>`).
        for param in params {
            let (name, span, ns) = match param {
                yelang_ast::GenericParam::Type(tp) => {
                    (tp.name.symbol, tp.name.span, Namespace::Type)
                }
                yelang_ast::GenericParam::Const(cp) => {
                    // Resolve the type annotation of the const param first so its
                    // own name is not yet in scope.
                    self.resolve_type(&cp.ty);
                    (cp.name.symbol, cp.name.span, Namespace::Value)
                }
            };
            if let Some(def_id) = self.resolver.generic_param_defs.get(&span).copied() {
                if let Some(rib) = match ns {
                    Namespace::Value => self.resolver.value_ribs.last_mut(),
                    Namespace::Type => self.resolver.type_ribs.last_mut(),
                } {
                    rib.insert(ns, name, Resolution::Def { def_id }, span);
                }
            } else {
                // Fallback: allocate a local binding if the generic param was not
                // collected. This keeps resolution working even for uncollected
                // params (e.g. trait-method generics).
                match ns {
                    Namespace::Value => self.add_value_binding(name, span),
                    Namespace::Type => self.add_type_binding(name, span),
                }
            }
        }

        // Second pass: resolve inline bounds on type parameters.
        for param in params {
            if let yelang_ast::GenericParam::Type(tp) = param {
                for bound in &tp.bounds {
                    self.resolve_trait_bound(bound);
                }
            }
        }
    }

    fn resolve_fn(&mut self, func: &FnDef) {
        self.push_rib(RibKind::Fn);
        self.resolve_generic_params(&func.generics.params);
        if let Some(where_clause) = &func.generics.where_clause {
            self.resolve_where_clause(where_clause);
        }
        // Add function parameters to value scope.
        for param in &func.sig.params {
            self.resolve_param(param);
        }
        // Resolve return type.
        if let yelang_ast::FnRefType::Type(ty) = &func.sig.return_type {
            self.resolve_type(ty);
        }
        // Resolve body.
        self.resolve_block_expr(&func.body);
        self.pop_rib();
    }

    fn resolve_struct(&mut self, s: &Struct) {
        self.push_rib(RibKind::Opaque);
        self.resolve_generic_params(&s.generics.params);
        match &s.fields {
            yelang_ast::StructFields::Named(fields) => {
                for f in fields {
                    self.resolve_type(&f.ty);
                }
            }
            yelang_ast::StructFields::Tuple(tys) => {
                for ty in tys {
                    self.resolve_type(ty);
                }
            }
            yelang_ast::StructFields::Unit => {}
        }
        self.pop_rib();
    }

    fn resolve_enum(&mut self, e: &Enum) {
        self.push_rib(RibKind::Opaque);
        self.resolve_generic_params(&e.generics.params);
        for variant in &e.variants {
            match &variant.kind {
                yelang_ast::VariantKind::Struct(fields) => {
                    for f in fields {
                        self.resolve_type(&f.ty);
                    }
                }
                yelang_ast::VariantKind::Tuple(tys) => {
                    for ty in tys {
                        self.resolve_type(ty);
                    }
                }
                yelang_ast::VariantKind::Unit => {}
            }
        }
        self.pop_rib();
    }

    fn resolve_type_alias(&mut self, ta: &TypeAlias) {
        self.push_rib(RibKind::Opaque);
        self.resolve_generic_params(&ta.generics.params);
        self.resolve_type(&ta.target);
        self.pop_rib();
    }

    fn resolve_trait(&mut self, t: &Trait) {
        self.push_rib(RibKind::Opaque);
        // `Self` is the implicit type parameter of every trait.
        let self_symbol = self.resolver.interner.get_or_intern("Self");
        self.add_type_binding(self_symbol, t.name.span);
        self.resolve_generic_params(&t.generics.params);
        for item in &t.items {
            if let yelang_ast::TraitItemKind::Method(m) = &item.item {
                self.push_rib(RibKind::Fn);
                self.resolve_generic_params(&m.generics.params);
                if let Some(where_clause) = &m.generics.where_clause {
                    self.resolve_where_clause(where_clause);
                }
                for param in &m.sig.params {
                    self.resolve_param(param);
                }
                if let yelang_ast::FnRefType::Type(ty) = &m.sig.return_type {
                    self.resolve_type(ty);
                }
                if let Some(body) = &m.body {
                    self.resolve_block_expr(body);
                }
                self.pop_rib();
            }
        }
        self.pop_rib();
    }

    fn resolve_module(&mut self, m: &yelang_ast::ModDef) {
        if let ModKind::Inline { items } = &m.kind {
            let old_module = self.resolver.current_module;
            if let Some(id) = self.find_module_by_name(m.name.symbol) {
                self.resolver.current_module = id;
            }
            self.resolve_items(items);
            self.resolver.current_module = old_module;
        }
    }

    fn resolve_const(&mut self, c: &yelang_ast::item::Const) {
        self.resolve_type(&c.ty);
        self.resolve_expr(&c.value);
    }

    fn resolve_static(&mut self, s: &yelang_ast::item::Static) {
        self.resolve_type(&s.ty);
        self.resolve_expr(&s.value);
    }

    fn resolve_impl(&mut self, i: &yelang_ast::item::Impl) {
        self.push_rib(RibKind::Opaque);
        // `Self` is the type being implemented.
        let self_symbol = self.resolver.interner.get_or_intern("Self");
        self.add_type_binding(self_symbol, i.self_ty.span);
        // Set self_type so `Self::item` resolves correctly.
        self.resolver.self_type = crate::associated::extract_type_name(&i.self_ty);
        // Impl generic parameters are in scope for all items.
        self.resolve_generic_params(&i.generics.params);
        if let Some(trait_path) = &i.trait_impl {
            self.resolve_type_path(trait_path);
            // Generic arguments in the trait path (e.g. `Aggregate<T, Acc, Out>`)
            // are not resolved by `resolve_type_path`; resolve them explicitly so
            // type parameters like `T` are correctly bound.
            self.resolve_path_generic_args(trait_path);
        }
        self.resolve_type(&i.self_ty);
        for item in &i.items {
            match &item.item {
                yelang_ast::ImplItemKind::Method(m) => {
                    self.resolve_fn(&yelang_ast::FnDef {
                        name: m.name.clone(),
                        generics: m.generics.clone(),
                        sig: m.sig.clone(),
                        body: m.body.clone(),
                        is_const: m.is_const,
                        span: m.name.span(),
                    });
                }
                yelang_ast::ImplItemKind::AssociatedType(at) => {
                    self.resolve_type(&at.ty);
                }
                yelang_ast::ImplItemKind::Constant(c) => {
                    self.resolve_type(&c.ty);
                    if let Some(value) = &c.value {
                        self.resolve_expr(value);
                    }
                }
            }
        }
        self.resolver.self_type = None;
        self.pop_rib();
    }

    fn resolve_param(&mut self, param: &Param) {
        self.resolve_type(&param.ty);
        self.resolve_pattern(&param.pattern);
    }

    fn resolve_expr(&mut self, expr: &Expr) {
        if split_outermost_selector(expr).is_some() {
            self.resolve_selector_chain_expr(expr);
            return;
        }
        match &expr.kind {
            ExprKind::Literal(_) => {}
            ExprKind::InterpolatedString(parts) => {
                for part in parts {
                    if let yelang_ast::StringPart::Expr(e) = part {
                        self.resolve_expr(e);
                    }
                }
            }
            ExprKind::Path(path) => {
                self.resolve_value_path(path);
            }
            ExprKind::Underscore => {}
            ExprKind::Binary(bin) => {
                self.resolve_expr(&bin.left);
                self.resolve_expr(&bin.right);
            }
            ExprKind::Unary(unary) => {
                self.resolve_expr(&unary.expr);
            }
            ExprKind::AssignEq(assign) => {
                self.resolve_expr(&assign.target);
                self.resolve_expr(&assign.value);
            }
            ExprKind::AssignOp(assign) => {
                self.resolve_expr(&assign.target);
                self.resolve_expr(&assign.value);
            }
            ExprKind::DestructureAssign(assign) => {
                self.resolve_pattern(&assign.pattern);
                self.resolve_expr(&assign.value);
            }
            ExprKind::Try(try_expr) => {
                self.resolve_expr(&try_expr.base);
            }
            ExprKind::If(if_expr) => {
                self.resolve_expr(&if_expr.condition);
                self.resolve_block_expr(&if_expr.then_block);
                if let Some(else_expr) = &if_expr.else_expr {
                    self.resolve_expr(else_expr);
                }
            }
            ExprKind::Let(let_expr) => {
                self.resolve_expr(&let_expr.expr);
                self.resolve_pattern(&let_expr.pattern);
            }
            ExprKind::Match(match_expr) => {
                self.resolve_expr(&match_expr.scrutinee);
                for arm in &match_expr.arms {
                    self.push_rib(RibKind::Pat);
                    self.resolve_pattern(&arm.pattern);
                    if let Some(guard) = &arm.guard {
                        self.resolve_expr(guard);
                    }
                    self.resolve_expr(&arm.body);
                    self.pop_rib();
                }
            }
            ExprKind::Ternary(ternary) => {
                self.resolve_expr(&ternary.condition);
                self.resolve_expr(&ternary.if_true);
                self.resolve_expr(&ternary.if_false);
            }
            ExprKind::Loop(loop_expr) => {
                self.push_breakable(loop_expr.label.as_ref().map(|l| l.symbol), true, expr.span);
                self.push_rib(RibKind::Loop);
                self.resolve_block_expr(&loop_expr.body);
                self.pop_rib();
                self.pop_breakable();
            }
            ExprKind::While(while_expr) => {
                self.resolve_expr(&while_expr.condition);
                self.push_breakable(while_expr.label.as_ref().map(|l| l.symbol), true, expr.span);
                self.push_rib(RibKind::Loop);
                self.resolve_block_expr(&while_expr.body);
                self.pop_rib();
                self.pop_breakable();
            }
            ExprKind::ForLoop(for_loop) => {
                self.resolve_expr(&for_loop.iter);
                self.push_breakable(for_loop.label.as_ref().map(|l| l.symbol), true, expr.span);
                self.push_rib(RibKind::Pat);
                self.resolve_pattern(&for_loop.pat);
                self.resolve_block_expr(&for_loop.body);
                self.pop_rib();
                self.pop_breakable();
            }
            ExprKind::Break(break_expr) => {
                self.resolve_break(break_expr);
            }
            ExprKind::Continue(continue_expr) => {
                self.resolve_continue(continue_expr);
            }
            ExprKind::Return(opt) => {
                if let Some(e) = opt {
                    self.resolve_expr(e);
                }
            }
            ExprKind::TypeCast(cast) => {
                self.resolve_expr(&cast.base);
                self.resolve_type(&cast.ty);
            }
            ExprKind::TypeAscription(asc) => {
                self.resolve_expr(&asc.expr);
                self.resolve_type(&asc.ty);
            }
            ExprKind::IsType(is_type) => {
                self.resolve_expr(&is_type.expr);
                self.resolve_type(&is_type.ty);
            }
            ExprKind::Struct(struct_expr) => {
                self.resolve_type_path(&struct_expr.path);
                for field in &struct_expr.fields {
                    self.resolve_expr(&field.value);
                }
                if let Some(rest) = &struct_expr.rest {
                    self.resolve_expr(rest);
                }
            }
            ExprKind::Array(array) => match &array.kind {
                yelang_ast::ArrayKind::List(elements) => {
                    for elem in elements {
                        self.resolve_expr(elem);
                    }
                }
                yelang_ast::ArrayKind::Repeat { value, count } => {
                    self.resolve_expr(value);
                    self.resolve_expr(count);
                }
            },
            ExprKind::Object(obj) => {
                for field in &obj.fields {
                    self.resolve_expr(field.value());
                }
            }
            ExprKind::Tuple(exprs) => {
                for e in exprs {
                    self.resolve_expr(e);
                }
            }
            ExprKind::Range(range) => {
                if let Some(start) = &range.start {
                    self.resolve_expr(start);
                }
                if let Some(end) = &range.end {
                    self.resolve_expr(end);
                }
            }
            ExprKind::Comprehension(comp) => {
                self.resolve_expr(&comp.element);
                for var in &comp.variables {
                    self.resolve_pattern(&var.pattern);
                    self.resolve_expr(&var.source);
                }
                if let Some(cond) = &comp.condition {
                    self.resolve_expr(cond);
                }
            }
            ExprKind::MemberAccess(access) => {
                self.resolve_expr(access.base());
            }
            ExprKind::ArrayAccess(access) => {
                self.resolve_expr(access.base());
                match access.index() {
                    yelang_ast::ArrayIndex::Single(idx) => self.resolve_expr(idx.expr()),
                    yelang_ast::ArrayIndex::Range(r) => {
                        if let Some(s) = &r.start {
                            self.resolve_expr(s);
                        }
                        if let Some(e) = &r.end {
                            self.resolve_expr(e);
                        }
                    }
                    yelang_ast::ArrayIndex::Filter(e) => self.resolve_expr(&e),
                    yelang_ast::ArrayIndex::OrderBy(clause) => {
                        for part in &clause.orders {
                            self.resolve_expr(&part.field);
                        }
                    }
                    yelang_ast::ArrayIndex::GroupBy(selector) => {
                        for key in selector.keys() {
                            self.resolve_expr(key.expr());
                        }
                    }
                    _ => {}
                }
            }
            ExprKind::DocumentAccess(doc) => {
                self.resolve_expr(doc.base());
                for field in doc.object().fields() {
                    match field {
                        yelang_ast::DocumentField::KeyVal(kv) => {
                            self.resolve_expr(&kv.value);
                        }
                        yelang_ast::DocumentField::Spread(s) => {
                            self.resolve_expr(&s.expr);
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::BindAt(bind_at) => {
                self.resolve_expr(&bind_at.base);
            }
            ExprKind::Call(call) => {
                self.resolve_expr(&call.callee);
                for arg in &call.args {
                    match arg {
                        yelang_ast::CallArgument::Positional(e) => self.resolve_expr(e),
                        yelang_ast::CallArgument::Named(_, e) => self.resolve_expr(e),
                    }
                }
            }
            ExprKind::MethodCall(method_call) => {
                self.resolve_expr(&method_call.receiver);
                for arg in &method_call.arguments {
                    match arg {
                        yelang_ast::CallArgument::Positional(e) => self.resolve_expr(e),
                        yelang_ast::CallArgument::Named(_, e) => self.resolve_expr(e),
                    }
                }
            }
            ExprKind::Lambda(lambda) => {
                self.push_rib(RibKind::Fn);
                for param in &lambda.fn_sig.params {
                    self.resolve_param(param);
                }
                if let yelang_ast::FnRefType::Type(ty) = &lambda.fn_sig.return_type {
                    self.resolve_type(ty);
                }
                self.resolve_expr(&lambda.body);
                self.pop_rib();
            }
            ExprKind::Block(block) => {
                self.resolve_block_expr(block);
            }
            ExprKind::Query(query) => {
                self.resolve_query(query);
            }
            ExprKind::Grouped(grouped) => {
                self.resolve_expr(&grouped.expr);
            }
            ExprKind::Gen(e) => {
                self.resolve_expr(e);
            }
            ExprKind::Await(e) => {
                self.resolve_expr(e);
            }
            ExprKind::Err => {}
            ExprKind::Dummy => {}
            ExprKind::Async(async_expr) => {
                self.resolve_block_expr(&async_expr.block);
            }
            ExprKind::Intrinsic(intrinsic) => {
                for arg in &intrinsic.args {
                    self.resolve_expr(arg);
                }
            }
        }
    }

    fn resolve_block_expr(&mut self, block: &BlockExpr) {
        let has_label = block.label.is_some();
        if has_label {
            let label = block.label.as_ref().map(|l| l.symbol);
            self.push_breakable(label, false, block.label.as_ref().unwrap().span);
        }
        self.push_rib(RibKind::Block);

        // Hoist item names into the block rib so they are visible throughout
        // the entire block (including before their declaration), matching Rust
        // semantics (RFC 2103 / item hoisting).
        for stmt in &block.statements {
            if let StmtKind::Item(item) = &stmt.kind {
                self.hoist_block_item(item);
            }
        }

        for stmt in &block.statements {
            self.resolve_stmt(stmt);
        }
        self.pop_rib();
        if has_label {
            self.pop_breakable();
        }
    }

    /// Add item names from a block-local item into the current rib before
    /// the item is actually resolved.  This enables forward references like:
    /// `fn foo() { bar(); fn bar() {} }`
    fn hoist_block_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Fn(func) => {
                self.add_value_binding(func.name.symbol, func.name.span());
            }
            ItemKind::Struct(s) => {
                self.add_type_binding(s.name.symbol, s.name.span());
            }
            ItemKind::Enum(e) => {
                self.add_type_binding(e.name.symbol, e.name.span());
                // Enum variants are also visible in the same scope as the enum.
                for variant in &e.variants {
                    self.add_type_binding(variant.name.symbol, variant.span);
                    self.add_value_binding(variant.name.symbol, variant.span);
                }
            }
            ItemKind::TypeAlias(ta) => {
                self.add_type_binding(ta.name.symbol, ta.name.span());
            }
            ItemKind::Trait(t) => {
                self.add_type_binding(t.name.symbol, t.name.span());
            }
            ItemKind::Module(_) => {
                // Modules inside blocks are not supported for forward
                // reference hoisting.  (Rust does not allow `mod` inside
                // functions; we follow the same restriction for now.)
            }
            ItemKind::Const(c) => {
                self.add_value_binding(c.name.symbol, c.name.span());
            }
            ItemKind::Static(s) => {
                self.add_value_binding(s.name.symbol, s.name.span());
            }
            ItemKind::Impl(_) | ItemKind::Use(_) => {
                // Impls have no namespace binding; uses are resolved separately.
            }
        }
    }

    fn resolve_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Expr(e) => self.resolve_expr(e),
            StmtKind::TermExpr(e) => self.resolve_expr(e),
            StmtKind::Let(let_stmt) => {
                if let Some(ty) = &let_stmt.ty {
                    self.resolve_type(ty);
                }
                if let Some(init) = &let_stmt.init {
                    self.resolve_expr(init);
                }
                self.resolve_pattern(&let_stmt.pattern);
            }
            StmtKind::Item(item) => {
                self.resolve_item(item);
            }
            StmtKind::Empty => {}
        }
    }

    fn resolve_pattern(&mut self, pattern: &Pattern) {
        match &pattern.pattern {
            PatternKind::Binding {
                name, subpattern, ..
            } => {
                if let Some(sub) = subpattern {
                    self.resolve_pattern(sub);
                }
                // Add the binding to the current value rib.
                self.add_value_binding(name.symbol, pattern.span);
            }
            PatternKind::Wildcard => {}
            PatternKind::Path(path) => {
                self.resolve_type_path(path);
            }
            PatternKind::Literal(_) => {}
            PatternKind::Tuple { patterns } => {
                for p in patterns {
                    self.resolve_pattern(p);
                }
            }
            PatternKind::Struct { path, fields, .. } => {
                self.resolve_type_path(path);
                for field in fields {
                    self.resolve_pattern(&field.pattern);
                }
            }
            PatternKind::Record { fields, .. } => {
                for field in fields {
                    self.resolve_pattern(&field.pattern);
                }
            }
            PatternKind::TupleStruct { path, patterns } => {
                self.resolve_type_path(path);
                for p in patterns {
                    self.resolve_pattern(p);
                }
            }
            PatternKind::Slice { patterns } => {
                for p in patterns {
                    self.resolve_pattern(p);
                }
            }
            PatternKind::Ref { pattern, .. } => {
                self.resolve_pattern(pattern);
            }
            PatternKind::Or(patterns) => {
                for p in patterns {
                    self.resolve_pattern(p);
                }
            }
            PatternKind::Rest { .. } => {}
            PatternKind::Range(range) => {
                if let Some(start) = &range.start {
                    self.resolve_expr(start);
                }
                if let Some(end) = &range.end {
                    self.resolve_expr(end);
                }
            }
            PatternKind::Grouped(pat) => {
                self.resolve_pattern(pat);
            }
            PatternKind::Absent => {}
        }
    }

    fn resolve_type(&mut self, ty: &Type) {
        match &ty.kind {
            TypeKind::Named(path) => {
                self.resolve_type_path(path);
                // Generic arguments inside path segments (e.g. `MapIter<Self, T, U>`)
                // are not resolved by `resolve_type_path`, so walk them explicitly.
                self.resolve_path_generic_args(path);
            }
            TypeKind::Tuple(tys) => {
                for t in tys {
                    self.resolve_type(t);
                }
            }
            TypeKind::Array(t, len) => {
                self.resolve_type(t);
                self.resolve_expr(len);
            }
            TypeKind::Slice(t) => {
                self.resolve_type(t);
            }
            TypeKind::Ref { ty, .. } => {
                self.resolve_type(ty);
            }
            TypeKind::RawPtr { ty, .. } => {
                self.resolve_type(ty);
            }
            TypeKind::Function(func_ty) => {
                for param in &func_ty.params {
                    self.resolve_type(param);
                }
                self.resolve_type(&func_ty.return_type);
            }
            TypeKind::ForAll { params, ty } => {
                self.push_rib(RibKind::Opaque);
                for p in &params.params {
                    match p {
                        yelang_ast::TypeBinderParam::Type(tp) => {
                            self.add_type_binding(tp.name.symbol, ty.span);
                        }
                        yelang_ast::TypeBinderParam::Const(c) => {
                            self.add_value_binding(c.name.symbol, c.ty.span);
                            self.resolve_type(&c.ty);
                        }
                    }
                }
                self.resolve_type(ty);
                self.pop_rib();
            }
            TypeKind::Never => {}
            TypeKind::Infer => {}
            TypeKind::Literal(_) => {}
            TypeKind::Structural(fields) => {
                for field in fields {
                    self.resolve_type(&field.ty);
                }
            }
            TypeKind::Union(types) => {
                for t in types {
                    self.resolve_type(t);
                }
            }
            TypeKind::Operator(op) => match op {
                yelang_ast::TypeOperator::TypeOf(expr) => self.resolve_expr(expr),
                yelang_ast::TypeOperator::ReturnType(ty)
                | yelang_ast::TypeOperator::Parameters(ty) => {
                    self.resolve_type(ty);
                }
                yelang_ast::TypeOperator::Pick(base, keys)
                | yelang_ast::TypeOperator::Omit(base, keys) => {
                    self.resolve_type(base);
                    self.resolve_type(keys);
                }
            },
            TypeKind::ImplTrait(path) | TypeKind::DynTrait(path) => {
                self.resolve_type_path(path);
                self.resolve_path_generic_args(path);
            }
            TypeKind::Error => {}
        }
    }

    /// Recursively resolve generic arguments appearing inside a path's segments.
    ///
    /// `resolve_type_path` only resolves the path spine; type arguments such as
    /// `Self` in `MapIter<Self, T, U>` need their own resolution pass.
    ///
    /// The surface parser cannot distinguish const-generic arguments from type
    /// arguments in angle brackets (`Vector<T, N>` parses `N` as a type), so we
    /// first try the value namespace for single-segment path arguments. If they
    /// resolve to a const/generic value, we accept them without emitting a type
    /// resolution error.
    fn resolve_path_generic_args(&mut self, path: &Path) {
        for segment in &path.segments {
            if let Some(yelang_ast::GenericArgs::AngleBracketed(args)) = &segment.args {
                for arg in &args.args {
                    match arg {
                        yelang_ast::AngleBracketedArg::Type(ty) => {
                            if !self.try_resolve_generic_arg_as_value(ty) {
                                self.resolve_type(ty);
                            }
                        }
                        yelang_ast::AngleBracketedArg::Const(expr) => self.resolve_expr(expr),
                        yelang_ast::AngleBracketedArg::AssociatedType { ty, .. } => {
                            self.resolve_type(ty)
                        }
                    }
                }
            }
        }
    }

    /// Try to resolve a generic argument that the parser classified as a type as
    /// a value (e.g. a const generic parameter). Returns true if it succeeded.
    fn try_resolve_generic_arg_as_value(&self, ty: &Type) -> bool {
        let TypeKind::Named(path) = &ty.kind else { return false };
        if path.segments.len() != 1 {
            return false;
        }
        let seg = &path.segments[0];
        if seg.args.is_some() {
            return false;
        }
        self.resolver
            .resolve_name(Namespace::Value, seg.ident.symbol, seg.ident.span())
            .is_some()
    }

    fn resolve_where_clause(&mut self, where_clause: &yelang_ast::WhereClause) {
        for predicate in &where_clause.predicates {
            match predicate {
                yelang_ast::WherePredicate::TraitBound { ty, bounds } => {
                    self.resolve_type(ty);
                    for bound in bounds {
                        self.resolve_trait_bound(bound);
                    }
                }
                yelang_ast::WherePredicate::TypeEq { lhs, rhs } => {
                    self.resolve_type(lhs);
                    self.resolve_type(rhs);
                }
                yelang_ast::WherePredicate::ForAll { params, predicate, .. } => {
                    self.push_rib(RibKind::Opaque);
                    for param in &params.params {
                        match param {
                            yelang_ast::TypeBinderParam::Type(tp) => {
                                self.add_type_binding(tp.name.symbol, tp.name.span);
                                for bound in &tp.bounds {
                                    self.resolve_trait_bound(bound);
                                }
                            }
                            yelang_ast::TypeBinderParam::Const(c) => {
                                self.resolve_type(&c.ty);
                                self.add_value_binding(c.name.symbol, c.name.span);
                            }
                        }
                    }
                    self.resolve_where_clause(&yelang_ast::WhereClause {
                        predicates: vec![*predicate.clone()],
                        span: where_clause.span,
                    });
                    self.pop_rib();
                }
            }
        }
    }

    fn resolve_trait_bound(&mut self, bound: &yelang_ast::TraitBound) {
        if let Some(binder) = &bound.binder {
            self.push_rib(RibKind::Opaque);
            for param in &binder.params {
                match param {
                    yelang_ast::TypeBinderParam::Type(tp) => {
                        self.add_type_binding(tp.name.symbol, tp.name.span);
                        for b in &tp.bounds {
                            self.resolve_trait_bound(b);
                        }
                    }
                    yelang_ast::TypeBinderParam::Const(c) => {
                        self.resolve_type(&c.ty);
                        self.add_value_binding(c.name.symbol, c.name.span);
                    }
                }
            }
        }

        self.resolve_type_path(&bound.path);
        self.resolve_path_generic_args(&bound.path);

        // Parenthesized `Fn(A, B) -> C` arguments on the bound path are stored
        // on the final segment and need separate resolution.
        if let Some(last) = bound.path.segments.last() {
            if let Some(yelang_ast::GenericArgs::Parenthesized(p)) = &last.args {
                for ty in &p.ins {
                    self.resolve_type(ty);
                }
                if let Some(out) = &p.out {
                    self.resolve_type(out);
                }
            }
        }

        if bound.binder.is_some() {
            self.pop_rib();
        }
    }

    fn resolve_type_path(&mut self, path: &Path) {
        if let Some(res) = resolve_type_path(self.resolver, path) {
            self.record_path_resolution(path, &res);
            self.check_path_privacy(path, &res);
        } else if !path.segments.is_empty() {
            let name = path.segments[0].ident.symbol;
            let span = path.span;
            self.resolver
                .errors
                .push(ResolutionError::NotFound { name, span });
        }
    }

    fn resolve_value_path(&mut self, path: &Path) {
        if let Some(res) = resolve_value_path(self.resolver, path) {
            self.record_path_resolution(path, &res);
            self.check_path_privacy(path, &res);
        } else if !path.segments.is_empty() {
            let name = path.segments[0].ident.symbol;
            let span = path.span;
            self.resolver
                .errors
                .push(ResolutionError::NotFound { name, span });
        }
    }

    /// If a path resolved to a definition (not a local), record it in
    /// `def_resolutions` so HIR lowering can look it up by span.
    fn record_path_resolution(&mut self, path: &Path, res: &Resolution) {
        if let Resolution::Def { def_id } = res {
            self.resolver.def_resolutions.insert(path.span, *def_id);
        }
    }

    fn check_path_privacy(&mut self, path: &Path, res: &Resolution) {
        if let Resolution::Def { def_id } = res {
            if !path.segments.is_empty() {
                let name = path.segments.last().unwrap().ident.symbol;
                let span = path.span;
                if !crate::privacy::check_accessibility(
                    self.resolver,
                    *def_id,
                    self.resolver.current_module,
                    name,
                    span,
                ) {
                    let def_module = self
                        .resolver
                        .definitions
                        .get(*def_id)
                        .and_then(|d| d.parent)
                        .unwrap_or(self.resolver.module_tree.root.def_id);
                    self.resolver.errors.push(ResolutionError::PrivacyError {
                        name,
                        span,
                        def_module,
                        use_module: self.resolver.current_module,
                    });
                }
            }
        }
    }

    fn resolve_query(&mut self, query: &yelang_ast::Query) {
        match &query.kind {
            yelang_ast::QueryKind::Select(select) => {
                // Introduce the root collection label (e.g. `users`) and the
                // element binder (e.g. `u`) into the value scope, then resolve
                // all type annotations and sub-expressions in that scope.
                for node in &select.from {
                    if let Some(var) = &node.var {
                        self.add_value_binding(var.symbol, query.span);
                    }
                    if let Some(bind) = &node.bind {
                        self.add_value_binding(bind.symbol, query.span);
                    }
                    if let Some(ty) = &node.ty {
                        self.resolve_type(ty);
                    }
                    self.resolve_node_modifiers(&node.modifiers);
                }

                // `links` traversals introduce edge/target binders and labels.
                for path in &select.links {
                    self.resolve_select_link_path(path);
                }

                // Per-root tail modifiers in multi-root SELECT.
                for root_mods in &select.post_links_for {
                    self.resolve_node_modifiers(&root_mods.modifiers);
                }

                if let Some(where_clause) = &select.where_clause {
                    self.resolve_expr(where_clause);
                }

                if let Some(group_by) = &select.group_by {
                    self.resolve_group_by(group_by, query.span);
                }

                if let Some(order) = &select.order_by {
                    for part in order {
                        self.resolve_expr(&part.field);
                    }
                }
                if let Some(range) = &select.range {
                    if let Some(start) = &range.start {
                        self.resolve_expr(start);
                    }
                    if let Some(end) = &range.end {
                        self.resolve_expr(end);
                    }
                }

                self.resolve_expr(&select.projection);
            }
            yelang_ast::QueryKind::Create(create) => {
                self.resolve_create_update_header(
                    create.var.symbol,
                    create.binding.symbol,
                    &create.table,
                    query.span,
                );
                self.resolve_creation_data(&create.data);
                for path in &create.links {
                    self.resolve_create_path(path);
                }
                if let Some(ret) = &create.return_ {
                    self.resolve_expr(ret);
                }
            }
            yelang_ast::QueryKind::Update(update) => {
                self.resolve_create_update_header(
                    update.var.symbol,
                    update.binding.symbol,
                    &update.table,
                    query.span,
                );
                match &update.mutation {
                    yelang_ast::query::UpdateMutation::Merge(obj) => {
                        for field in &obj.fields {
                            self.resolve_expr(&field.value());
                        }
                    }
                    yelang_ast::query::UpdateMutation::Set(setters) => {
                        for setter in setters {
                            self.resolve_expr(&setter.path);
                            self.resolve_expr(&setter.value);
                        }
                    }
                }
                for path in &update.links {
                    self.resolve_create_path(path);
                }
                if let Some(cond) = &update.condition {
                    self.resolve_expr(cond);
                }
                if let Some(ret) = &update.return_ {
                    self.resolve_expr(ret);
                }
            }
            yelang_ast::QueryKind::Upsert(upsert) => {
                self.resolve_create_update_header(
                    upsert.var.symbol,
                    upsert.binding.symbol,
                    &upsert.table,
                    query.span,
                );
                self.resolve_creation_data(&upsert.data);
                for path in &upsert.links {
                    self.resolve_create_path(path);
                }
                if let Some(ret) = &upsert.return_ {
                    self.resolve_expr(ret);
                }
            }
            yelang_ast::QueryKind::Delete(delete) => {
                self.resolve_create_update_header(
                    delete.var.symbol,
                    delete.binding.symbol,
                    &delete.table,
                    query.span,
                );
                if let Some(cond) = &delete.condition {
                    self.resolve_expr(cond);
                }
                if let Some(ret) = &delete.return_ {
                    self.resolve_expr(ret);
                }
            }
            yelang_ast::QueryKind::Link(link) => {
                for path in &link.paths {
                    self.resolve_create_path(path);
                }
                if let Some(ret) = &link.return_ {
                    self.resolve_expr(ret);
                }
            }
            yelang_ast::QueryKind::Unlink(unlink) => {
                for path in &unlink.paths {
                    self.resolve_link_path(path);
                }
                if let Some(ret) = &unlink.return_ {
                    self.resolve_expr(ret);
                }
            }
        }
    }

    fn resolve_node_modifiers(&mut self, modifiers: &yelang_ast::query::Modifiers) {
        if let Some(filter) = &modifiers.filter {
            self.resolve_expr(filter);
        }
        if let Some(order) = &modifiers.order {
            for part in order {
                self.resolve_expr(&part.field);
            }
        }
        if let Some(range) = &modifiers.range {
            if let Some(start) = &range.start {
                self.resolve_expr(start);
            }
            if let Some(end) = &range.end {
                self.resolve_expr(end);
            }
        }
    }

    fn resolve_select_link_path(&mut self, path: &LinkPath) {
        // The start node is a reference to an already-bound label; its modifiers
        // may reference upstream binders.
        self.resolve_node_modifiers(&path.start.modifiers);

        for segment in &path.segments {
            self.resolve_select_link_segment(segment);
        }
    }

    fn resolve_select_link_segment(&mut self, segment: &LinkSegment) {
        let edge = &segment.edge;
        let target = &segment.target;

        // Edge binder is visible inside the edge modifiers.
        if let Some(bind) = &edge.bind {
            self.add_value_binding(bind.symbol, bind.span);
        }
        if let Some(ty) = &edge.ty {
            self.resolve_type(ty);
        }
        self.resolve_node_modifiers(&edge.modifiers);

        // Target binder and label are visible for the rest of the path and for
        // later sibling paths / the projection.
        let target_span = target
            .var
            .as_ref()
            .map(|i| i.span)
            .or_else(|| target.bind.as_ref().map(|i| i.span))
            .unwrap_or_default();
        if let Some(bind) = &target.bind {
            self.add_value_binding(bind.symbol, bind.span);
        }
        if let Some(var) = &target.var {
            self.add_value_binding(var.symbol, target_span);
        }
        if let Some(ty) = &target.ty {
            self.resolve_type(ty);
        }
        self.resolve_node_modifiers(&target.modifiers);
    }

    fn resolve_group_by(&mut self, group_by: &GroupByClause, span: Span) {
        for key in &group_by.keys {
            self.resolve_expr(&key.expr);
        }
        // The `into <label>` name becomes a value binding for the projection.
        self.add_value_binding(group_by.into.symbol, span);
    }

    fn resolve_create_update_header(
        &mut self,
        var: Symbol,
        binding: Symbol,
        table: &yelang_ast::Type,
        span: Span,
    ) {
        self.add_value_binding(var, span);
        self.resolve_type(table);
        self.add_value_binding(binding, span);
    }

    fn resolve_creation_data(&mut self, data: &yelang_ast::query::CreationData) {
        match data {
            yelang_ast::query::CreationData::Object(obj) => {
                for field in &obj.fields {
                    self.resolve_expr(field.value());
                }
            }
            yelang_ast::query::CreationData::Array(arr) => {
                if let yelang_ast::ArrayKind::List(elems) = &arr.kind {
                    for elem in elems {
                        self.resolve_expr(elem);
                    }
                } else if let yelang_ast::ArrayKind::Repeat { value, count } = &arr.kind {
                    self.resolve_expr(value);
                    self.resolve_expr(count);
                }
            }
        }
    }

    fn resolve_create_path(&mut self, path: &yelang_ast::query::CreatePath) {
        for segment in &path.segments {
            match segment {
                yelang_ast::query::CreatePathSegment::Node(node) => {
                    self.resolve_link_node(node);
                }
                yelang_ast::query::CreatePathSegment::Edge(edge) => {
                    self.add_value_binding(edge.var.symbol, edge.var.span);
                    self.add_value_binding(edge.binding.symbol, edge.binding.span);
                    self.resolve_type(&edge.table);
                    for field in &edge.data.fields {
                        self.resolve_expr(field.value());
                    }
                }
            }
        }
    }

    fn resolve_link_path(&mut self, path: &yelang_ast::query::LinkPath) {
        self.resolve_link_node(&path.start);
        for segment in &path.segments {
            self.resolve_link_edge(&segment.edge);
            self.resolve_link_node(&segment.target);
        }
    }

    fn resolve_link_node(&mut self, node: &yelang_ast::query::Node) {
        if let Some(var) = &node.var {
            self.add_value_binding(var.symbol, var.span);
        }
        if let Some(bind) = &node.bind {
            self.add_value_binding(bind.symbol, bind.span);
        }
        if let Some(ty) = &node.ty {
            self.resolve_type(ty);
        }
        if let Some(filter) = &node.modifiers.filter {
            self.resolve_expr(filter);
        }
        if let Some(order) = &node.modifiers.order {
            for part in order {
                self.resolve_expr(&part.field);
            }
        }
        if let Some(range) = &node.modifiers.range {
            if let Some(start) = &range.start {
                self.resolve_expr(start);
            }
            if let Some(end) = &range.end {
                self.resolve_expr(end);
            }
        }
    }

    fn resolve_link_edge(&mut self, edge: &yelang_ast::query::Edge) {
        if let Some(var) = &edge.var {
            self.add_value_binding(var.symbol, var.span);
        }
        if let Some(bind) = &edge.bind {
            self.add_value_binding(bind.symbol, bind.span);
        }
        if let Some(ty) = &edge.ty {
            self.resolve_type(ty);
        }
        if let Some(filter) = &edge.modifiers.filter {
            self.resolve_expr(filter);
        }
        if let Some(order) = &edge.modifiers.order {
            for part in order {
                self.resolve_expr(&part.field);
            }
        }
        if let Some(range) = &edge.modifiers.range {
            if let Some(start) = &range.start {
                self.resolve_expr(start);
            }
            if let Some(end) = &range.end {
                self.resolve_expr(end);
            }
        }
    }

    /// Resolve an expression that begins with a binder-bearing selector chain.
    /// Pushes a rib for the binder, resolves the source and selector
    /// expressions, then resolves the suffix in the binder's scope.
    fn resolve_selector_chain_expr(&mut self, expr: &Expr) {
        let Some((source, binder, selector, suffix)) = split_outermost_selector(expr) else {
            return;
        };

        self.push_rib(RibKind::Block);
        self.add_value_binding(binder.symbol, binder.span);

        self.resolve_expr(source);
        resolve_selector_index(self, selector);

        self.resolve_expr(&suffix);

        self.pop_rib();
    }

    fn push_rib(&mut self, kind: RibKind) {
        self.resolver.push_rib(kind);
    }

    fn pop_rib(&mut self) {
        self.resolver.pop_rib();
    }

    fn add_value_binding(&mut self, name: Symbol, span: Span) {
        let local_id = self.resolver.next_local_id();
        if let Some(rib) = self.resolver.value_ribs.last_mut() {
            rib.insert(Namespace::Value, name, Resolution::Local { local_id }, span);
        }
    }

    fn add_type_binding(&mut self, name: Symbol, span: Span) {
        let local_id = self.resolver.next_local_id();
        if let Some(rib) = self.resolver.type_ribs.last_mut() {
            rib.insert(Namespace::Type, name, Resolution::Local { local_id }, span);
        }
    }

    fn find_module_by_name(&self, name: Symbol) -> Option<DefId> {
        let current = self.resolver.current_module;
        self.resolver
            .module_tree
            .modules
            .get(&current)
            .and_then(|m| {
                m.items
                    .get(&Namespace::Type)
                    .and_then(|map| map.get(&name))
                    .copied()
            })
    }

    fn push_breakable(&mut self, label: Option<Symbol>, is_loop: bool, span: Span) {
        self.breakable_stack.push(BreakableScope {
            label,
            is_loop,
            span,
        });
    }

    fn pop_breakable(&mut self) {
        self.breakable_stack.pop();
    }

    fn resolve_break(&mut self, break_expr: &BreakExpr) {
        if let Some(value) = &break_expr.value {
            self.resolve_expr(value);
        }
        if let Some(label) = &break_expr.label {
            let label_sym = label.symbol;
            let label_span = label.span;
            if !self.find_label_in_stack(label_sym, false) {
                self.resolver.errors.push(ResolutionError::LabelError {
                    name: label_sym,
                    span: label_span,
                });
            }
        } else if self.breakable_stack.is_empty() {
            self.resolver
                .errors
                .push(ResolutionError::BreakOutsideLoop {
                    span: break_expr.span,
                });
        }
    }

    fn resolve_continue(&mut self, continue_expr: &ContinueExpr) {
        if let Some(label) = &continue_expr.label {
            let label_sym = label.symbol;
            let label_span = label.span;
            if !self.find_label_in_stack(label_sym, true) {
                self.resolver.errors.push(ResolutionError::LabelError {
                    name: label_sym,
                    span: label_span,
                });
            }
        } else if !self.breakable_stack.iter().any(|s| s.is_loop) {
            self.resolver
                .errors
                .push(ResolutionError::ContinueOutsideLoop {
                    span: continue_expr.span,
                });
        }
    }

    fn find_label_in_stack(&self, label: Symbol, require_loop: bool) -> bool {
        for scope in self.breakable_stack.iter().rev() {
            if scope.label == Some(label) {
                if !require_loop || scope.is_loop {
                    return true;
                }
                // Found labeled block but need loop - continue searching
                if require_loop && !scope.is_loop {
                    continue;
                }
            }
        }
        false
    }
}

// -----------------------------------------------------------------------------
// Selector-chain resolution helpers
// -----------------------------------------------------------------------------

/// If `expr` is a postfix expression whose outermost operation is applied to a
/// binder-bearing selector (`base@binder[*]` or `base@binder[where ...]`),
/// return the selector's source expression, binder identifier, selector index,
/// and the suffix expression with the selector replaced by a reference to the
/// binder.
fn split_outermost_selector(expr: &Expr) -> Option<(&Expr, yelang_ast::Ident, &yelang_ast::ArrayIndex, Expr)> {
    match &expr.kind {
        ExprKind::ArrayAccess(access) => {
            if let ExprKind::BindAt(bind) = &access.base().kind {
                match access.index() {
                    yelang_ast::ArrayIndex::Stars { .. }
                    | yelang_ast::ArrayIndex::Filter(_) => {
                        let suffix = path_expr(bind.at);
                        return Some((bind.base.as_ref(), bind.at, access.index(), suffix));
                    }
                    _ => {}
                }
            }
            None
        }
        ExprKind::MemberAccess(access) => {
            let (source, binder, selector, base_suffix) = split_outermost_selector(access.base())?;
            let suffix = Expr {
                kind: ExprKind::MemberAccess(MemberAccess {
                    base: Box::new(base_suffix),
                    member: access.member().clone(),
                }),
                span: expr.span,
            };
            Some((source, binder, selector, suffix))
        }
        ExprKind::MethodCall(call) => {
            let (source, binder, selector, callee_suffix) = split_outermost_selector(&call.receiver)?;
            let suffix = Expr {
                kind: ExprKind::MethodCall(MethodCallExpr {
                    receiver: Box::new(callee_suffix),
                    segment: call.segment.clone(),
                    arguments: call.arguments.clone(),
                }),
                span: expr.span,
            };
            Some((source, binder, selector, suffix))
        }
        ExprKind::Call(call) => {
            let (source, binder, selector, callee_suffix) = split_outermost_selector(&call.callee)?;
            let suffix = Expr {
                kind: ExprKind::Call(CallExpr {
                    callee: Box::new(callee_suffix),
                    args: call.args.clone(),
                }),
                span: expr.span,
            };
            Some((source, binder, selector, suffix))
        }
        ExprKind::DocumentAccess(access) => {
            let (source, binder, selector, base_suffix) = split_outermost_selector(access.base())?;
            let suffix = Expr {
                kind: ExprKind::DocumentAccess(yelang_ast::DocumentAccess {
                    base: Box::new(base_suffix),
                    object: access.object().clone(),
                }),
                span: expr.span,
            };
            Some((source, binder, selector, suffix))
        }
        _ => None,
    }
}

/// Resolve any expressions that appear inside a selector index (filter
/// condition, range bounds, order-by keys) in the current scope.
fn resolve_selector_index(resolver: &mut LateResolver<'_, '_>, selector: &yelang_ast::ArrayIndex) {
    match selector {
        yelang_ast::ArrayIndex::Filter(cond) => resolver.resolve_expr(cond),
        yelang_ast::ArrayIndex::Range(r) => {
            if let Some(start) = &r.start {
                resolver.resolve_expr(start);
            }
            if let Some(end) = &r.end {
                resolver.resolve_expr(end);
            }
        }
        yelang_ast::ArrayIndex::OrderBy(clause) => {
            for part in &clause.orders {
                resolver.resolve_expr(&part.field);
            }
        }
        yelang_ast::ArrayIndex::GroupBy(selector) => {
            for key in selector.keys() {
                resolver.resolve_expr(key.expr());
            }
        }
        _ => {}
    }
}

/// Construct a single-identifier path expression.
fn path_expr(ident: yelang_ast::Ident) -> Expr {
    Expr {
        kind: ExprKind::Path(Path::new_single_ident(ident)),
        span: ident.span,
    }
}
