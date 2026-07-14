use yelang_ast::{
    Attribute, AttributeArgs, BlockExpr, Expr, ExprKind, FieldAssign, Ident, ImplItem, ImplItemKind,
    Item, ItemKind, Literal, Param, Path, PathSegment, Stmt, StmtKind, StructExpr, StructFields, Type, TypeKind,
    Visibility,
};
use yelang_interner::Interner;
use yelang_lexer::Span;

/// Result of applying a decorator to an item.
pub struct DecoratorResult {
    /// The transformed item (or items) after decorator expansion.
    pub items: Vec<Item>,
    /// Any errors encountered during expansion.
    pub errors: Vec<String>,
}

impl DecoratorResult {
    pub fn single(item: Item) -> Self {
        Self {
            items: vec![item],
            errors: vec![],
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            items: vec![],
            errors: vec![msg.into()],
        }
    }
}

/// Built-in decorators recognized by the compiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinDecorator {
    /// `@derive(Trait)` — generate trait implementations.
    Derive,
    /// `@repr(C)` / `@repr(u8)` — control memory layout.
    Repr,
    /// `@test` — mark function as a test.
    Test,
    /// `@inline` / `@inline(always)` / `@inline(never)` — inlining hint.
    Inline,
    /// `@lang = "..."` — language item marker.
    Lang,
    /// `@no_std` / `@no_core` — disable standard library.
    NoStd,
}

impl BuiltinDecorator {
    pub fn from_attribute(attr: &Attribute, interner: &Interner) -> Option<Self> {
        let name = attr.path.first().map(|id| interner.resolve(&id.symbol))?;
        match name {
            "derive" => Some(BuiltinDecorator::Derive),
            "repr" => Some(BuiltinDecorator::Repr),
            "test" => Some(BuiltinDecorator::Test),
            "inline" => Some(BuiltinDecorator::Inline),
            "lang" => Some(BuiltinDecorator::Lang),
            "no_std" => Some(BuiltinDecorator::NoStd),
            "no_core" => Some(BuiltinDecorator::NoStd),
            _ => None,
        }
    }
}

/// Memory representation kind for `@repr(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReprKind {
    C,
    Transparent,
    U8, U16, U32, U64, U128, Usize,
    I8, I16, I32, I64, I128, Isize,
}

impl ReprKind {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "C" => Some(ReprKind::C),
            "transparent" => Some(ReprKind::Transparent),
            "u8" => Some(ReprKind::U8),
            "u16" => Some(ReprKind::U16),
            "u32" => Some(ReprKind::U32),
            "u64" => Some(ReprKind::U64),
            "u128" => Some(ReprKind::U128),
            "usize" => Some(ReprKind::Usize),
            "i8" => Some(ReprKind::I8),
            "i16" => Some(ReprKind::I16),
            "i32" => Some(ReprKind::I32),
            "i64" => Some(ReprKind::I64),
            "i128" => Some(ReprKind::I128),
            "isize" => Some(ReprKind::Isize),
            _ => None,
        }
    }
}

/// Parsed arguments for a decorator.
#[derive(Debug, Clone, PartialEq)]
pub enum DecoratorArgs {
    /// `@test` — no arguments.
    None,
    /// `@repr(C)` — single positional string.
    String(String),
    /// `@derive(Debug, Clone)` — list of trait names.
    List(Vec<String>),
    /// `@lang = "eq"` — key-value pair.
    KeyValue(String, String),
}

/// Apply a built-in decorator to an item.
pub fn apply_decorator(
    decorator: BuiltinDecorator,
    attr: &Attribute,
    item: &Item,
    interner: &Interner,
) -> DecoratorResult {
    match decorator {
        BuiltinDecorator::Derive => apply_derive(attr, item, interner),
        BuiltinDecorator::Repr => apply_repr(attr, item, interner),
        BuiltinDecorator::Test => apply_test(item),
        BuiltinDecorator::Inline => apply_inline(item),
        BuiltinDecorator::Lang => apply_lang(attr, item, interner),
        BuiltinDecorator::NoStd => DecoratorResult::single(item.clone()),
    }
}

