//! Analyzer pipeline: tokenize, lowercase, stop-word filter, stem.

use crate::schema::FieldOptions;
use crate::stemmer::stem;
use crate::tokenizer::{Token, remove_stop_words, tokenize};

/// Analyze `text` according to field options.
///
/// If `tokenize` is false, the whole text is treated as a single token.
/// If `stem` is true, tokens are passed through the Porter stemmer.
/// If `with_positions` is true, token positions are preserved.
pub fn analyze(text: &str, options: &FieldOptions) -> Vec<Token> {
    if !options.indexed {
        return Vec::new();
    }

    if options.tokenize {
        let tokens = remove_stop_words(tokenize(text));
        if options.stem {
            tokens
                .into_iter()
                .map(|mut t| {
                    t.text = stem(&t.text);
                    t
                })
                .collect()
        } else {
            tokens
        }
    } else {
        vec![Token {
            text: text.to_lowercase(),
            position: 0,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::FieldOptions;

    #[test]
    fn analyze_text_field() {
        let opts = FieldOptions::text();
        let tokens = analyze("Running quickly through the fields", &opts);
        assert!(tokens.iter().any(|t| t.text == "run"));
        assert!(!tokens.iter().any(|t| t.text == "the"));
    }

    #[test]
    fn analyze_keyword_field() {
        let opts = FieldOptions::keyword();
        let tokens = analyze("User-ID-123", &opts);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].text, "user-id-123");
    }
}
