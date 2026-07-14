use yelang_ast::{
    Array, AssignEqExpr, AssignOpExpr, AsyncExpr, BinaryExpr, BlockExpr, BreakExpr, CallExpr,
    ComprehensionExpr, ContinueExpr, DestructureAssignExpr, DocumentAccess, Expr, ExprKind, FieldDef,
    FnDef, ForLoopExpr, GroupedExpr, Ident, IfExpr, IsTypeExpr, Item, ItemKind, LambdaExpr, LetExpr,
    LetStmt, Literal, LoopExpr, MatchExpr, MemberAccess, MethodCallExpr, ModKind, Object, Param,
    Pattern, PatternKind, Path, Program, RangeExpr, Stmt, StmtKind, StructExpr, TupleExpr, Type,
    TypeKind, UnaryExpr, WhileExpr,
};
use yelang_ast::item::{Const, Enum, Impl, Static, Struct, Trait, TypeAlias};
use yelang_interner::Symbol;
use yelang_lexer::Span;
use yelang_util::DefId;

use crate::{
    def_collector::DefKind,
    error::ResolutionError,
    namespaces::Namespace,
    path::{resolve_path, resolve_type_path, resolve_value_path},
    rib::{Resolution, Rib, RibKind},
    scope::Resolver,
};

pub struct LateResolver<'a, 'b> {
    resolver: &'b mut Resolver<'a>,
}