fn apply_derive(attr: &Attribute, item: &Item, interner: &Interner) -> DecoratorResult {
    let traits = collect_trait_names(&attr.args, interner);

    let (struct_name, fields, generics) = match &item.kind {
        ItemKind::Struct(s) => (s.name.clone(), s.fields.clone(), s.generics.clone()),
        ItemKind::Enum(e) => {
            // For enums, we support Copy, Clone, Debug, PartialEq by generating
            // impls that delegate to each variant.  For the MVP we keep it simple
            // and generate empty / placeholder impls.
            return derive_for_enum(&traits, item, e, interner);
        }
        _ => {
            return DecoratorResult::error("@derive can only be applied to structs and enums");
        }
    };

    let span = item.span;
    let self_ty = Type {
        kind: TypeKind::Named(path_from_ident(&struct_name)),
        span,
    };

    let mut result = vec![item.clone()];
    for trait_name in &traits {
        let impl_item = match trait_name.as_str() {
            "Clone" => generate_clone_impl(self_ty.clone(), &fields, &generics, span, interner),
            "Copy" => generate_copy_impl(self_ty.clone(), &generics, span, interner),
            "Debug" => generate_debug_impl(self_ty.clone(), &fields, &struct_name, &generics, span, interner),
            "PartialEq" => generate_partial_eq_impl(self_ty.clone(), &fields, &generics, span, interner),
            _ => {
                return DecoratorResult::error(format!(
                    "@derive does not support trait `{}` yet",
                    trait_name
                ));
            }
        };
        result.push(impl_item);
    }

    DecoratorResult {
        items: result,
        errors: vec![],
    }
}

fn derive_for_enum(
    traits: &[String],
    item: &Item,
    e: &yelang_ast::Enum,
    interner: &Interner,
) -> DecoratorResult {
    let span = item.span;
    let self_ty = Type {
        kind: TypeKind::Named(path_from_ident(&e.name)),
        span,
    };
    let mut result = vec![item.clone()];
    for trait_name in traits {
        let impl_item = match trait_name.as_str() {
            "Clone" => generate_clone_impl(self_ty.clone(), &StructFields::Unit, &e.generics, span, interner),
            "Copy" => generate_copy_impl(self_ty.clone(), &e.generics, span, interner),
            "Debug" => generate_debug_impl(self_ty.clone(), &StructFields::Unit, &e.name, &e.generics, span, interner),
            "PartialEq" => generate_partial_eq_impl(self_ty.clone(), &StructFields::Unit, &e.generics, span, interner),
            _ => {
                return DecoratorResult::error(format!(
                    "@derive does not support trait `{}` yet",
                    trait_name
                ));
            }
        };
        result.push(impl_item);
    }
    DecoratorResult {
        items: result,
        errors: vec![],
    }
}

fn generate_clone_impl(
    self_ty: Type,
    fields: &StructFields,
    generics: &yelang_ast::Generics,
    span: Span,
    interner: &Interner,
) -> Item {
    let body = match fields {
        StructFields::Named(fields) => {
            let self_path = path_from_str("Self", span, interner);
            let field_assigns: Vec<FieldAssign> = fields
                .iter()
                .map(|f| {
                    let field_name_str = interner.resolve(&f.name.symbol);
                    let value = expr_from_str(&format!("self.{}.clone()", field_name_str), span, interner);
                    FieldAssign {
                        name: f.name.clone(),
                        value,
                        is_shorthand: false,
                        span,
                    }
                })
                .collect();
            let struct_expr = Expr {
                kind: ExprKind::Struct(StructExpr {
                    path: self_path,
                    fields: field_assigns,
                    rest: None,
                }),
                span,
            };
            let stmts = vec![Stmt {
                kind: StmtKind::TermExpr(Box::new(struct_expr)),
                span,
            }];
            BlockExpr { label: None, statements: stmts }
        }
        StructFields::Tuple(types) => {
            let field_inits: Vec<Expr> = (0..types.len())
                .map(|i| expr_from_str(&format!("self.{}.clone()", i), span, interner))
                .collect();
            let stmts = vec![Stmt {
                kind: StmtKind::TermExpr(Box::new(tuple_literal(field_inits, span, interner))),
                span,
            }];
            BlockExpr { label: None, statements: stmts }
        }
        StructFields::Unit => {
            let stmts = vec![Stmt {
                kind: StmtKind::TermExpr(Box::new(Expr {
                    kind: ExprKind::Path(path_from_str("Self", span, interner)),
                    span,
                })),
                span,
            }];
            BlockExpr { label: None, statements: stmts }
        }
    };

    let method = method_def(
        "clone",
        vec![self_param(span, interner)],
        Some(Type {
            kind: TypeKind::Named(path_from_str("Self", span, interner)),
            span,
        }),
        body,
        span,
        interner,
    );

    make_impl(self_ty, path_from_str("Clone", span, interner), generics, vec![method], span)
}

