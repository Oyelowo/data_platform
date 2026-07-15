//! Eager expansion of built-in macros.
//!
//! Eager macros (`concat!`, `stringify!`, `include!`, `env!`, ...) must have
//! their arguments fully resolved *before* the surrounding macro expansion can
//! proceed. This module provides the expansion engine and the individual
//! builtin implementations.
//!
//! Design points:
//! - File-system and environment access are injected through traits so tests
//!   can be deterministic and hermetic.
//! - Expansion operates on `yelang_macro_core::token_tree::TokenStream`, the
//!   same representation used by declarative macros.
//! - The engine recursively walks token trees, expanding eager invocations and
//!   descending into groups.
//! - A simple path parser supports both unqualified (`concat!`) and qualified
//!   (`std::concat!`) invocations.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use yelang_interner::Interner;
use yelang_macro_core::token_tree::{
    Delimiter, Group, Ident, LitKind, Literal, Punct, Spacing, Span as TokenSpan, TokenStream,
    TokenTree,
};

use crate::error::ExpandError;
use yelang_ast::expr::convert::from_lexer_tokens;
use yelang_lexer::Span as LexerSpan;

/// Abstraction over file-system access for `include!`, `include_str!`, and
/// `include_bytes!`.
pub trait FileLoader: std::fmt::Debug {
    /// Resolve `path` relative to `relative_to` and return the absolute path.
    fn resolve_path(&self, relative_to: &Path, path: &str) -> Option<PathBuf>;

    /// Read the entire file as a UTF-8 string.
    fn read_to_string(&self, path: &Path) -> Result<String, std::io::Error>;

    /// Read the entire file as raw bytes.
    fn read_to_bytes(&self, path: &Path) -> Result<Vec<u8>, std::io::Error>;
}

/// Default file-system implementation.
#[derive(Debug, Clone, Default)]
pub struct StdFileLoader;

impl StdFileLoader {
    pub fn new() -> Self {
        Self
    }
}

impl FileLoader for StdFileLoader {
    fn resolve_path(&self, relative_to: &Path, path: &str) -> Option<PathBuf> {
        let base = relative_to.parent()?;
        Some(base.join(path))
    }

    fn read_to_string(&self, path: &Path) -> Result<String, std::io::Error> {
        std::fs::read_to_string(path)
    }

    fn read_to_bytes(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        std::fs::read(path)
    }
}

/// In-memory file loader for deterministic tests.
#[derive(Debug, Clone, Default)]
pub struct MemoryFileLoader {
    files: HashMap<PathBuf, Vec<u8>>,
}

impl MemoryFileLoader {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    pub fn insert<P: Into<PathBuf>>(&mut self, path: P, contents: impl Into<Vec<u8>>) {
        self.files.insert(path.into(), contents.into());
    }
}

impl FileLoader for MemoryFileLoader {
    fn resolve_path(&self, _relative_to: &Path, path: &str) -> Option<PathBuf> {
        // Tests use absolute paths as keys.
        Some(PathBuf::from(path))
    }

    fn read_to_string(&self, path: &Path) -> Result<String, std::io::Error> {
        let bytes = self
            .files
            .get(path)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"))?;
        String::from_utf8(bytes.clone())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    fn read_to_bytes(&self, path: &Path) -> Result<Vec<u8>, std::io::Error> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"))
    }
}

/// Abstraction over environment-variable access for `env!` and `option_env!`.
pub trait EnvProvider: std::fmt::Debug {
    fn var(&self, key: &str) -> Option<String>;
}

/// Default environment implementation.
#[derive(Debug, Clone, Default)]
pub struct StdEnvProvider;

impl StdEnvProvider {
    pub fn new() -> Self {
        Self
    }
}

impl EnvProvider for StdEnvProvider {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var_os(key).map(|s| s.to_string_lossy().into_owned())
    }
}

/// In-memory environment provider for deterministic tests.
#[derive(Debug, Clone, Default)]
pub struct MemoryEnvProvider {
    vars: HashMap<String, String>,
}

impl MemoryEnvProvider {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }
}

impl EnvProvider for MemoryEnvProvider {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }
}

/// A predicate accepted by `cfg!`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfgPredicate {
    Name(String),
    KeyValue(String, String),
    All(Vec<CfgPredicate>),
    Any(Vec<CfgPredicate>),
    Not(Box<CfgPredicate>),
}