impl<'a, 'b> LateResolver<'a, 'b> {
    pub fn new(resolver: &'b mut Resolver<'a>) -> Self {
        Self { resolver }
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

    fn resolve_fn(&mut self, func: &FnDef) {
        // Add generic params to type scope.
        self.push_rib(RibKind::Fn);
        for param in &func.generics.params {
            if let yelang_ast::GenericParam::Type(tp) = param {
                self.add_type_binding(tp.name.symbol, func.span);
            }
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
        for param in &s.generics.params {
            if let yelang_ast::GenericParam::Type(tp) = param {
                self.add_type_binding(tp.name.symbol, s.span);
            }
        }
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
        for param in &e.generics.params {
            if let yelang_ast::GenericParam::Type(tp) = param {
                self.add_type_binding(tp.name.symbol, e.name.span);
            }
        }
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
        for param in &ta.generics.params {
            if let yelang_ast::GenericParam::Type(tp) = param {
                self.add_type_binding(tp.name.symbol, ta.span);
            }
        }
        self.resolve_type(&ta.target);
        self.pop_rib();
    }

    fn resolve_trait(&mut self, t: &Trait) {
        self.push_rib(RibKind::Opaque);
        for param in &t.generics.params {
            if let yelang_ast::GenericParam::Type(tp) = param {
                self.add_type_binding(tp.name.symbol, t.name.span);
            }
        }
        for item in &t.items {
            if let yelang_ast::TraitItemKind::Method(m) = &item.item {
                self.push_rib(RibKind::Fn);
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

    fn resolve_const(&mut self, c: &yelang_ast::Const) {
        self.resolve_type(&c.ty);
        self.resolve_expr(&c.value);
    }

    fn resolve_static(&mut self, s: &yelang_ast::Static) {
        self.resolve_type(&s.ty);
        self.resolve_expr(&s.value);
    }

    fn resolve_impl(&mut self, i: &yelang_ast::Impl) {
        if let Some(trait_path) = &i.trait_impl {
            self.resolve_type_path(trait_path);
        }
        self.resolve_type(&i.self_ty);
        for item in &i.items {
            if let yelang_ast::ImplItemKind::Method(m) = &item.item {
                self.resolve_fn(&yelang_ast::FnDef {
                    name: m.name.clone(),
                    generics: m.generics.clone(),
                    sig: m.sig.clone(),
                    body: m.body.clone(),
                    is_const: m.is_const,
                    span: m.name.span(),
                });
            }
        }
    }

    fn resolve_param(&mut self, param: &Param) {
        self.resolve_type(&param.ty);
        self.resolve_pattern(&param.pattern);
    }

    fn resolve_expr(&mut self, expr: &Expr) {
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
                self.push_rib(RibKind::Loop);
                self.resolve_block_expr(&loop_expr.body);
                self.pop_rib();
            }
            ExprKind::While(while_expr) => {
                self.resolve_expr(&while_expr.condition);
                self.push_rib(RibKind::Loop);
                self.resolve_block_expr(&while_expr.body);
                self.pop_rib();
            }
            ExprKind::ForLoop(for_loop) => {
                self.resolve_expr(&for_loop.iter);
                self.push_rib(RibKind::Pat);
                self.resolve_pattern(&for_loop.pat);
                self.resolve_block_expr(&for_loop.body);
                self.pop_rib();
            }
            ExprKind::Break(break_expr) => {
                if let Some(value) = &break_expr.value {
                    self.resolve_expr(value);
                }
            }
            ExprKind::Continue(_) => {}
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
            ExprKind::Array(array) => {
                match &array.kind {
                    yelang_ast::ArrayKind::List(elements) => {
                        for elem in elements {
                            self.resolve_expr(elem);
                        }
                    }
                    yelang_ast::ArrayKind::Repeat { value, count } => {
                        self.resolve_expr(value);
                        self.resolve_expr(count);
                    }
                }
            }
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
        }
    }

    fn resolve_block_expr(&mut self, block: &BlockExpr) {
        self.push_rib(RibKind::Block);
        for stmt in &block.statements {
            self.resolve_stmt(stmt);
        }
        self.pop_rib();
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
                name,
                subpattern,
                ..
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
            TypeKind::Operator(op) => {
                match op {
                    yelang_ast::TypeOperator::TypeOf(expr) => self.resolve_expr(expr),
                    yelang_ast::TypeOperator::ReturnType(ty) | yelang_ast::TypeOperator::Parameters(ty) => {
                        self.resolve_type(ty);
                    }
                    yelang_ast::TypeOperator::Pick(base, keys) | yelang_ast::TypeOperator::Omit(base, keys) => {
                        self.resolve_type(base);
                        self.resolve_type(keys);
                    }
                }
            }
            TypeKind::ImplTrait(path) | TypeKind::DynTrait(path) => {
                self.resolve_type_path(path);
            }
            TypeKind::Error => {}
        }
    }

    fn resolve_type_path(&mut self, path: &Path) {
        if let Some(res) = resolve_type_path(self.resolver, path) {
            // Type path resolved successfully.
        } else if !path.segments.is_empty() {
            let name = path.segments[0].ident.symbol;
            let span = path.span;
            self.resolver.errors.push(ResolutionError::NotFound { name, span });
        }
    }

    fn resolve_value_path(&mut self, path: &Path) {
        if let Some(res) = resolve_value_path(self.resolver, path) {
            // Value path resolved successfully.
        } else if !path.segments.is_empty() {
            let name = path.segments[0].ident.symbol;
            let span = path.span;
            self.resolver.errors.push(ResolutionError::NotFound { name, span });
        }
    }

    fn resolve_query(&mut self, query: &yelang_ast::Query) {
        match &query.kind {
            yelang_ast::QueryKind::Select(select) => {
                for node in &select.from {
                    if let Some(var) = &node.var {
                        self.add_value_binding(var.symbol, query.span);
                    }
                    if let Some(bind) = &node.bind {
                        self.add_value_binding(bind.symbol, query.span);
                    }
                }
            }
            _ => {}
        }
    }

    fn push_rib(&mut self, kind: RibKind) {
        self.resolver.push_rib(kind);
    }

    fn pop_rib(&mut self) {
        self.resolver.pop_rib();
    }

    fn add_value_binding(&mut self, name: Symbol, _span: Span) {
        let local_id = self.resolver.next_local_id();
        if let Some(rib) = self.resolver.value_ribs.last_mut() {
            rib.insert(Namespace::Value, name, Resolution::Local { local_id });
        }
    }

    fn add_type_binding(&mut self, name: Symbol, _span: Span) {
        if let Some(rib) = self.resolver.type_ribs.last_mut() {
            rib.insert(Namespace::Type, name, Resolution::Err);
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
}
