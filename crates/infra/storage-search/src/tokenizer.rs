//! Unicode word tokenizer.

use unicode_segmentation::UnicodeSegmentation;

/// A token produced by the tokenizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Token text, lowercased.
    pub text: String,
    /// Position of the token in the source text (zero-based word index).
    pub position: u32,
}

/// Tokenize `text` into lowercase word tokens.
///
/// Punctuation and whitespace are skipped. Positions count only word tokens.
pub fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut position = 0u32;
    for word in text.unicode_words() {
        let text: String = word
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect();
        if text.is_empty() {
            continue;
        }
        tokens.push(Token { text, position });
        position += 1;
    }
    tokens
}

/// Return true if `token` is a stop word.
pub fn is_stop_word(token: &str) -> bool {
    matches!(
        token,
        "a"
            | "an"
            | "and"
            | "are"
            | "as"
            | "at"
            | "be"
            | "by"
            | "for"
            | "from"
            | "has"
            | "he"
            | "in"
            | "is"
            | "it"
            | "its"
            | "of"
            | "on"
            | "that"
            | "the"
            | "to"
            | "was"
            | "will"
            | "with"
    )
}

/// Filter stop words from a token stream.
pub fn remove_stop_words(tokens: Vec<Token>) -> Vec<Token> {
    tokens.into_iter().filter(|t| !is_stop_word(&t.text)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_tokenization() {
        let tokens = tokenize("Hello, world!");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].text, "hello");
        assert_eq!(tokens[0].position, 0);
        assert_eq!(tokens[1].text, "world");
        assert_eq!(tokens[1].position, 1);
    }

    #[test]
    fn stop_word_filtering() {
        let tokens = tokenize("The quick brown fox");
        let filtered = remove_stop_words(tokens);
        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].text, "quick");
    }
}