/// Active `cfg` options used to evaluate `cfg!` predicates.
#[derive(Debug, Clone, Default)]
pub struct CfgOptions {
    pub names: HashSet<String>,
    pub key_values: HashMap<String, String>,
}

impl CfgOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.names.insert(name.into());
        self
    }

    pub fn with_key_value(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.key_values.insert(key.into(), value.into());
        self
    }

    pub fn check(&self, predicate: &CfgPredicate) -> bool {
        match predicate {
            CfgPredicate::Name(name) => self.names.contains(name),
            CfgPredicate::KeyValue(key, value) => self
                .key_values
                .get(key)
                .map(|v| v == value)
                .unwrap_or(false),
            CfgPredicate::All(preds) => preds.iter().all(|p| self.check(p)),
            CfgPredicate::Any(preds) => preds.iter().any(|p| self.check(p)),
            CfgPredicate::Not(pred) => !self.check(pred),
        }
    }
}

/// Context required to execute eager builtins.
#[derive(Debug)]
pub struct EagerContext<'a> {
    pub interner: &'a Interner,
    pub file_loader: &'a dyn FileLoader,
    pub env_provider: &'a dyn EnvProvider,
    pub current_file: Option<&'a Path>,
    pub cfg_options: CfgOptions,
}

impl<'a> EagerContext<'a> {
    pub fn new(interner: &'a Interner) -> Self {
        Self {
            interner,
            file_loader: &StdFileLoader,
            env_provider: &StdEnvProvider,
            current_file: None,
            cfg_options: CfgOptions::new(),
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

    pub fn with_current_file(mut self, path: &'a Path) -> Self {
        self.current_file = Some(path);
        self
    }

    pub fn with_cfg_options(mut self, cfg: CfgOptions) -> Self {
        self.cfg_options = cfg;
        self
    }
}

/// Built-in macros that expand eagerly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EagerBuiltin {
    Concat,
    ConcatBytes,
    Stringify,
    Include,
    IncludeStr,
    IncludeBytes,
    Env,
    OptionEnv,
    CompileError,
    Cfg,
}

impl EagerBuiltin {
    /// Recognize an eager builtin from a macro name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "concat" => Some(Self::Concat),
            "concat_bytes" => Some(Self::ConcatBytes),
            "stringify" => Some(Self::Stringify),
            "include" => Some(Self::Include),
            "include_str" => Some(Self::IncludeStr),
            "include_bytes" => Some(Self::IncludeBytes),
            "env" => Some(Self::Env),
            "option_env" => Some(Self::OptionEnv),
            "compile_error" => Some(Self::CompileError),
            "cfg" => Some(Self::Cfg),
            _ => None,
        }
    }

    /// Recognize an eager builtin from a token-tree macro path.
    pub fn from_path(segments: &[Ident], interner: &Interner) -> Option<Self> {
        if segments.len() != 1 {
            return None;
        }
        Self::from_name(interner.resolve(&segments[0].sym))
    }
}

/// Information extracted from a token-tree macro invocation.
#[derive(Debug, Clone)]
struct ParsedInvocation {
    path_segments: Vec<Ident>,
    args: TokenStream,
    span: LexerSpan,
}

/// Recursively expand all eager macro invocations in `stream`.
///
/// Returns the expanded stream, or an error if an eager builtin receives
/// malformed arguments or a file/env lookup fails.
pub fn expand_eager_macros_in_stream(
    stream: &TokenStream,
    ctx: &EagerContext<'_>,
) -> Result<TokenStream, ExpandError> {
    let trees: Vec<TokenTree> = stream.clone().into_iter().collect();
    let mut result = TokenStream::new();
    let mut i = 0;

    while i < trees.len() {
        if let Some((inv, consumed)) = try_parse_invocation(&trees[i..], ctx.interner) {
            if EagerBuiltin::from_path(&inv.path_segments, ctx.interner).is_some() {
                let expanded_args = expand_eager_macros_in_stream(&inv.args, ctx)?;
                let builtin = EagerBuiltin::from_path(&inv.path_segments, ctx.interner).unwrap();
                let replacement = expand_eager_builtin(builtin, &expanded_args, ctx, inv.span)?;
                result.extend(replacement);
                i += consumed;
                continue;
            }
        }

        match &trees[i] {
            TokenTree::Group(group) => {
                let expanded_inner = expand_eager_macros_in_stream(&group.stream, ctx)?;
                let mut new_group = group.clone();
                new_group.stream = expanded_inner;
                result.push(TokenTree::Group(new_group));
            }
            other => result.push(other.clone()),
        }
        i += 1;
    }

    Ok(result)
}

