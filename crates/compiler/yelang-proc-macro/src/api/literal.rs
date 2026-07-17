//! Literal tokens.

use std::fmt;

use super::Span;

/// A literal token (string, integer, float, char, bool).
#[derive(Debug, Clone, PartialEq)]
pub struct Literal {
    pub(crate) inner: yelang_macro_core::Literal,
    pub(crate) cached: String,
}

impl Literal {
    /// Create a string literal.
    pub fn string<S: Into<String>>(value: S, span: Span) -> Self {
        let value = value.into();
        let sym = super::ident::with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Literal::string(sym, span.into_inner()),
            cached: format!("\"{}\"", value),
        }
    }

    /// Create a raw string literal with `hashes` delimiter hashes.
    ///
    /// `value` is the string contents without quotes; `hashes` is the number of
    /// `#` characters surrounding the raw string (`0` for `r"..."`, `1` for
    /// `r#"..."#`, etc.).
    pub fn raw_string<S: Into<String>>(value: S, hashes: usize, span: Span) -> Self {
        let value = value.into();
        let sym = super::ident::with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Literal::new(
                yelang_macro_core::LitKind::Str {
                    value: sym,
                    kind: yelang_macro_core::StrKind::Raw(hashes),
                },
                span.into_inner(),
            ),
            cached: {
                let hashes_str = "#".repeat(hashes);
                format!("r{}\"{}\"{}", hashes_str, value, hashes_str)
            },
        }
    }

    /// Create a character literal.
    pub fn character(ch: char, span: Span) -> Self {
        Self {
            inner: yelang_macro_core::Literal::char(ch, span.into_inner()),
            cached: format!("'{}'", ch),
        }
    }

    /// Create an integer literal.
    pub fn integer<S: Into<String>>(value: S, span: Span) -> Self {
        let value = value.into();
        let sym = super::ident::with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Literal::int(sym, span.into_inner()),
            cached: value,
        }
    }

    /// Create a floating-point literal.
    pub fn float<S: Into<String>>(value: S, span: Span) -> Self {
        let value = value.into();
        let sym = super::ident::with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Literal::float(sym, span.into_inner()),
            cached: value,
        }
    }

    /// Create a boolean literal.
    pub fn boolean(value: bool, span: Span) -> Self {
        Self {
            inner: yelang_macro_core::Literal::bool(value, span.into_inner()),
            cached: value.to_string(),
        }
    }

    /// Create a byte-string literal.
    pub fn byte_string<S: Into<String>>(value: S, span: Span) -> Self {
        let value = value.into();
        let sym = super::ident::with_api_interner(|i| i.get_or_intern(&value));
        Self {
            inner: yelang_macro_core::Literal::byte_string(sym, span.into_inner()),
            cached: format!("b\"{}\"", value),
        }
    }

    /// Create a byte literal.
    pub fn byte(value: u8, span: Span) -> Self {
        Self {
            inner: yelang_macro_core::Literal::byte(value, span.into_inner()),
            cached: format!("b'{}'", value as char),
        }
    }

    /// Parse a literal from its source text.
    ///
    /// This is the constructor used by the `quote!` macro: it takes a literal
    /// token as it would appear in source code (e.g. `"hello"`, `42`,
    /// `b"bytes"`, `r#"raw"#`) and builds the matching `Literal` value.
    pub fn from_source_text<S: AsRef<str>>(text: S, span: Span) -> Self {
        let text = text.as_ref();

        // Raw strings (normal and byte).
        if let Some((hashes, content)) = parse_raw_string(text, "r") {
            return Self::raw_string(content, hashes, span);
        }
        if let Some((hashes, content)) = parse_raw_string(text, "br") {
            let _ = hashes;
            return Self::byte_string(content, span);
        }

        // Byte string: b"..."
        if let Some(content) = text.strip_prefix("b\"").and_then(|s| s.strip_suffix('"')) {
            match unescape_bytes(content) {
                Ok(bytes) => {
                    let value = String::from_utf8_lossy(&bytes).into_owned();
                    return Self::byte_string(value, span);
                }
                Err(_) => return Self::byte_string(content.to_string(), span),
            }
        }

        // Byte char: b'...'
        if let Some(content) = text.strip_prefix("b'").and_then(|s| s.strip_suffix('\'')) {
            match unescape_bytes(content) {
                Ok(bytes) if bytes.len() == 1 => return Self::byte(bytes[0], span),
                _ => return Self::byte_string(content.to_string(), span),
            }
        }

        // Normal string: "..."
        if let Some(content) = text.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            let value = unescape_str(content).unwrap_or_else(|_| content.to_string());
            return Self::string(value, span);
        }

        // Character: '...'
        if let Some(content) = text.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            let value = unescape_str(content).unwrap_or_else(|_| content.to_string());
            if let Some(ch) = value.chars().next() {
                return Self::character(ch, span);
            }
        }

        // Numbers.
        if is_float(text) {
            Self::float(text, span)
        } else {
            Self::integer(text, span)
        }
    }

    /// The span of this literal.
    pub fn span(&self) -> Span {
        Span::from_inner(self.inner.span)
    }

    /// Return a new literal with the given span.
    pub fn with_span(self, span: Span) -> Self {
        let mut inner = self.inner;
        inner.span = span.into_inner();
        Self {
            inner,
            cached: self.cached,
        }
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.cached)
    }
}

