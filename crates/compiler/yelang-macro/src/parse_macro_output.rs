use yelang_ast::{Expr, Item, Pattern, Program, Stmt, TokenKind as AstTokenKind, Type};
use yelang_interner::Interner;
use yelang_lexer::tokenizer::{
    FloatLit as LexerFloatLit, Ident as LexerIdent, IdentOrigin as LexerIdentOrigin,
    IntegerLit as LexerIntegerLit, StrKind as LexerStrKind, StringLit as LexerStringLit,
};
use yelang_lexer::{
    Literal as LexerLiteral, Span as LexerSpan, Token, TokenStream as LexerTokenStream,
};
use yelang_macro_core::token_tree::{
    Delimiter, Group, Ident as CoreIdent, LitKind, Literal as CoreLiteral, Punct,
    TokenStream as CoreStream, TokenTree,
};

use crate::eager::{EagerContext, expand_eager_macros_in_stream};

/// Parse a macro-expanded token stream as a single expression, preserving every
/// span and hygiene context from the original macro output.
pub fn parse_expr_from_macro_stream(
    stream: &CoreStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Expr, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let mut lex = macro_stream_to_lexer_stream(&stream, interner)?;
    let expr = lex.parse::<Expr>().map_err(|e| e.to_string())?;
    if !lex.is_eof() {
        return Err("trailing tokens after expression".to_string());
    }
    Ok(expr)
}

/// Parse a macro-expanded token stream as a sequence of items.
pub fn parse_items_from_macro_stream(
    stream: &CoreStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Vec<Item>, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let mut lex = macro_stream_to_lexer_stream(&stream, interner)?;
    let program = lex.parse::<Program>().map_err(|e| e.to_string())?;
    if !lex.is_eof() {
        return Err("trailing tokens after items".to_string());
    }
    Ok(program.items)
}

/// Parse a macro-expanded token stream as a sequence of statements.
pub fn parse_stmts_from_macro_stream(
    stream: &CoreStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Vec<Stmt>, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let mut lex = macro_stream_to_lexer_stream(&stream, interner)?;
    let mut stmts = vec![];
    while !lex.is_eof() {
        let stmt = lex.parse::<Stmt>().map_err(|e| e.to_string())?;
        stmts.push(stmt);
    }
    Ok(stmts)
}

/// Parse a macro-expanded token stream as a single type.
pub fn parse_type_from_macro_stream(
    stream: &CoreStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Type, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let mut lex = macro_stream_to_lexer_stream(&stream, interner)?;
    let ty = lex.parse::<Type>().map_err(|e| e.to_string())?;
    if !lex.is_eof() {
        return Err("trailing tokens after type".to_string());
    }
    Ok(ty)
}

/// Parse a macro-expanded token stream as a single pattern.
pub fn parse_pattern_from_macro_stream(
    stream: &CoreStream,
    interner: &Interner,
    ctx: &EagerContext<'_>,
) -> Result<Pattern, String> {
    let stream = expand_eager_macros_in_stream(stream, ctx).map_err(|e| e.to_string())?;
    let mut lex = macro_stream_to_lexer_stream(&stream, interner)?;
    let pat = lex.parse::<Pattern>().map_err(|e| e.to_string())?;
    if !lex.is_eof() {
        return Err("trailing tokens after pattern".to_string());
    }
    Ok(pat)
}

fn macro_stream_to_lexer_stream(
    stream: &CoreStream,
    interner: &Interner,
) -> Result<LexerTokenStream<AstTokenKind>, String> {
    let tokens = convert_stream(stream, interner)?;
    Ok(LexerTokenStream::new_with_tokens(tokens, interner.clone()))
}