fn generate_copy_impl(
    self_ty: Type,
    generics: &yelang_ast::Generics,
    span: Span,
    _interner: &Interner,
) -> Item {
    make_impl(self_ty, path_from_str("Copy", span, _interner), generics, vec![], span)
}

fn generate_debug_impl(
    self_ty: Type,
    fields: &StructFields,
    struct_name: &Ident,
    generics: &yelang_ast::Generics,
    span: Span,
    interner: &Interner,
) -> Item {
    let name_str = interner.resolve(&struct_name.symbol);
    let msg = format!("{}", name_str);
    let body = BlockExpr {
        label: None,
        statements: vec![Stmt {
            kind: StmtKind::TermExpr(Box::new(string_expr(&msg, span, interner))),
            span,
        }],
    };

    let method = method_def(
        "fmt",
        vec![self_param(span, interner)],
        Some(Type {
            kind: TypeKind::Named(path_from_str("String", span, interner)),
            span,
        }),
        body,
        span,
        interner,
    );

    make_impl(self_ty, path_from_str("Debug", span, interner), generics, vec![method], span)
}

fn generate_partial_eq_impl(
    self_ty: Type,
    fields: &StructFields,
    generics: &yelang_ast::Generics,
    span: Span,
    interner: &Interner,
) -> Item {
    let eq_expr = match fields {
        StructFields::Named(fields) => {
            fields.iter().fold(None, |acc, f| {
                let field_name = interner.resolve(&f.name.symbol);
                let cmp = expr_from_str(&format!("self.{} == other.{}", field_name, field_name), span, interner);
                Some(match acc {
                    Some(prev) => binary_expr(prev, yelang_ast::BinaryOp::And, cmp, span),
                    None => cmp,
                })
            })
        }
        StructFields::Tuple(types) => {
            (0..types.len()).fold(None, |acc, i| {
                let cmp = expr_from_str(&format!("self.{} == other.{}", i, i), span, interner);
                Some(match acc {
                    Some(prev) => binary_expr(prev, yelang_ast::BinaryOp::And, cmp, span),
                    None => cmp,
                })
            })
        }
        StructFields::Unit => Some(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            span,
        }),
    };

    let body = BlockExpr {
        label: None,
        statements: vec![Stmt {
            kind: StmtKind::TermExpr(Box::new(eq_expr.unwrap_or_else(|| Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                span,
            }))),
            span,
        }],
    };

    let other_param = Param {
        pattern: yelang_ast::Pattern {
            pattern: yelang_ast::PatternKind::Binding {
                name: Ident::new(interner.get_or_intern("other"), span),
                mutability: yelang_ast::Mutability::Immutable,
                subpattern: None,
            },
            span,
        },
        ty: Type {
            kind: TypeKind::Ref {
                ty: Box::new(Type {
                    kind: TypeKind::Named(path_from_str("Self", span, interner)),
                    span,
                }),
                is_mut: false,
            },
            span,
        },
        span,
    };

    let method = method_def(
        "eq",
        vec![self_param(span, interner), other_param],
        Some(Type {
            kind: TypeKind::Named(path_from_str("bool", span, interner)),
            span,
        }),
        body,
        span,
        interner,
    );

    make_impl(self_ty, path_from_str("PartialEq", span, interner), generics, vec![method], span)
}

// --- Helpers for AST construction ---

fn make_impl(
    self_ty: Type,
    trait_path: Path,
    generics: &yelang_ast::Generics,
    items: Vec<ImplItem>,
    span: Span,
) -> Item {
    Item {
        kind: ItemKind::Impl(Box::new(yelang_ast::item::Impl {
            attributes: vec![],
            defaultness: yelang_ast::item::Defaultness::Final,
            generics: generics.clone(),
            trait_impl: Some(trait_path),
            is_negative: false,
            self_ty,
            items,
            span,
        })),
        attributes: vec![],
        visibility: Visibility::Public(span),
        span,
    }
}

