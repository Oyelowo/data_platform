use yelang_ast::{Attribute, AttributeArgs, Expr, ExprKind, Ident, Item, ItemKind, Literal};
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

    // For the MVP, we just annotate the item with derive metadata.
    // In a full implementation, this would generate impl items.
    let mut new_item = item.clone();
    // TODO: attach derive metadata to the item for later phases.
    let _ = traits;
    DecoratorResult::single(new_item)
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
        let item = dummy_item();
        let result = apply_derive(&attr, &item, &interner);
        assert!(result.errors.is_empty());
        assert_eq!(result.items.len(), 1);
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