fn convert_stream(
    stream: &CoreStream,
    interner: &Interner,
) -> Result<Vec<Token<AstTokenKind>>, String> {
    let mut tokens = Vec::new();
    let mut punct_buffer: Vec<&Punct> = Vec::new();

    for tree in stream.trees() {
        if let TokenTree::Punct(p) = tree {
            punct_buffer.push(p);
            continue;
        }

        if !punct_buffer.is_empty() {
            tokens.extend(convert_punct_run(&punct_buffer, interner));
            punct_buffer.clear();
        }

        match tree {
            TokenTree::Group(g) => tokens.extend(convert_group(g, interner)?),
            TokenTree::Ident(i) => tokens.extend(convert_ident(i, interner)),
            TokenTree::Literal(l) => tokens.push(convert_literal(l, interner)?),
            TokenTree::Punct(_) => unreachable!("handled above"),
        }
    }

    if !punct_buffer.is_empty() {
        tokens.extend(convert_punct_run(&punct_buffer, interner));
    }

    Ok(tokens)
}

fn convert_group(group: &Group, interner: &Interner) -> Result<Vec<Token<AstTokenKind>>, String> {
    let mut tokens = Vec::new();
    let span: LexerSpan = group.span.into();

    match group.delimiter {
        Delimiter::Parenthesis => {
            tokens.push(Token::new(AstTokenKind::OpenParen, span));
            tokens.extend(convert_stream(&group.stream, interner)?);
            tokens.push(Token::new(AstTokenKind::CloseParen, span));
        }
        Delimiter::Brace => {
            tokens.push(Token::new(AstTokenKind::OpenBrace, span));
            tokens.extend(convert_stream(&group.stream, interner)?);
            tokens.push(Token::new(AstTokenKind::CloseBrace, span));
        }
        Delimiter::Bracket => {
            tokens.push(Token::new(AstTokenKind::OpenBracket, span));
            tokens.extend(convert_stream(&group.stream, interner)?);
            tokens.push(Token::new(AstTokenKind::CloseBracket, span));
        }
        Delimiter::None => {
            tokens.extend(convert_stream(&group.stream, interner)?);
        }
    }

    Ok(tokens)
}

fn convert_ident(ident: &CoreIdent, interner: &Interner) -> Vec<Token<AstTokenKind>> {
    use yelang_macro_core::token_tree::IdentOrigin;

    let span: LexerSpan = ident.span.into();

    match ident.origin {
        IdentOrigin::Crate => vec![
            Token::new(AstTokenKind::Dollar, span),
            Token::new(AstTokenKind::Crate, span),
        ],
        IdentOrigin::Package => {
            let sym = interner.get_or_intern("package");
            let lexer_ident = LexerIdent::new_with_origin(sym, span, LexerIdentOrigin::Package);
            vec![
                Token::new(AstTokenKind::Dollar, span),
                Token::new(AstTokenKind::Ident(lexer_ident), span),
            ]
        }
        IdentOrigin::Plain => {
            let text = interner.resolve(&ident.sym);

            if text == "_" {
                return vec![Token::new(AstTokenKind::Underscore, span)];
            }

            if let Some(kind) = keyword_token_kind(text) {
                return vec![Token::new(kind, span)];
            }

            if text.starts_with('\'') {
                return vec![Token::new(AstTokenKind::Lifetime(ident.sym), span)];
            }

            let lexer_ident = LexerIdent::new_with_origin(ident.sym, span, LexerIdentOrigin::Plain);
            vec![Token::new(AstTokenKind::Ident(lexer_ident), span)]
        }
    }
}

