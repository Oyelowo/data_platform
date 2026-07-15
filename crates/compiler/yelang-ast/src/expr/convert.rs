use crate::tokenizer::TokenKind as AstTokenKind;
use yelang_interner::Interner;
use yelang_lexer::{Literal as LexerLiteral, Token};
use yelang_macro_core::token_tree::{
    Delimiter, Group, Ident, LitKind, Literal, Punct, Spacing, Span, TokenStream, TokenTree,
};

/// Convert a sequence of lexer tokens into a macro `TokenStream`.
///
/// This handles balanced delimiters by building nested `Group`s and expands
/// compound punctuation into the correct sequence of `Punct` tokens so that
/// the stream can be round-tripped back to source text.
pub fn from_lexer_tokens(tokens: &[Token<AstTokenKind>], interner: &Interner) -> TokenStream {
    let mut stream = TokenStream::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(tree) = convert_dollar_crate(tokens, i, interner) {
            stream.push(tree);
            i += 2;
            continue;
        }
        if let Some((trees, consumed)) = convert_token(tokens, i, interner) {
            for tree in trees {
                stream.push(tree);
            }
            i += consumed;
        } else {
            // Skip whitespace, comments, and unrecognized tokens.
            i += 1;
        }
    }
    stream
}

/// Convert `$crate` / `$package` inside macro bodies to special-origin identifiers.
fn convert_dollar_crate(
    tokens: &[Token<AstTokenKind>],
    start: usize,
    interner: &Interner,
) -> Option<TokenTree> {
    let first = tokens.get(start)?;
    if !matches!(first.kind(), AstTokenKind::Dollar) {
        return None;
    }
    let second = tokens.get(start + 1)?;
    let span: Span = first.span().merge(second.span()).into();
    match second.kind() {
        AstTokenKind::Crate => {
            let sym = interner.get_or_intern("crate");
            Some(TokenTree::Ident(Ident::new_crate(
                sym,
                span,
                Default::default(),
            )))
        }
        AstTokenKind::Pkg => {
            let sym = interner.get_or_intern("pkg");
            Some(TokenTree::Ident(Ident::new_crate(
                sym,
                span,
                Default::default(),
            )))
        }
        AstTokenKind::Ident(ident) if interner.resolve(&ident.symbol) == "package" => {
            let sym = interner.get_or_intern("package");
            Some(TokenTree::Ident(Ident::new_package(sym, span)))
        }
        _ => None,
    }
}

fn convert_token(
    tokens: &[Token<AstTokenKind>],
    start: usize,
    interner: &Interner,
) -> Option<(Vec<TokenTree>, usize)> {
    let token = tokens.get(start)?;
    let span: Span = token.span().into();

    match token.kind() {
        AstTokenKind::Ident(ident) => {
            Some((vec![TokenTree::Ident(Ident::new(ident.symbol, span))], 1))
        }
        AstTokenKind::Lit(lit) => {
            convert_literal(lit, span).map(|l| (vec![TokenTree::Literal(l)], 1))
        }
        AstTokenKind::OpenParen => {
            let (inner, consumed) =
                read_delimited(tokens, start, Delimiter::Parenthesis, interner)?;
            Some((vec![TokenTree::Group(inner)], consumed))
        }
        AstTokenKind::OpenBrace => {
            let (inner, consumed) = read_delimited(tokens, start, Delimiter::Brace, interner)?;
            Some((vec![TokenTree::Group(inner)], consumed))
        }
        AstTokenKind::OpenBracket => {
            let (inner, consumed) = read_delimited(tokens, start, Delimiter::Bracket, interner)?;
            Some((vec![TokenTree::Group(inner)], consumed))
        }
        other => {
            if let Some(trees) = convert_punct(other, span) {
                Some((trees, 1))
            } else {
                convert_keyword(other, span, interner).map(|tree| (vec![tree], 1))
            }
        }
    }
}

