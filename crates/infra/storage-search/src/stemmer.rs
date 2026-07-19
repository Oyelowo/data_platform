//! English Porter stemmer (minimal inline implementation).

/// Stem an English word.
///
/// This is a simplified Porter-style stemmer sufficient for the search
/// engine's first release. It normalizes common suffixes and is not
/// linguistically exhaustive.
pub fn stem(word: &str) -> String {
    if word.len() <= 2 {
        return word.to_lowercase();
    }

    let mut s = word.to_lowercase();

    // Step 1a: plural forms.
    if s.ends_with("ies") && s.len() > 4 {
        s.truncate(s.len() - 3);
        s.push('y');
    } else if s.ends_with("sses") || s.ends_with("ss") {
        // leave as-is.
    } else if s.ends_with('s') && s.len() > 2 && !s.ends_with("us") && !s.ends_with("is") {
        s.pop();
    }

    // Step 1b: -ed, -ing.
    let mut step1b_applied = false;
    if s.ends_with("eed") && measure(&s[..s.len() - 3]) > 0 {
        s.truncate(s.len() - 1);
    } else if s.ends_with("ed") && has_vowel(&s[..s.len() - 2]) {
        s.truncate(s.len() - 2);
        step1b_applied = true;
    } else if s.ends_with("ing") && has_vowel(&s[..s.len() - 3]) {
        s.truncate(s.len() - 3);
        step1b_applied = true;
    }

    if step1b_applied {
        if s.ends_with("at") || s.ends_with("bl") || s.ends_with("iz") {
            s.push('e');
        } else if ends_double_consonant(&s)
            && !s.ends_with('l')
            && !s.ends_with('s')
            && !s.ends_with('z')
        {
            s.pop();
        } else if measure(&s) == 1 && ends_cvc(&s) {
            s.push('e');
        }
    }

    // Step 1c: y -> i when preceded by a consonant.
    if s.ends_with('y') && s.len() > 2 {
        let head = &s[..s.len() - 1];
        if head.ends_with(|c: char| is_consonant(c)) {
            s.pop();
            s.push('i');
        }
    }

    // Step 2: common suffixes.
    let replacements = [
        ("ational", "ate"),
        ("tional", "tion"),
        ("enci", "ence"),
        ("anci", "ance"),
        ("izer", "ize"),
        ("abli", "able"),
        ("alli", "al"),
        ("entli", "ent"),
        ("eli", "e"),
        ("ousli", "ous"),
        ("ization", "ize"),
        ("ation", "ate"),
        ("ator", "ate"),
        ("alism", "al"),
        ("iveness", "ive"),
        ("fulness", "ful"),
        ("ousness", "ous"),
        ("aliti", "al"),
        ("iviti", "ive"),
        ("biliti", "ble"),
    ];
    for (suffix, replacement) in &replacements {
        if s.ends_with(suffix) && measure(&s[..s.len() - suffix.len()]) > 0 {
            let len = s.len() - suffix.len();
            s.truncate(len);
            s.push_str(replacement);
            break;
        }
    }

    // Step 3: more suffixes.
    let step3 = [
        ("icate", "ic"),
        ("ative", ""),
        ("alize", "al"),
        ("iciti", "ic"),
        ("ical", "ic"),
        ("ful", ""),
        ("ness", ""),
    ];
    for (suffix, replacement) in &step3 {
        if s.ends_with(suffix) && measure(&s[..s.len() - suffix.len()]) > 0 {
            let len = s.len() - suffix.len();
            s.truncate(len);
            s.push_str(replacement);
            break;
        }
    }

    // Step 4: removal suffixes.
    let step4 = [
        "al", "ance", "ence", "er", "ic", "able", "ible", "ant", "ement", "ment", "ent",
        "ion", "ou", "ism", "ate", "iti", "ous", "ive", "ize",
    ];
    for suffix in &step4 {
        if s.ends_with(suffix) && measure(&s[..s.len() - suffix.len()]) > 1 {
            let len = s.len() - suffix.len();
            s.truncate(len);
            break;
        }
    }

    // Step 5a: trailing e.
    if s.ends_with('e') {
        let m = measure(&s[..s.len() - 1]);
        if m > 1 || (m == 1 && !ends_cvc(&s[..s.len() - 1])) {
            s.pop();
        }
    }

    // Step 5b: double l.
    if s.ends_with("ll") && measure(&s) > 1 {
        s.pop();
    }

    s
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

fn is_consonant(c: char) -> bool {
    c.is_alphabetic() && !is_vowel(c)
}

fn has_vowel(s: &str) -> bool {
    s.chars().any(is_vowel)
}

fn measure(s: &str) -> usize {
    let mut count = 0;
    let mut prev_vowel = false;
    for c in s.chars() {
        let vowel = is_vowel(c);
        if !prev_vowel && vowel {
            count += 1;
        }
        prev_vowel = vowel;
    }
    count
}

fn ends_double_consonant(s: &str) -> bool {
    if s.len() < 2 {
        return false;
    }
    let mut chars = s.chars().rev();
    let c1 = chars.next();
    let c2 = chars.next();
    match (c1, c2) {
        (Some(a), Some(b)) => a == b && is_consonant(a),
        _ => false,
    }
}

fn ends_cvc(s: &str) -> bool {
    if s.len() < 3 {
        return false;
    }
    let mut chars = s.chars().rev();
    let c3 = chars.next();
    let c2 = chars.next();
    let c1 = chars.next();
    match (c1, c2, c3) {
        (Some(a), Some(b), Some(c)) => {
            is_consonant(a) && is_vowel(b) && is_consonant(c) && c != 'w' && c != 'x' && c != 'y'
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_stemming() {
        assert_eq!(stem("running"), "run");
        assert_eq!(stem("flies"), "fli");
        assert_eq!(stem("tokenization"), "token");
        assert_eq!(stem("national"), "nation");
        assert_eq!(stem("relational"), "relat");
    }
}