fn convert_literal(
    literal: &CoreLiteral,
    interner: &Interner,
) -> Result<Token<AstTokenKind>, String> {
    let span: LexerSpan = literal.span.into();
    let kind = match &literal.kind {
        LitKind::Int { value, suffix } => {
            let suffix = suffix.as_deref().and_then(parse_int_suffix);
            AstTokenKind::Lit(LexerLiteral::Int(LexerIntegerLit {
                value: *value,
                suffix,
            }))
        }
        LitKind::Float { value, suffix } => {
            let suffix = suffix.as_deref().and_then(parse_float_suffix);
            AstTokenKind::Lit(LexerLiteral::Float(LexerFloatLit {
                value: *value,
                suffix,
            }))
        }
        LitKind::Str { value, kind } => AstTokenKind::Lit(LexerLiteral::Str(LexerStringLit {
            value: *value,
            kind: convert_str_kind(*kind),
        })),
        LitKind::Char(c) => AstTokenKind::Lit(LexerLiteral::Char(*c)),
        LitKind::Bool(b) => AstTokenKind::Lit(LexerLiteral::Bool(*b)),
        LitKind::ByteStr { value, kind: _ } => {
            let text = interner.resolve(value);
            AstTokenKind::Lit(LexerLiteral::Bytes(std::sync::Arc::from(text.as_bytes())))
        }
        LitKind::Byte(b) => AstTokenKind::Lit(LexerLiteral::Bytes(std::sync::Arc::from([*b]))),
    };
    Ok(Token::new(kind, span))
}

fn convert_str_kind(kind: yelang_macro_core::token_tree::StrKind) -> LexerStrKind {
    match kind {
        yelang_macro_core::token_tree::StrKind::Normal => LexerStrKind::Normal,
        yelang_macro_core::token_tree::StrKind::Raw(n) => LexerStrKind::Raw {
            hash_count: n.min(u8::MAX as usize) as u8,
        },
    }
}

fn parse_int_suffix(s: &str) -> Option<yelang_lexer::IntSuffix> {
    use yelang_lexer::IntSuffix;
    match s {
        "i8" => Some(IntSuffix::I8),
        "i16" => Some(IntSuffix::I16),
        "i32" => Some(IntSuffix::I32),
        "i64" => Some(IntSuffix::I64),
        "i128" => Some(IntSuffix::I128),
        "isize" => Some(IntSuffix::Isize),
        "u8" => Some(IntSuffix::U8),
        "u16" => Some(IntSuffix::U16),
        "u32" => Some(IntSuffix::U32),
        "u64" => Some(IntSuffix::U64),
        "u128" => Some(IntSuffix::U128),
        "usize" => Some(IntSuffix::Usize),
        _ => None,
    }
}

fn parse_float_suffix(s: &str) -> Option<yelang_lexer::FloatSuffix> {
    use yelang_lexer::FloatSuffix;
    match s {
        "f16" => Some(FloatSuffix::F16),
        "f32" => Some(FloatSuffix::F32),
        "f64" => Some(FloatSuffix::F64),
        "f128" => Some(FloatSuffix::F128),
        _ => None,
    }
}