/// Convert a keyword token into a macro `Ident` token tree.
///
/// Keywords are not represented by a dedicated token-tree variant, so we store
/// them as identifiers.  This lets macro bodies such as `let` statements round
/// trip through macro expansion.
fn convert_keyword(kind: &AstTokenKind, span: Span, interner: &Interner) -> Option<TokenTree> {
    let text = match kind {
        AstTokenKind::Select => "select",
        AstTokenKind::From_ => "from",
        AstTokenKind::Where => "where",
        AstTokenKind::Struct => "struct",
        AstTokenKind::Enum => "enum",
        AstTokenKind::Trait => "trait",
        AstTokenKind::Group => "group",
        AstTokenKind::By => "by",
        AstTokenKind::Order => "order",
        AstTokenKind::Into => "into",
        AstTokenKind::Let => "let",
        AstTokenKind::Fn => "fn",
        AstTokenKind::TypeToken => "type",
        AstTokenKind::DefaultKw => "default",
        AstTokenKind::TypeOf => "typeof",
        AstTokenKind::ReturnType => "returntype",
        AstTokenKind::Parameters => "parameters",
        AstTokenKind::Pick => "pick",
        AstTokenKind::Omit => "omit",
        AstTokenKind::Pub => "pub",
        AstTokenKind::As => "as",
        AstTokenKind::Or => "or",
        AstTokenKind::Mod => "mod",
        AstTokenKind::Mut => "mut",
        AstTokenKind::CreateIndex => "createindex",
        AstTokenKind::Create => "create",
        AstTokenKind::Crate => "crate",
        AstTokenKind::SelfKw => "self",
        AstTokenKind::SelfType => "Self",
        AstTokenKind::Super => "super",
        AstTokenKind::Pkg => "pkg",
        AstTokenKind::Const => "const",
        AstTokenKind::Static => "static",
        AstTokenKind::Update => "update",
        AstTokenKind::Set => "set",
        AstTokenKind::Insert => "insert",
        AstTokenKind::Impl => "impl",
        AstTokenKind::Dyn => "dyn",
        AstTokenKind::Delete => "delete",
        AstTokenKind::For => "for",
        AstTokenKind::Link => "link",
        AstTokenKind::Unlink => "unlink",
        AstTokenKind::Upsert => "upsert",
        AstTokenKind::BeginTransaction => "begintransaction",
        AstTokenKind::CommitTransaction => "committransaction",
        AstTokenKind::CancelTransaction => "canceltransaction",
        AstTokenKind::Enumerate => "enumerate",
        AstTokenKind::Match => "match",
        AstTokenKind::Macro => "macro",
        AstTokenKind::If => "if",
        AstTokenKind::Else => "else",
        AstTokenKind::While => "while",
        AstTokenKind::Loop => "loop",
        AstTokenKind::Async => "async",
        AstTokenKind::Gen => "gen",
        AstTokenKind::Await => "await",
        AstTokenKind::Continue => "continue",
        AstTokenKind::Break => "break",
        AstTokenKind::Yield => "yield",
        AstTokenKind::Return => "return",
        AstTokenKind::Links => "links",
        AstTokenKind::And => "and",
        AstTokenKind::Not => "not",
        AstTokenKind::Xor => "xor",
        AstTokenKind::Is => "is",
        AstTokenKind::In => "in",
        AstTokenKind::On => "on",
        AstTokenKind::Asc => "asc",
        AstTokenKind::Start => "start",
        AstTokenKind::Limit => "limit",
        AstTokenKind::Desc => "desc",
        AstTokenKind::Use => "use",
        AstTokenKind::Null => "null",
        AstTokenKind::Underscore => "_",
        AstTokenKind::Lifetime(sym) => {
            return Some(TokenTree::Ident(Ident::new(*sym, span)));
        }
        _ => return None,
    };
    Some(TokenTree::Ident(Ident::new(
        interner.get_or_intern(text),
        span,
    )))
}

fn read_delimited(
    tokens: &[Token<AstTokenKind>],
    start: usize,
    delimiter: Delimiter,
    interner: &Interner,
) -> Option<(Group, usize)> {
    let open = tokens.get(start)?;
    let close_kind = close_kind(delimiter)?;
    let mut depth = 1;
    let mut i = start + 1;
    while i < tokens.len() {
        let token = &tokens[i];
        if opens_delimiter(token.kind()) == Some(delimiter) {
            depth += 1;
        } else if token.kind() == &close_kind {
            depth -= 1;
            if depth == 0 {
                let inner_tokens = &tokens[start + 1..i];
                let inner = from_lexer_tokens(inner_tokens, interner);
                let merged = open.span().merge(token.span());
                let span: Span = merged.into();
                return Some((Group::new(delimiter, inner, span), i - start + 1));
            }
        }
        i += 1;
    }
    None
}

fn opens_delimiter(kind: &AstTokenKind) -> Option<Delimiter> {
    match kind {
        AstTokenKind::OpenParen => Some(Delimiter::Parenthesis),
        AstTokenKind::OpenBrace => Some(Delimiter::Brace),
        AstTokenKind::OpenBracket => Some(Delimiter::Bracket),
        _ => None,
    }
}

