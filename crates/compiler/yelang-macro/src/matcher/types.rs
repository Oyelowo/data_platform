use yelang_interner::Symbol;
use yelang_macro_core::{
    CrateId, MacroDefId,
    token_tree::{Delimiter, TokenStream, TokenTree},
};

/// A single matcher atom inside a macro rule.
#[derive(Debug, Clone, PartialEq)]
pub enum MatcherOp {
    /// A literal token tree that must match exactly.
    Terminal(TokenTree),
    /// A metavariable capture: `$name:fragment`.
    Metavar {
        name: Symbol,
        fragment: FragmentKind,
    },
    /// A delimited sub-matcher.
    Group {
        delimiter: Delimiter,
        ops: Vec<MatcherOp>,
    },
    /// A repetition sub-matcher: `$($matcher)sep*` / `+` / `?`.
    Repeat {
        kind: RepetitionKind,
        sep: Option<TokenTree>,
        ops: Vec<MatcherOp>,
    },
}

/// A metavariable expression inside a transcriber.
#[derive(Debug, Clone, PartialEq)]
pub enum MetavarExpr {
    /// `${count(name)}` or `${count(name, depth)}`.
    Count { name: Symbol, depth: Option<usize> },
    /// `${index()}` or `${index(depth)}`.
    Index { depth: Option<usize> },
    /// `${len()}` or `${len(depth)}`.
    Len { depth: Option<usize> },
    /// `${ignore(name)}`.
    Ignore { name: Symbol },
}

/// A single transcriber atom inside a macro rule.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriberOp {
    /// A literal token tree emitted as-is.
    Terminal(TokenTree),
    /// A delimited group whose contents are also transcribed.
    Group {
        delimiter: Delimiter,
        ops: Vec<TranscriberOp>,
    },
    /// A substitution: `$name`.
    Subst(Symbol),
    /// A fragment field access: `$name.field`.
    FragmentField { name: Symbol, field: Symbol },
    /// A repetition expansion: `$($body)sep*` / `+` / `?`.
    Repeat {
        kind: RepetitionKind,
        sep: Option<TokenTree>,
        ops: Vec<TranscriberOp>,
    },
    /// A metavariable expression: `${count(x)}`, `${index()}`, etc.
    MetavarExpr(MetavarExpr),
    /// `$$` — expands to a single `$` token.
    DollarDollar,
}

/// Fragment specifiers supported by the matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FragmentKind {
    Ident,
    Expr,
    Stmt,
    Block,
    Tt,
    Literal,
    Ty,
    Path,
    Item,
    Pat,
}

impl FragmentKind {
    pub fn from_symbol(interner: &yelang_interner::Interner, sym: Symbol) -> Option<Self> {
        match interner.resolve(&sym) {
            "ident" => Some(FragmentKind::Ident),
            "expr" => Some(FragmentKind::Expr),
            "stmt" => Some(FragmentKind::Stmt),
            "block" => Some(FragmentKind::Block),
            "tt" => Some(FragmentKind::Tt),
            "literal" => Some(FragmentKind::Literal),
            "ty" => Some(FragmentKind::Ty),
            "path" => Some(FragmentKind::Path),
            "item" => Some(FragmentKind::Item),
            "pat" => Some(FragmentKind::Pat),
            _ => None,
        }
    }
}

/// Repetition operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepetitionKind {
    ZeroOrMore,
    OneOrMore,
    ZeroOrOne,
}

impl RepetitionKind {
    pub fn from_char(ch: char) -> Option<Self> {
        match ch {
            '*' => Some(RepetitionKind::ZeroOrMore),
            '+' => Some(RepetitionKind::OneOrMore),
            '?' => Some(RepetitionKind::ZeroOrOne),
            _ => None,
        }
    }
}

/// The kind of a declarative macro rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroKind {
    /// Function-like macro: `name!(...)`.
    FunctionLike,
    /// Attribute macro: `@name(...)` or `#[name(...)]` on an item.
    Attribute,
    /// Derive macro: `@derive(Name)` or `#[derive(Name)]`.
    Derive,
}

/// Fields extracted from a captured fragment for `$name.field` syntax.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FragmentFields {
    /// For `:ident` fragments: the identifier token tree.
    pub name: Option<TokenStream>,
    /// For `:expr` fragments with type ascription: the ascribed type.
    pub ty: Option<TokenStream>,
    /// For `:ty` fragments: the base type name/path.
    pub type_name: Option<TokenStream>,
    /// For `:ty` fragments: the generic arguments.
    pub type_args: Option<TokenStream>,
    /// For `:item` fragments: visibility.
    pub vis: Option<TokenStream>,
    /// For `:item` fragments: item name.
    pub item_name: Option<TokenStream>,
    /// For `:item` fragments: attributes.
    pub attrs: Option<TokenStream>,
}

/// A parsed macro rule.
///
/// Function-like rules have `attr_args` empty and `matcher` matches the
/// invocation's delimited arguments. Attribute and derive rules have
/// `attr_args` matching the attribute arguments and `matcher` matching the
/// annotated item.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroRule {
    pub kind: MacroKind,
    /// Whether this rule requires `unsafe(...)` invocation syntax.
    pub is_unsafe: bool,
    pub attr_args: Vec<MatcherOp>,
    pub matcher: Vec<MatcherOp>,
    pub transcriber: Vec<TranscriberOp>,
}

/// A complete declarative macro definition.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclarativeMacro {
    pub name: String,
    pub rules: Vec<MacroRule>,
    pub def_id: MacroDefId,
    pub defining_crate: CrateId,
}

/// Errors produced while parsing macro rules or matching invocations.
#[derive(Debug, Clone, PartialEq)]
pub enum MatcherError {
    UnexpectedEof,
    Expected(String),
    UnknownFragmentSpecifier(Symbol),
    InvalidRepetition,
    InvalidMatcher(String),
    InvalidTranscriber(String),
    /// A fragment specifier is followed by a token outside its follow set.
    FollowSetViolation {
        fragment: FragmentKind,
        followed_by: String,
    },
    /// An invalid metavariable expression in the transcriber.
    InvalidMetavarExpr(String),
}

impl std::fmt::Display for MatcherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatcherError::UnexpectedEof => write!(f, "unexpected end of macro rule"),
            MatcherError::Expected(s) => write!(f, "expected {}", s),
            MatcherError::UnknownFragmentSpecifier(_) => write!(f, "unknown fragment specifier"),
            MatcherError::InvalidRepetition => write!(f, "invalid repetition syntax"),
            MatcherError::InvalidMatcher(s) => write!(f, "invalid matcher: {}", s),
            MatcherError::InvalidTranscriber(s) => write!(f, "invalid transcriber: {}", s),
            MatcherError::FollowSetViolation {
                fragment,
                followed_by,
            } => write!(
                f,
                "`{:?}` fragment may not be followed by `{}`",
                fragment, followed_by
            ),
            MatcherError::InvalidMetavarExpr(s) => {
                write!(f, "invalid metavariable expression: {}", s)
            }
        }
    }
}

impl std::error::Error for MatcherError {}