/// Try to parse a macro invocation (`path!group`) starting at the first token.
/// Returns the parsed invocation and the number of input tokens consumed.
fn try_parse_invocation(
    trees: &[TokenTree],
    interner: &Interner,
) -> Option<(ParsedInvocation, usize)> {
    let first = trees.first()?;
    let TokenTree::Ident(first_ident) = first else {
        return None;
    };

    let mut path_segments = vec![first_ident.clone()];
    let mut i = 1;

    // Parse `::ident` repeats.
    while i + 2 < trees.len()
        && is_punct(&trees[i], ':')
        && is_punct(&trees[i + 1], ':')
        && matches!(trees[i + 2], TokenTree::Ident(_))
    {
        if let TokenTree::Ident(ident) = &trees[i + 2] {
            path_segments.push(ident.clone());
        }
        i += 3;
    }

    // Expect `!`.
    if i >= trees.len() || !is_punct(&trees[i], '!') {
        return None;
    }
    i += 1;

    // Expect a delimited group.
    let group = trees.get(i)?;
    let TokenTree::Group(group) = group else {
        return None;
    };

    // Reject invocations whose path is not a known eager builtin early, so we
    // do not accidentally consume a user macro named `concat` in an unrelated
    // position. We only treat it as an invocation if the name is an eager
    // builtin; otherwise the caller falls back to normal token handling.
    if EagerBuiltin::from_path(&path_segments, interner).is_none() {
        return None;
    }

    Some((
        ParsedInvocation {
            path_segments,
            args: group.stream.clone(),
            span: group.span.into(),
        },
        i + 1,
    ))
}

fn is_punct(tree: &TokenTree, ch: char) -> bool {
    matches!(tree, TokenTree::Punct(p) if p.ch == ch)
}

pub(crate) fn expand_eager_builtin(
    builtin: EagerBuiltin,
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    match builtin {
        EagerBuiltin::Concat => expand_concat(args, ctx, span),
        EagerBuiltin::ConcatBytes => expand_concat_bytes(args, ctx, span),
        EagerBuiltin::Stringify => expand_stringify(args, ctx, span),
        EagerBuiltin::Include => expand_include(args, ctx, span),
        EagerBuiltin::IncludeStr => expand_include_str(args, ctx, span),
        EagerBuiltin::IncludeBytes => expand_include_bytes(args, ctx, span),
        EagerBuiltin::Env => expand_env(args, ctx, span),
        EagerBuiltin::OptionEnv => expand_option_env(args, ctx, span),
        EagerBuiltin::CompileError => expand_compile_error(args, ctx, span),
        EagerBuiltin::Cfg => expand_cfg(args, ctx, span),
    }
}

// -----------------------------------------------------------------------------
// concat!
// -----------------------------------------------------------------------------

fn expand_concat(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let elements = parse_concat_elements(args, ctx, span)?;
    let mut out = String::new();
    for element in elements {
        match element {
            ConcatElement::String(s) => out.push_str(&s),
            ConcatElement::Char(c) => out.push(c),
            ConcatElement::Text(t) => out.push_str(&t),
        }
    }
    let token_span = TokenSpan::from(span);
    Ok(TokenStream::from_vec(vec![TokenTree::Literal(
        Literal::string(ctx.interner.get_or_intern(&out), token_span),
    )]))
}

enum ConcatElement {
    String(String),
    Char(char),
    Text(String),
}