fn close_kind(delimiter: Delimiter) -> Option<AstTokenKind> {
    match delimiter {
        Delimiter::Parenthesis => Some(AstTokenKind::CloseParen),
        Delimiter::Brace => Some(AstTokenKind::CloseBrace),
        Delimiter::Bracket => Some(AstTokenKind::CloseBracket),
        Delimiter::None => None,
    }
}

fn convert_literal(lit: &LexerLiteral, span: Span) -> Option<Literal> {
    match lit {
        LexerLiteral::Int(i) => Some(Literal::new(
            LitKind::Int {
                value: i.value,
                suffix: i.suffix.map(|s| s.to_string()),
            },
            span,
        )),
        LexerLiteral::Float(f) => Some(Literal::new(
            LitKind::Float {
                value: f.value,
                suffix: f.suffix.map(|s| s.to_string()),
            },
            span,
        )),
        LexerLiteral::Bool(b) => Some(Literal::bool(*b, span)),
        LexerLiteral::Char(c) => Some(Literal::char(*c, span)),
        LexerLiteral::Str(s) => Some(Literal::string(s.value, span)),
        _ => None,
    }
}

/// Convert a single lexer punctuation token into one or more macro `Punct` tokens.
///
/// Compound operators are expanded so that they round-trip correctly when
/// rendered back to source (e.g. `<=` becomes `<` + `=`).
fn convert_punct(kind: &AstTokenKind, span: Span) -> Option<Vec<TokenTree>> {
    use AstTokenKind::*;

    let joint = Spacing::Joint;
    let alone = Spacing::Alone;

    let chars = match kind {
        Dot => vec![('.', alone)],
        Comma => vec![(',', alone)],
        Colon => vec![(':', joint)],
        Semicolon => vec![(';', alone)],
        Plus => vec![('+', joint)],
        Minus => vec![('-', joint)],
        Star => vec![('*', joint)],
        Slash => vec![('/', joint)],
        Percent => vec![('%', joint)],
        Caret => vec![('^', joint)],
        Ampersand => vec![('&', joint)],
        Pipe => vec![('|', joint)],
        Bang => vec![('!', joint)],
        LessThan => vec![('<', joint)],
        GreaterThan => vec![('>', joint)],
        Equal => vec![('=', joint)],
        Dollar => vec![('$', joint)],
        QuestionMark => vec![('?', joint)],
        At => vec![('@', joint)],
        Hash => vec![('#', joint)],
        Tilde => vec![('~', joint)],
        Backslash => vec![('\\', joint)],
        Backtick => vec![('`', joint)],
        SingleQuote => vec![('\'', joint)],
        DoubleQuote => vec![('"', joint)],
        Hyphen => vec![('-', joint)],
        // Compound operators.
        DotDotDot => vec![('.', joint), ('.', joint), ('.', alone)],
        DotDotEq => vec![('.', joint), ('.', joint), ('=', alone)],
        ShiftLeftEqual => vec![('<', joint), ('<', joint), ('=', alone)],
        ShiftRightEqual => vec![('>', joint), ('>', joint), ('=', alone)],
        PlusEqual => vec![('+', joint), ('=', alone)],
        MinusEqual => vec![('-', joint), ('=', alone)],
        StarEqual => vec![('*', joint), ('=', alone)],
        SlashEqual => vec![('/', joint), ('=', alone)],
        PercentEqual => vec![('%', joint), ('=', alone)],
        CaretEqual => vec![('^', joint), ('=', alone)],
        AmpersandEqual => vec![('&', joint), ('=', alone)],
        PipeEqual => vec![('|', joint), ('=', alone)],
        LessThanEqual => vec![('<', joint), ('=', alone)],
        GreaterThanEqual => vec![('>', joint), ('=', alone)],
        EqualEqual => vec![('=', joint), ('=', alone)],
        BangEqual => vec![('!', joint), ('=', alone)],
        ArrowBoth => vec![('<', joint), ('-', joint), ('>', alone)],
        ArrowLeft => vec![('<', joint), ('-', alone)],
        ArrowRight => vec![('-', joint), ('>', alone)],
        ArrowRight2Lines => vec![('=', joint), ('>', alone)],
        DotDot => vec![('.', joint), ('.', alone)],
        ColonColon => vec![(':', joint), (':', alone)],
        _ => return None,
    };

    Some(
        chars
            .into_iter()
            .map(|(ch, spacing)| TokenTree::Punct(Punct::new(ch, spacing, span)))
            .collect(),
    )
}
