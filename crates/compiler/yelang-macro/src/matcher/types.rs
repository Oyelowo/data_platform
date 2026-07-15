use yelang_interner::Symbol;
use yelang_macro_core::token_tree::{Delimiter, TokenTree};

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
    /// A repetition expansion: `$($body)sep*` / `+` / `?`.
    Repeat {
        kind: RepetitionKind,
        sep: Option<TokenTree>,
        ops: Vec<TranscriberOp>,
    },
}

/// Fragment specifiers supported by the matcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// A parsed macro rule.
///
/// Function-like rules have `attr_args` empty and `matcher` matches the
/// invocation's delimited arguments. Attribute and derive rules have
/// `attr_args` matching the attribute arguments and `matcher` matching the
/// annotated item.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroRule {
    pub kind: MacroKind,
    pub attr_args: Vec<MatcherOp>,
    pub matcher: Vec<MatcherOp>,
    pub transcriber: Vec<TranscriberOp>,
}

/// A complete declarative macro definition.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclarativeMacro {
    pub name: String,
    pub rules: Vec<MacroRule>,
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
        }
    }
}

impl std::error::Error for MatcherError {}