fn parse_raw_string(s: &str, prefix: &str) -> Option<(usize, String)> {
    if !s.starts_with(prefix) {
        return None;
    }
    let bytes = s.as_bytes();
    let mut i = prefix.len();
    let mut hashes = 0;
    while i < bytes.len() && bytes[i] == b'#' {
        hashes += 1;
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'"' {
        return None;
    }
    i += 1;
    let content_start = i;
    loop {
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'"' {
            let content_end = i;
            i += 1;
            let mut closing = 0;
            while i < bytes.len() && bytes[i] == b'#' && closing < hashes {
                closing += 1;
                i += 1;
            }
            if closing == hashes {
                let content =
                    String::from_utf8_lossy(&bytes[content_start..content_end]).into_owned();
                return Some((hashes, content));
            }
            i = content_end + 1;
        } else {
            i += 1;
        }
    }
}

fn is_float(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("0x") || lower.starts_with("0o") || lower.starts_with("0b") {
        return lower.contains('p');
    }
    lower.contains('.') || lower.contains('e')
}

fn unescape_str(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('0') => out.push('\0'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some('x') => {
                let hi = parse_hex_char(chars.next())?;
                let lo = parse_hex_char(chars.next())?;
                let value = (hi << 4) | lo;
                if let Some(ch) = char::from_u32(value as u32) {
                    out.push(ch);
                } else {
                    return Err(format!("invalid \\x escape value {}", value));
                }
            }
            Some('u') => {
                if chars.next() != Some('{') {
                    return Err("expected `{` after `\\u`".to_string());
                }
                let mut hex = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) if c.is_ascii_hexdigit() => hex.push(c),
                        Some(c) => return Err(format!("invalid character `{}` in \\u escape", c)),
                        None => return Err("unterminated `\\u{{...}}` escape".to_string()),
                    }
                }
                let value = u32::from_str_radix(&hex, 16)
                    .map_err(|e| format!("invalid \\u escape: {}", e))?;
                let ch = char::from_u32(value)
                    .ok_or_else(|| format!("invalid Unicode scalar 0x{:X}", value))?;
                out.push(ch);
            }
            Some(other) => {
                // Unknown escapes are preserved literally rather than failing.
                out.push('\\');
                out.push(other);
            }
            None => return Err("dangling backslash".to_string()),
        }
    }
    Ok(out)
}

fn unescape_bytes(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            if !c.is_ascii() {
                return Err(format!("non-ASCII character `{}` in byte literal", c));
            }
            out.push(c as u8);
            continue;
        }
        match chars.next() {
            Some('n') => out.push(b'\n'),
            Some('r') => out.push(b'\r'),
            Some('t') => out.push(b'\t'),
            Some('\\') => out.push(b'\\'),
            Some('0') => out.push(b'\0'),
            Some('\'') => out.push(b'\''),
            Some('"') => out.push(b'"'),
            Some('x') => {
                let hi = parse_hex_char(chars.next())?;
                let lo = parse_hex_char(chars.next())?;
                out.push((hi << 4) | lo);
            }
            Some('u') => return Err("Unicode escapes are not allowed in byte literals".to_string()),
            Some(other) => {
                out.push(b'\\');
                out.push(other as u8);
            }
            None => return Err("dangling backslash".to_string()),
        }
    }
    Ok(out)
}

fn parse_hex_char(c: Option<char>) -> Result<u8, String> {
    match c {
        Some(c) => c
            .to_digit(16)
            .map(|d| d as u8)
            .ok_or_else(|| format!("expected hexadecimal digit, found `{}`", c)),
        None => Err("expected hexadecimal digit".to_string()),
    }
}