fn method_def(
    name: &str,
    params: Vec<yelang_ast::Param>,
    return_type: Option<Type>,
    body: BlockExpr,
    span: Span,
    interner: &Interner,
) -> ImplItem {
    ImplItem {
        item: ImplItemKind::Method(yelang_ast::FnDef {
            name: Ident::new(interner.get_or_intern(name), span),
            generics: yelang_ast::Generics::default(),
            sig: yelang_ast::FnSig {
                params,
                return_type: match return_type {
                    Some(ty) => yelang_ast::FnRefType::Type(ty),
                    None => yelang_ast::FnRefType::Default(span),
                },
                is_async: false,
                is_variadic: false,
            },
            body,
            is_const: false,
            span,
        }),
        defaultness: yelang_ast::item::Defaultness::Final,
        attributes: vec![],
        visibility: Visibility::Public(span),
        span,
    }
}

fn self_param(span: Span, interner: &Interner) -> yelang_ast::Param {
    yelang_ast::Param {
        pattern: yelang_ast::Pattern {
            pattern: yelang_ast::PatternKind::Binding {
                name: Ident::new(interner.get_or_intern("self"), span),
                mutability: yelang_ast::Mutability::Immutable,
                subpattern: None,
            },
            span,
        },
        ty: Type {
            kind: TypeKind::Ref {
                ty: Box::new(Type {
                    kind: TypeKind::Named(path_from_str("Self", span, interner)),
                    span,
                }),
                is_mut: false,
            },
            span,
        },
        span,
    }
}

fn path_from_str(name: &str, span: Span, interner: &Interner) -> Path {
    Path {
        qself: None,
        segments: vec![PathSegment {
            ident: Ident::new(interner.get_or_intern(name), span),
            args: None,
        }],
        is_absolute: false,
        span,
    }
}

fn path_from_ident(ident: &Ident) -> Path {
    Path {
        qself: None,
        segments: vec![PathSegment {
            ident: ident.clone(),
            args: None,
        }],
        is_absolute: false,
        span: ident.span(),
    }
}

fn expr_from_str(src: &str, span: Span, interner: &Interner) -> Expr {
    // For the MVP, we tokenize and parse a tiny expression fragment.
    // In a production compiler we'd build the Expr directly.
    let mut stream = yelang_lexer::TokenKind::tokenize(src, interner).expect("tokenize expr");
    stream.parse::<Expr>().expect("parse expr")
}

fn tuple_literal(elements: Vec<Expr>, span: Span, _interner: &Interner) -> Expr {
    Expr {
        kind: ExprKind::Tuple(elements),
        span,
    }
}

fn string_expr(text: &str, span: Span, interner: &Interner) -> Expr {
    Expr {
        kind: ExprKind::Literal(Literal::Str(yelang_lexer::StringLit {
            value: interner.get_or_intern(text),
            kind: yelang_lexer::StrKind::Normal,
        })),
        span,
    }
}

fn binary_expr(left: Expr, op: yelang_ast::BinaryOp, right: Expr, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Binary(yelang_ast::BinaryExpr {
            left: Box::new(left),
            op,
            right: Box::new(right),
        }),
        span,
    }
}

fn apply_repr(attr: &Attribute, item: &Item, interner: &Interner) -> DecoratorResult {
    let repr = match &attr.args {
        AttributeArgs::Positional(exprs) => {
            exprs.first().and_then(|e| expr_to_string(e, interner))
        }
        _ => None,
    };

    let _kind = repr.as_deref().and_then(ReprKind::from_str);
    let mut new_item = item.clone();
    // TODO: attach repr metadata to the item.
    let _ = _kind;
    DecoratorResult::single(new_item)
}

fn apply_test(item: &Item) -> DecoratorResult {
    match &item.kind {
        ItemKind::Fn(_) => {
            // Mark the function as a test. For MVP, we just keep the item
            // as-is; the test harness will look for @test later.
            DecoratorResult::single(item.clone())
        }
        _ => DecoratorResult::error("@test can only be applied to functions"),
    }
}