fn keyword_token_kind(text: &str) -> Option<AstTokenKind> {
    let kind = match text {
        "select" => AstTokenKind::Select,
        "from" => AstTokenKind::From_,
        "where" => AstTokenKind::Where,
        "struct" => AstTokenKind::Struct,
        "enum" => AstTokenKind::Enum,
        "trait" => AstTokenKind::Trait,
        "group" => AstTokenKind::Group,
        "by" => AstTokenKind::By,
        "order" => AstTokenKind::Order,
        "into" => AstTokenKind::Into,
        "let" => AstTokenKind::Let,
        "fn" => AstTokenKind::Fn,
        "type" => AstTokenKind::TypeToken,
        "default" => AstTokenKind::DefaultKw,
        "typeof" => AstTokenKind::TypeOf,
        "returntype" => AstTokenKind::ReturnType,
        "parameters" => AstTokenKind::Parameters,
        "pick" => AstTokenKind::Pick,
        "omit" => AstTokenKind::Omit,
        "pub" => AstTokenKind::Pub,
        "as" => AstTokenKind::As,
        "or" => AstTokenKind::Or,
        "mod" => AstTokenKind::Mod,
        "mut" => AstTokenKind::Mut,
        "create" => AstTokenKind::Create,
        "crate" => AstTokenKind::Crate,
        "self" => AstTokenKind::SelfKw,
        "Self" => AstTokenKind::SelfType,
        "super" => AstTokenKind::Super,
        "pkg" => AstTokenKind::Pkg,
        "const" => AstTokenKind::Const,
        "static" => AstTokenKind::Static,
        "update" => AstTokenKind::Update,
        "set" => AstTokenKind::Set,
        "insert" => AstTokenKind::Insert,
        "impl" => AstTokenKind::Impl,
        "dyn" => AstTokenKind::Dyn,
        "delete" => AstTokenKind::Delete,
        "for" => AstTokenKind::For,
        "link" => AstTokenKind::Link,
        "unlink" => AstTokenKind::Unlink,
        "upsert" => AstTokenKind::Upsert,
        "begin" => AstTokenKind::BeginTransaction,
        "commit" => AstTokenKind::CommitTransaction,
        "cancel" => AstTokenKind::CancelTransaction,
        "enumerate" => AstTokenKind::Enumerate,
        "distinct" => AstTokenKind::Distinct,
        "match" => AstTokenKind::Match,
        "macro" => AstTokenKind::Macro,
        "if" => AstTokenKind::If,
        "else" => AstTokenKind::Else,
        "while" => AstTokenKind::While,
        "loop" => AstTokenKind::Loop,
        "async" => AstTokenKind::Async,
        "extern" => AstTokenKind::Extern,
        "gen" => AstTokenKind::Gen,
        "await" => AstTokenKind::Await,
        "continue" => AstTokenKind::Continue,
        "break" => AstTokenKind::Break,
        "yield" => AstTokenKind::Yield,
        "return" => AstTokenKind::Return,
        "links" => AstTokenKind::Links,
        "and" => AstTokenKind::And,
        "not" => AstTokenKind::Not,
        "xor" => AstTokenKind::Xor,
        "is" => AstTokenKind::Is,
        "in" => AstTokenKind::In,
        "on" => AstTokenKind::On,
        "asc" => AstTokenKind::Asc,
        "start" => AstTokenKind::Start,
        "limit" => AstTokenKind::Limit,
        "range" => AstTokenKind::RangeKw,
        "hops" => AstTokenKind::HopsKw,
        "desc" => AstTokenKind::Desc,
        "use" => AstTokenKind::Use,
        "null" => AstTokenKind::Null,
        "true" => AstTokenKind::Lit(LexerLiteral::Bool(true)),
        "false" => AstTokenKind::Lit(LexerLiteral::Bool(false)),
        _ => return None,
    };
    Some(kind)
}

fn convert_punct_run(puncts: &[&Punct], _interner: &Interner) -> Vec<Token<AstTokenKind>> {
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < puncts.len() {
        let remaining: String = puncts[i..].iter().map(|p| p.ch).collect();
        if let Some((compound, kind)) = COMPOUND_PUNCTUATION
            .iter()
            .find(|(s, _)| remaining.starts_with(s))
        {
            let len = compound.len();
            let span = merge_punct_spans(puncts, i, len);
            tokens.push(Token::new(kind.clone(), span));
            i += len;
            continue;
        }

        if let Some(token) = single_punct(puncts[i].ch, puncts[i].span.into()) {
            tokens.push(token);
        }
        i += 1;
    }

    tokens
}

fn merge_punct_spans(puncts: &[&Punct], start: usize, len: usize) -> LexerSpan {
    let first: LexerSpan = puncts[start].span.into();
    if len == 1 {
        return first;
    }
    let last: LexerSpan = puncts[start + len - 1].span.into();
    first.merge(last)
}

