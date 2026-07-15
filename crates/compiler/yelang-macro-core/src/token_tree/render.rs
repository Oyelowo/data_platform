use yelang_interner::Interner;

use super::{Delimiter, Group, Ident, LitKind, Literal, StrKind};

/// Render a group to a source string.
pub fn render_group(group: &Group, interner: &Interner) -> String {
    let (open, close) = match group.delimiter {
        Delimiter::Parenthesis => ("(", ")"),
        Delimiter::Brace => ("{", "}"),
        Delimiter::Bracket => ("[", "]"),
        Delimiter::None => ("", ""),
    };
    let inner = group.stream.render(interner);
    format!("{}{}{}", open, inner, close)
}

/// Render an identifier to a source string.
pub fn render_ident(ident: &Ident, interner: &Interner) -> String {
    let name = interner.resolve(&ident.sym);
    if ident.is_raw {
        format!("r#{}", name)
    } else {
        name.to_string()
    }
}

/// Render a literal to a source string.
pub fn render_literal(lit: &Literal, interner: &Interner) -> String {
    match &lit.kind {
        LitKind::Int { value, suffix } => {
            let mut s = interner.resolve(value).to_string();
            if let Some(suffix) = suffix {
                s.push_str(suffix);
            }
            s
        }
        LitKind::Float { value, suffix } => {
            let mut s = interner.resolve(value).to_string();
            if let Some(suffix) = suffix {
                s.push_str(suffix);
            }
            s
        }
        LitKind::Str { value, kind } => {
            let text = interner.resolve(value);
            match kind {
                StrKind::Normal => format!("\"{}\"", text),
                StrKind::Raw(n) => {
                    let hashes = "#".repeat(*n);
                    format!("r{}\"{}\"{}", hashes, text, hashes)
                }
            }
        }
        LitKind::Char(c) => format!("'{}'", c),
        LitKind::Bool(b) => b.to_string(),
    }
}

/// Decide whether a space is needed between two rendered token strings.
pub fn needs_space(prev: &str, next: &str) -> bool {
    let prev_last = prev.chars().last().unwrap_or(' ');
    let next_first = next.chars().next().unwrap_or(' ');

    // Word-like tokens (identifiers, literals, and closing delimiters) must be
    // separated from word-like following tokens to avoid merging into a single
    // identifier/literal.
    let prev_is_word_like = prev_last.is_alphanumeric()
        || prev_last == '_'
        || prev_last == '"'
        || prev_last == '\''
        || prev_last == ')'
        || prev_last == ']'
        || prev_last == '}';
    let next_is_word_like = next_first.is_alphanumeric()
        || next_first == '_'
        || next_first == '"'
        || next_first == '\'';

    // Separators also need a space before a word-like token so that
    // `(a,b)` renders as `(a, b)`. We match the whole rendered previous token
    // to avoid splitting multi-character punctuation.
    let prev_is_lonely_separator = prev == "," || prev == ";";

    (prev_is_word_like || prev_is_lonely_separator) && next_is_word_like
}