fn parse_concat_elements(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<Vec<ConcatElement>, ExpandError> {
    let mut elements = Vec::new();
    let mut iter = args.iter().peekable();
    let mut index = 0;

    while let Some(tree) = iter.next() {
        if index % 2 == 1 {
            if !is_punct(tree, ',') {
                return Err(ExpandError::malformed_macro_args(
                    "concat! arguments must be separated by commas".to_string(),
                    span,
                ));
            }
            index += 1;
            continue;
        }

        match tree {
            TokenTree::Literal(lit) => match &lit.kind {
                LitKind::Str { value, .. } => {
                    elements.push(ConcatElement::String(
                        ctx.interner.resolve(value).to_string(),
                    ));
                }
                LitKind::Char(c) => elements.push(ConcatElement::Char(*c)),
                LitKind::Int { value, .. } | LitKind::Float { value, .. } => {
                    elements.push(ConcatElement::Text(ctx.interner.resolve(value).to_string()));
                }
                LitKind::Bool(b) => elements.push(ConcatElement::Text(b.to_string())),
            },
            TokenTree::Ident(ident) => {
                let name = ctx.interner.resolve(&ident.sym);
                if name == "true" || name == "false" {
                    elements.push(ConcatElement::Text(name.to_string()));
                } else {
                    return Err(ExpandError::malformed_macro_args(
                        format!("concat! expected literal, found identifier `{}`", name),
                        span,
                    ));
                }
            }
            TokenTree::Punct(p) if p.ch == '-' => {
                // Negative number: `-` followed by integer or float literal.
                let next = iter.next().ok_or_else(|| {
                    ExpandError::malformed_macro_args(
                        "unexpected end of input after `-`".to_string(),
                        span,
                    )
                })?;
                match next {
                    TokenTree::Literal(lit) => match &lit.kind {
                        LitKind::Int { value, .. } | LitKind::Float { value, .. } => {
                            let mut text = "-".to_string();
                            text.push_str(ctx.interner.resolve(value));
                            elements.push(ConcatElement::Text(text));
                        }
                        _ => {
                            return Err(ExpandError::malformed_macro_args(
                                "expected number after `-`".to_string(),
                                span,
                            ));
                        }
                    },
                    _ => {
                        return Err(ExpandError::malformed_macro_args(
                            "expected number after `-`".to_string(),
                            span,
                        ));
                    }
                }
            }
            _ => {
                return Err(ExpandError::malformed_macro_args(
                    "concat! argument must be a literal".to_string(),
                    span,
                ));
            }
        }
        index += 1;
    }

    // Trailing comma is allowed: it leaves us one past the last argument with
    // an odd index, which is fine.
    Ok(elements)
}

// -----------------------------------------------------------------------------
// concat_bytes!
// -----------------------------------------------------------------------------

fn expand_concat_bytes(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let bytes = parse_concat_bytes_elements(args, ctx, span)?;
    let symbol = ctx
        .interner
        .get_or_intern(String::from_utf8_lossy(&bytes).as_ref());
    let token_span = TokenSpan::from(span);
    Ok(TokenStream::from_vec(vec![TokenTree::Literal(
        Literal::new(
            LitKind::Str {
                value: symbol,
                kind: yelang_macro_core::token_tree::StrKind::Normal,
            },
            token_span,
        ),
    )]))
}

fn parse_concat_bytes_elements(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<Vec<u8>, ExpandError> {
    let mut bytes = Vec::new();
    let mut iter = args.iter().peekable();
    let mut index = 0;

    while let Some(tree) = iter.next() {
        if index % 2 == 1 {
            if !is_punct(tree, ',') {
                return Err(ExpandError::malformed_macro_args(
                    "concat_bytes! arguments must be separated by commas".to_string(),
                    span,
                ));
            }
            index += 1;
            continue;
        }

        match tree {
            TokenTree::Literal(lit) => match &lit.kind {
                LitKind::Str { value, .. } => {
                    let text = ctx.interner.resolve(value);
                    bytes.extend(text.bytes());
                }
                LitKind::Int { value, .. } => {
                    let text = ctx.interner.resolve(value);
                    let b: u8 = text.parse().map_err(|_| {
                        ExpandError::malformed_macro_args(
                            format!("concat_bytes! integer out of u8 range: {}", text),
                            span,
                        )
                    })?;
                    bytes.push(b);
                }
                _ => {
                    return Err(ExpandError::malformed_macro_args(
                        "concat_bytes! expected byte or integer literal".to_string(),
                        span,
                    ));
                }
            },
            TokenTree::Group(group) if group.delimiter == Delimiter::Bracket => {
                for (gi, inner) in group.stream.iter().enumerate() {
                    if gi % 2 == 1 {
                        if !is_punct(inner, ',') {
                            return Err(ExpandError::malformed_macro_args(
                                "array elements must be separated by commas".to_string(),
                                span,
                            ));
                        }
                        continue;
                    }
                    match inner {
                        TokenTree::Literal(lit) => match &lit.kind {
                            LitKind::Str { value, .. } => {
                                let text = ctx.interner.resolve(value);
                                if text.len() != 1 {
                                    return Err(ExpandError::malformed_macro_args(
                                        "concat_bytes! byte literal must be a single byte"
                                            .to_string(),
                                        span,
                                    ));
                                }
                                bytes.push(text.as_bytes()[0]);
                            }
                            LitKind::Int { value, .. } => {
                                let text = ctx.interner.resolve(value);
                                let b: u8 = text.parse().map_err(|_| {
                                    ExpandError::malformed_macro_args(
                                        format!("concat_bytes! integer out of u8 range: {}", text),
                                        span,
                                    )
                                })?;
                                bytes.push(b);
                            }
                            _ => {
                                return Err(ExpandError::malformed_macro_args(
                                    "concat_bytes! array element must be a byte or integer"
                                        .to_string(),
                                    span,
                                ));
                            }
                        },
                        _ => {
                            return Err(ExpandError::malformed_macro_args(
                                "concat_bytes! array element must be a byte or integer".to_string(),
                                span,
                            ));
                        }
                    }
                }
            }
            _ => {
                return Err(ExpandError::malformed_macro_args(
                    "concat_bytes! argument must be a byte literal or array".to_string(),
                    span,
                ));
            }
        }
        index += 1;
    }

    Ok(bytes)
}

// -----------------------------------------------------------------------------
// stringify!
// -----------------------------------------------------------------------------

fn expand_stringify(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let rendered = args.render(ctx.interner);
    let token_span = TokenSpan::from(span);
    Ok(TokenStream::from_vec(vec![TokenTree::Literal(
        Literal::string(ctx.interner.get_or_intern(&rendered), token_span),
    )]))
}

// -----------------------------------------------------------------------------
// include!
// -----------------------------------------------------------------------------

fn expand_include(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let (path_str, _) = parse_single_string_arg(args, ctx, span)?;
    let resolved = resolve_include_path(&path_str, ctx, span)?;
    let contents = ctx
        .file_loader
        .read_to_string(&resolved)
        .map_err(|e| ExpandError::malformed_macro_args(format!("include! failed: {}", e), span))?;

    // Tokenize the included file. We intentionally do not parse it here: the
    // caller will parse the returned tokens according to the invocation context
    // (items, expression, etc.).
    let mut local_interner = ctx.interner.clone();
    let mut lex = yelang_ast::TokenKind::tokenize(&contents, &mut local_interner).map_err(|e| {
        ExpandError::malformed_macro_args(format!("include! tokenize: {}", e), span)
    })?;
    let tokens: Vec<yelang_lexer::Token<_>> =
        std::iter::from_fn(|| lex.advance().map(|t| t.clone())).collect();
    Ok(from_lexer_tokens(&tokens, ctx.interner))
}

// -----------------------------------------------------------------------------
// include_str!
// -----------------------------------------------------------------------------

fn expand_include_str(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let (path_str, _) = parse_single_string_arg(args, ctx, span)?;
    let resolved = resolve_include_path(&path_str, ctx, span)?;
    let contents = ctx.file_loader.read_to_string(&resolved).map_err(|e| {
        ExpandError::malformed_macro_args(format!("include_str! failed: {}", e), span)
    })?;

    let token_span = TokenSpan::from(span);
    Ok(TokenStream::from_vec(vec![TokenTree::Literal(
        Literal::string(ctx.interner.get_or_intern(&contents), token_span),
    )]))
}

// -----------------------------------------------------------------------------
// include_bytes!
// -----------------------------------------------------------------------------

fn expand_include_bytes(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let (path_str, _) = parse_single_string_arg(args, ctx, span)?;
    let resolved = resolve_include_path(&path_str, ctx, span)?;
    let bytes = ctx.file_loader.read_to_bytes(&resolved).map_err(|e| {
        ExpandError::malformed_macro_args(format!("include_bytes! failed: {}", e), span)
    })?;

    // Represent bytes as a comma-separated array literal `[b, b, ...]`.
    let token_span = TokenSpan::from(span);
    let mut inner = TokenStream::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            inner.push(TokenTree::Punct(Punct::alone(',', token_span)));
        }
        let text = format!("{}", b);
        inner.push(TokenTree::Literal(Literal::int(
            ctx.interner.get_or_intern(&text),
            token_span,
        )));
    }
    let array = Group::new(Delimiter::Bracket, inner, token_span);
    Ok(TokenStream::from_vec(vec![TokenTree::Group(array)]))
}