fn single_punct(ch: char, span: LexerSpan) -> Option<Token<AstTokenKind>> {
    let kind = match ch {
        '.' => AstTokenKind::Dot,
        ',' => AstTokenKind::Comma,
        ':' => AstTokenKind::Colon,
        ';' => AstTokenKind::Semicolon,
        '+' => AstTokenKind::Plus,
        '-' => AstTokenKind::Minus,
        '*' => AstTokenKind::Star,
        '/' => AstTokenKind::Slash,
        '%' => AstTokenKind::Percent,
        '^' => AstTokenKind::Caret,
        '&' => AstTokenKind::Ampersand,
        '|' => AstTokenKind::Pipe,
        '!' => AstTokenKind::Bang,
        '<' => AstTokenKind::LessThan,
        '>' => AstTokenKind::GreaterThan,
        '=' => AstTokenKind::Equal,
        '$' => AstTokenKind::Dollar,
        '?' => AstTokenKind::QuestionMark,
        '@' => AstTokenKind::At,
        '#' => AstTokenKind::Hash,
        '~' => AstTokenKind::Tilde,
        '\\' => AstTokenKind::Backslash,
        '`' => AstTokenKind::Backtick,
        '\'' => AstTokenKind::SingleQuote,
        '"' => AstTokenKind::DoubleQuote,
        '_' => AstTokenKind::Underscore,
        '(' => AstTokenKind::OpenParen,
        ')' => AstTokenKind::CloseParen,
        '{' => AstTokenKind::OpenBrace,
        '}' => AstTokenKind::CloseBrace,
        '[' => AstTokenKind::OpenBracket,
        ']' => AstTokenKind::CloseBracket,
        _ => return None,
    };
    Some(Token::new(kind, span))
}