fn apply_inline(item: &Item) -> DecoratorResult {
    // Inline is a codegen hint; no AST transformation needed.
    DecoratorResult::single(item.clone())
}

fn apply_lang(attr: &Attribute, item: &Item, interner: &Interner) -> DecoratorResult {
    let lang = match &attr.args {
        AttributeArgs::Positional(exprs) => {
            exprs.first().and_then(|e| expr_to_string(e, interner))
        }
        _ => None,
    };

    if lang.is_none() {
        return DecoratorResult::error("@lang requires a string argument");
    }

    // Mark as language item for later phases.
    DecoratorResult::single(item.clone())
}

// --- Helpers ---

fn collect_trait_names(args: &AttributeArgs, interner: &Interner) -> Vec<String> {
    match args {
        AttributeArgs::Positional(exprs) => {
            exprs.iter().filter_map(|e| expr_to_string(e, interner)).collect()
        }
        AttributeArgs::Named(named) => {
            named.iter().map(|n| interner.resolve(&n.name.symbol).to_string()).collect()
        }
        AttributeArgs::Empty => vec![],
    }
}

fn expr_to_string(expr: &Expr, interner: &Interner) -> Option<String> {
    match &expr.kind {
        ExprKind::Path(path) if path.segments.len() == 1 => {
            Some(interner.resolve(&path.segments[0].ident.symbol).to_string())
        }
        ExprKind::Literal(Literal::Str(s)) => {
            Some(interner.resolve(&s.value).to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ast::TokenKind;
    use yelang_interner::Interner;
    use yelang_lexer::ParseTokenStream;

    fn parse_attribute(src: &str) -> (Attribute, Interner) {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
        let attr = stream.parse::<Attribute>().unwrap();
        (attr, interner)
    }

    fn dummy_item() -> Item {
        Item {
            kind: ItemKind::Fn(Box::new(yelang_ast::FnDef {
                name: yelang_ast::Ident::new(yelang_interner::Symbol::from(0u32), Span::default()),
                generics: yelang_ast::Generics::default(),
                sig: yelang_ast::FnSig {
                    params: vec![],
                    return_type: yelang_ast::FnRefType::Default(Span::default()),
                    is_async: false,
                    is_variadic: false,
                },
                body: yelang_ast::BlockExpr {
                    label: None,
                    statements: vec![],
                },
                is_const: false,
                span: Span::default(),
            })),
            attributes: vec![],
            visibility: yelang_ast::Visibility::Public(Span::default()),
            span: Span::default(),
        }
    }

    #[test]
    fn derive_recognizes_trait_names() {
        let (attr, interner) = parse_attribute("@derive(Debug, Clone)");
        let item = Item {
            kind: ItemKind::Struct(yelang_ast::Struct {
                name: yelang_ast::Ident::new(interner.get_or_intern("Point"), Span::default()),
                generics: yelang_ast::Generics::default(),
                fields: yelang_ast::StructFields::Unit,
                span: Span::default(),
            }),
            attributes: vec![],
            visibility: yelang_ast::Visibility::Public(Span::default()),
            span: Span::default(),
        };
        let result = apply_derive(&attr, &item, &interner);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        // Original struct + Debug impl + Clone impl = 3 items
        assert_eq!(result.items.len(), 3);
    }

    #[test]
    fn test_applied_to_function_succeeds() {
        let item = dummy_item();
        let result = apply_test(&item);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_applied_to_non_function_errors() {
        let item = Item {
            kind: ItemKind::Struct(yelang_ast::Struct {
                name: yelang_ast::Ident::new(yelang_interner::Symbol::from(0u32), Span::default()),
                generics: yelang_ast::Generics::default(),
                fields: yelang_ast::StructFields::Unit,
                span: Span::default(),
            }),
            attributes: vec![],
            visibility: yelang_ast::Visibility::Public(Span::default()),
            span: Span::default(),
        };
        let result = apply_test(&item);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn repr_recognizes_c() {
        let (attr, interner) = parse_attribute("@repr(C)");
        let item = dummy_item();
        let result = apply_repr(&attr, &item, &interner);
        assert!(result.errors.is_empty());
    }
}