fn resolve_include_path(
    path_str: &str,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<PathBuf, ExpandError> {
    let current = ctx.current_file.unwrap_or_else(|| Path::new(""));
    ctx.file_loader
        .resolve_path(current, path_str)
        .ok_or_else(|| {
            ExpandError::malformed_macro_args("could not resolve include path".to_string(), span)
        })
}

// -----------------------------------------------------------------------------
// env! / option_env!
// -----------------------------------------------------------------------------

fn expand_env(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let (key, _) = parse_single_string_arg(args, ctx, span)?;
    let value = ctx.env_provider.var(&key).ok_or_else(|| {
        ExpandError::malformed_macro_args(
            format!("environment variable `{}` not defined", key),
            span,
        )
    })?;
    let token_span = TokenSpan::from(span);
    Ok(TokenStream::from_vec(vec![TokenTree::Literal(
        Literal::string(ctx.interner.get_or_intern(&value), token_span),
    )]))
}

fn expand_option_env(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let (key, _) = parse_single_string_arg(args, ctx, span)?;
    let token_span = TokenSpan::from(span);
    match ctx.env_provider.var(&key) {
        Some(value) => {
            // Option::<&str>::Some("value")
            let value_lit = Literal::string(ctx.interner.get_or_intern(&value), token_span);
            let mut some_args = TokenStream::new();
            some_args.push(TokenTree::Literal(value_lit));
            let some_group = Group::new(Delimiter::Parenthesis, some_args, token_span);
            let tokens = vec![
                ident("Option", token_span, ctx.interner),
                punct(':', token_span, Spacing::Joint),
                punct(':', token_span, Spacing::Alone),
                punct('<', token_span, Spacing::Alone),
                punct('&', token_span, Spacing::Alone),
                ident("str", token_span, ctx.interner),
                punct('>', token_span, Spacing::Alone),
                punct(':', token_span, Spacing::Joint),
                punct(':', token_span, Spacing::Alone),
                ident("Some", token_span, ctx.interner),
                TokenTree::Group(some_group),
            ];
            Ok(TokenStream::from_vec(tokens))
        }
        None => {
            // Option::<&str>::None
            let tokens = vec![
                ident("Option", token_span, ctx.interner),
                punct(':', token_span, Spacing::Joint),
                punct(':', token_span, Spacing::Alone),
                punct('<', token_span, Spacing::Alone),
                punct('&', token_span, Spacing::Alone),
                ident("str", token_span, ctx.interner),
                punct('>', token_span, Spacing::Alone),
                punct(':', token_span, Spacing::Joint),
                punct(':', token_span, Spacing::Alone),
                ident("None", token_span, ctx.interner),
            ];
            Ok(TokenStream::from_vec(tokens))
        }
    }
}

// -----------------------------------------------------------------------------
// compile_error!
// -----------------------------------------------------------------------------

fn expand_compile_error(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let (msg, _) = parse_single_string_arg(args, ctx, span)?;
    Err(ExpandError::malformed_macro_args(msg, span))
}

// -----------------------------------------------------------------------------
// cfg!
// -----------------------------------------------------------------------------

fn expand_cfg(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<TokenStream, ExpandError> {
    let predicate = parse_cfg_predicate(args, ctx, span)?;
    let value = ctx.cfg_options.check(&predicate);
    let token_span = TokenSpan::from(span);
    Ok(TokenStream::from_vec(vec![TokenTree::Literal(
        Literal::bool(value, token_span),
    )]))
}

fn parse_cfg_predicate(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<CfgPredicate, ExpandError> {
    let mut iter = args.iter().peekable();
    parse_cfg_predicate_from_iter(&mut iter, ctx, span)
}

fn parse_cfg_predicate_from_iter<'a>(
    iter: &mut std::iter::Peekable<impl Iterator<Item = &'a TokenTree>>,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<CfgPredicate, ExpandError> {
    let tree = iter.next().ok_or_else(|| {
        ExpandError::malformed_macro_args("cfg! predicate expected".to_string(), span)
    })?;

    match tree {
        TokenTree::Ident(ident) => {
            let name = ctx.interner.resolve(&ident.sym);
            match name {
                "all" | "any" => {
                    let open = iter.next().ok_or_else(|| {
                        ExpandError::malformed_macro_args(format!("{}! expected `(`", name), span)
                    })?;
                    let TokenTree::Group(group) = open else {
                        return Err(ExpandError::malformed_macro_args(
                            format!("{}! expected `(`", name),
                            span,
                        ));
                    };
                    if group.delimiter != Delimiter::Parenthesis {
                        return Err(ExpandError::malformed_macro_args(
                            format!("{}! expected `(`", name),
                            span,
                        ));
                    }
                    let mut inner = group.stream.iter().peekable();
                    let mut preds = Vec::new();
                    while inner.peek().is_some() {
                        preds.push(parse_cfg_predicate_from_iter(&mut inner, ctx, span)?);
                        if let Some(comma) = inner.peek() {
                            if is_punct(comma, ',') {
                                inner.next();
                            } else {
                                return Err(ExpandError::malformed_macro_args(
                                    "expected `,` in cfg! predicate".to_string(),
                                    span,
                                ));
                            }
                        }
                    }
                    if name == "all" {
                        Ok(CfgPredicate::All(preds))
                    } else {
                        Ok(CfgPredicate::Any(preds))
                    }
                }
                "not" => {
                    let open = iter.next().ok_or_else(|| {
                        ExpandError::malformed_macro_args("not! expected `(`".to_string(), span)
                    })?;
                    let TokenTree::Group(group) = open else {
                        return Err(ExpandError::malformed_macro_args(
                            "not! expected `(`".to_string(),
                            span,
                        ));
                    };
                    if group.delimiter != Delimiter::Parenthesis {
                        return Err(ExpandError::malformed_macro_args(
                            "not! expected `(`".to_string(),
                            span,
                        ));
                    }
                    let mut inner = group.stream.iter().peekable();
                    let pred = parse_cfg_predicate_from_iter(&mut inner, ctx, span)?;
                    Ok(CfgPredicate::Not(Box::new(pred)))
                }
                _ => {
                    // `name` or `name = "value"`
                    if let Some(eq) = iter.peek() {
                        if is_punct(eq, '=') {
                            iter.next();
                            let value_tree = iter.next().ok_or_else(|| {
                                ExpandError::malformed_macro_args(
                                    "cfg! expected value after `=`".to_string(),
                                    span,
                                )
                            })?;
                            let TokenTree::Literal(lit) = value_tree else {
                                return Err(ExpandError::malformed_macro_args(
                                    "cfg! value must be a string literal".to_string(),
                                    span,
                                ));
                            };
                            let LitKind::Str { value, .. } = &lit.kind else {
                                return Err(ExpandError::malformed_macro_args(
                                    "cfg! value must be a string literal".to_string(),
                                    span,
                                ));
                            };
                            let value = ctx.interner.resolve(value).to_string();
                            return Ok(CfgPredicate::KeyValue(name.to_string(), value));
                        }
                    }
                    Ok(CfgPredicate::Name(name.to_string()))
                }
            }
        }
        _ => Err(ExpandError::malformed_macro_args(
            "cfg! predicate must start with an identifier".to_string(),
            span,
        )),
    }
}

// -----------------------------------------------------------------------------
// Shared helpers
// -----------------------------------------------------------------------------

fn parse_single_string_arg(
    args: &TokenStream,
    ctx: &EagerContext<'_>,
    span: LexerSpan,
) -> Result<(String, TokenSpan), ExpandError> {
    let mut iter = args.iter();
    let tree = iter.next().ok_or_else(|| {
        ExpandError::malformed_macro_args("expected a string literal argument".to_string(), span)
    })?;

    let lit = match tree {
        TokenTree::Literal(lit) => lit,
        _ => {
            return Err(ExpandError::malformed_macro_args(
                "expected a string literal argument".to_string(),
                span,
            ));
        }
    };

    let LitKind::Str { value, .. } = &lit.kind else {
        return Err(ExpandError::malformed_macro_args(
            "expected a string literal argument".to_string(),
            span,
        ));
    };

    // Reject trailing tokens except a single trailing comma.
    let remaining: Vec<_> = iter.collect();
    if !remaining.is_empty() && !(remaining.len() == 1 && is_punct(remaining[0], ',')) {
        return Err(ExpandError::malformed_macro_args(
            "expected exactly one string literal argument".to_string(),
            span,
        ));
    }

    Ok((ctx.interner.resolve(value).to_string(), lit.span))
}

fn ident(name: &str, span: TokenSpan, interner: &Interner) -> TokenTree {
    TokenTree::Ident(Ident::new(interner.get_or_intern(name), span))
}

fn punct(ch: char, span: TokenSpan, spacing: Spacing) -> TokenTree {
    TokenTree::Punct(Punct::new(ch, spacing, span))
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with<'a>(
        interner: &'a Interner,
        files: &'a MemoryFileLoader,
        env: &'a MemoryEnvProvider,
    ) -> EagerContext<'a> {
        EagerContext::new(interner)
            .with_file_loader(files)
            .with_env_provider(env)
            .with_current_file(Path::new("/test/main.ye"))
    }

    #[test]
    fn concat_strings_and_chars() {
        let interner = Interner::new();
        let files = MemoryFileLoader::new();
        let env = MemoryEnvProvider::new();
        let ctx = ctx_with(&interner, &files, &env);

        // Build token stream for concat!("a", 'b', 1, true)
        let mut inner = TokenStream::new();
        inner.push(TokenTree::Literal(Literal::string(
            interner.get_or_intern("a"),
            TokenSpan::default(),
        )));
        inner.push(TokenTree::Punct(Punct::alone(',', TokenSpan::default())));
        inner.push(TokenTree::Literal(Literal::char('b', TokenSpan::default())));
        inner.push(TokenTree::Punct(Punct::alone(',', TokenSpan::default())));
        inner.push(TokenTree::Literal(Literal::int(
            interner.get_or_intern("1"),
            TokenSpan::default(),
        )));
        inner.push(TokenTree::Punct(Punct::alone(',', TokenSpan::default())));
        inner.push(TokenTree::Literal(Literal::bool(
            true,
            TokenSpan::default(),
        )));

        let result = expand_concat(&inner, &ctx, LexerSpan::default()).unwrap();
        assert_eq!(result.render(&interner), "\"ab1true\"");
    }

    #[test]
    fn stringify_renders_tokens() {
        let interner = Interner::new();
        let files = MemoryFileLoader::new();
        let env = MemoryEnvProvider::new();
        let ctx = ctx_with(&interner, &files, &env);

        let mut inner = TokenStream::new();
        inner.push(TokenTree::Literal(Literal::int(
            interner.get_or_intern("1"),
            TokenSpan::default(),
        )));
        inner.push(TokenTree::Punct(Punct::alone('+', TokenSpan::default())));
        inner.push(TokenTree::Literal(Literal::int(
            interner.get_or_intern("2"),
            TokenSpan::default(),
        )));

        let result = expand_stringify(&inner, &ctx, LexerSpan::default()).unwrap();
        // The token-tree renderer emits minimal spacing; both "1+2" and "1 + 2"
        // tokenize back to the same token sequence, so this is semantically valid.
        assert_eq!(result.render(&interner), "\"1+2\"");
    }

    #[test]
    fn env_looks_up_variable() {
        let interner = Interner::new();
        let files = MemoryFileLoader::new();
        let mut env = MemoryEnvProvider::new();
        env.insert("YELANG_TEST", "hello");
        let ctx = ctx_with(&interner, &files, &env);

        let mut inner = TokenStream::new();
        inner.push(TokenTree::Literal(Literal::string(
            interner.get_or_intern("YELANG_TEST"),
            TokenSpan::default(),
        )));

        let result = expand_env(&inner, &ctx, LexerSpan::default()).unwrap();
        assert_eq!(result.render(&interner), "\"hello\"");
    }

    #[test]
    fn cfg_evaluates_predicate() {
        let interner = Interner::new();
        let files = MemoryFileLoader::new();
        let env = MemoryEnvProvider::new();
        let cfg = CfgOptions::new()
            .with_name("unix")
            .with_key_value("feature", "foo");
        let ctx = EagerContext::new(&interner)
            .with_file_loader(&files)
            .with_env_provider(&env)
            .with_cfg_options(cfg);

        // cfg!(unix)
        let mut inner = TokenStream::new();
        inner.push(TokenTree::Ident(Ident::new(
            interner.get_or_intern("unix"),
            TokenSpan::default(),
        )));
        let result = expand_cfg(&inner, &ctx, LexerSpan::default()).unwrap();
        assert_eq!(result.render(&interner), "true");

        // cfg!(not(windows))
        let mut inner = TokenStream::new();
        inner.push(TokenTree::Ident(Ident::new(
            interner.get_or_intern("not"),
            TokenSpan::default(),
        )));
        let mut not_args = TokenStream::new();
        not_args.push(TokenTree::Ident(Ident::new(
            interner.get_or_intern("windows"),
            TokenSpan::default(),
        )));
        inner.push(TokenTree::Group(Group::new(
            Delimiter::Parenthesis,
            not_args,
            TokenSpan::default(),
        )));
        let result = expand_cfg(&inner, &ctx, LexerSpan::default()).unwrap();
        assert_eq!(result.render(&interner), "true");
    }
}