// Sorted longest-first so greedy matching prefers compound operators.
static COMPOUND_PUNCTUATION: &[(&str, AstTokenKind)] = &[
    ("..=", AstTokenKind::DotDotEq),
    ("...", AstTokenKind::DotDotDot),
    ("<<=", AstTokenKind::ShiftLeftEqual),
    (">>=", AstTokenKind::ShiftRightEqual),
    ("<->", AstTokenKind::ArrowBoth),
    ("==", AstTokenKind::EqualEqual),
    ("&&", AstTokenKind::And),
    ("->", AstTokenKind::ArrowRight),
    ("..", AstTokenKind::DotDot),
    ("<-", AstTokenKind::ArrowLeft),
    ("<=", AstTokenKind::LessThanEqual),
    (">=", AstTokenKind::GreaterThanEqual),
    ("=>", AstTokenKind::ArrowRight2Lines),
    ("::", AstTokenKind::ColonColon),
    ("+=", AstTokenKind::PlusEqual),
    ("-=", AstTokenKind::MinusEqual),
    ("*=", AstTokenKind::StarEqual),
    ("/=", AstTokenKind::SlashEqual),
    ("%=", AstTokenKind::PercentEqual),
    ("^=", AstTokenKind::CaretEqual),
    ("&=", AstTokenKind::AmpersandEqual),
    ("|=", AstTokenKind::PipeEqual),
    ("!=", AstTokenKind::BangEqual),
];

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ast::ExprKind;
    use yelang_interner::Interner;
    use yelang_macro_core::token_tree::{
        Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree,
    };

    fn core_ident(name: &str, interner: &Interner, span: Span) -> TokenTree {
        TokenTree::Ident(Ident::new(interner.get_or_intern(name), span))
    }

    fn core_punct(ch: char, spacing: Spacing, span: Span) -> TokenTree {
        TokenTree::Punct(Punct::new(ch, spacing, span))
    }

    fn path_name(expr: &Expr, interner: &Interner) -> String {
        let ExprKind::Path(path) = &expr.kind else {
            panic!("expected path expression, got {:?}", expr.kind);
        };
        interner
            .resolve(&path.segments.first().unwrap().ident.symbol)
            .to_string()
    }

    fn eager_ctx(interner: &Interner) -> EagerContext<'_> {
        EagerContext::new(interner)
    }

    #[test]
    fn round_trip_simple_expression() {
        let interner = Interner::new();
        let span = Span::default();
        let stream = TokenStream::from_vec(vec![
            core_ident("x", &interner, span),
            core_punct('+', Spacing::Joint, span),
            core_ident("y", &interner, span),
        ]);
        let expr = parse_expr_from_macro_stream(&stream, &interner, &eager_ctx(&interner)).unwrap();
        let ExprKind::Binary(bin) = &expr.kind else {
            panic!("expected binary expression");
        };
        assert_eq!(path_name(&bin.left, &interner), "x");
        assert_eq!(path_name(&bin.right, &interner), "y");
    }

    #[test]
    fn compound_operator_combines() {
        let interner = Interner::new();
        let span = Span::default();
        let stream = TokenStream::from_vec(vec![
            core_ident("x", &interner, span),
            core_punct('<', Spacing::Joint, span),
            core_punct('=', Spacing::Alone, span),
            core_ident("y", &interner, span),
        ]);
        let expr = parse_expr_from_macro_stream(&stream, &interner, &eager_ctx(&interner)).unwrap();
        let ExprKind::Binary(bin) = &expr.kind else {
            panic!("expected binary expression");
        };
        assert!(matches!(bin.op, yelang_ast::BinaryOp::Lte));
    }

    #[test]
    fn keyword_identifier_becomes_keyword_token() {
        let interner = Interner::new();
        let span = Span::default();
        // `let x = 1;`
        let stream = TokenStream::from_vec(vec![
            core_ident("let", &interner, span),
            core_ident("x", &interner, span),
            core_punct('=', Spacing::Alone, span),
            TokenTree::Literal(Literal::int(interner.get_or_intern("1"), span)),
            core_punct(';', Spacing::Alone, span),
        ]);
        let stmts =
            parse_stmts_from_macro_stream(&stream, &interner, &eager_ctx(&interner)).unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn group_with_braces() {
        let interner = Interner::new();
        let span = Span::default();
        let inner = TokenStream::from_vec(vec![core_ident("x", &interner, span)]);
        let stream = TokenStream::from_vec(vec![TokenTree::Group(Group::new(
            Delimiter::Brace,
            inner,
            span,
        ))]);
        let expr = parse_expr_from_macro_stream(&stream, &interner, &eager_ctx(&interner)).unwrap();
        assert!(matches!(expr.kind, ExprKind::Block(_)));
    }

    #[test]
    fn package_origin_emits_dollar_package_ident() {
        let interner = Interner::new();
        let span = Span::default();
        let stream = TokenStream::from_vec(vec![
            TokenTree::Ident(Ident::new_package(interner.get_or_intern("package"), span)),
            core_punct(':', Spacing::Joint, span),
            core_punct(':', Spacing::Alone, span),
            core_ident("bar", &interner, span),
            core_punct('(', Spacing::Alone, span),
            core_punct(')', Spacing::Alone, span),
        ]);
        let stmts =
            parse_stmts_from_macro_stream(&stream, &interner, &eager_ctx(&interner)).unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn crate_origin_emits_dollar_crate_keyword() {
        let interner = Interner::new();
        let span = Span::default();
        let stream = TokenStream::from_vec(vec![
            TokenTree::Ident(Ident::new_crate(
                interner.get_or_intern("crate"),
                span,
                yelang_macro_core::CrateId::new(1),
            )),
            core_punct(':', Spacing::Joint, span),
            core_punct(':', Spacing::Alone, span),
            core_ident("bar", &interner, span),
        ]);
        let stmts =
            parse_stmts_from_macro_stream(&stream, &interner, &eager_ctx(&interner)).unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn empty_stream_rejected_for_expression() {
        let interner = Interner::new();
        let stream = TokenStream::new();
        assert!(parse_expr_from_macro_stream(&stream, &interner, &eager_ctx(&interner)).is_err());
    }

    #[test]
    fn trailing_tokens_after_expression_are_rejected() {
        let interner = Interner::new();
        let span = Span::default();
        let stream = TokenStream::from_vec(vec![
            core_ident("x", &interner, span),
            core_ident("y", &interner, span),
        ]);
        assert!(parse_expr_from_macro_stream(&stream, &interner, &eager_ctx(&interner)).is_err());
    }
}
