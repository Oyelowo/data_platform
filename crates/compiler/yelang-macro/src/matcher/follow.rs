use super::types::{FragmentKind, MacroRule, MatcherError, MatcherOp, RepetitionKind};

/// A simplified token class used for FIRST/LAST/FOLLOW computations.
/// Spans and syntax contexts are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenClass {
    /// The matcher can match the empty fragment at this position.
    Epsilon,
    /// A literal terminal token.
    Terminal(SimpleToken),
    /// A metavariable fragment specifier.
    Fragment(FragmentKind),
    /// Any token is allowed (used for unrestricted fragments).
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SimpleToken {
    Ident(String),
    Punct(char),
    Literal,
    GroupOpen(yelang_macro_core::token_tree::Delimiter),
    GroupClose(yelang_macro_core::token_tree::Delimiter),
}

impl SimpleToken {
    fn from_token_tree(
        tree: &yelang_macro_core::token_tree::TokenTree,
        interner: &yelang_interner::Interner,
    ) -> Self {
        use yelang_macro_core::token_tree::TokenTree;
        match tree {
            TokenTree::Ident(i) => SimpleToken::Ident(interner.resolve(&i.sym).to_string()),
            TokenTree::Punct(p) => SimpleToken::Punct(p.ch),
            TokenTree::Literal(_) => SimpleToken::Literal,
            TokenTree::Group(g) => SimpleToken::GroupOpen(g.delimiter),
        }
    }
}

/// A small set of token classes. Implemented as a vector because the sets are
/// tiny and it avoids requiring `Ord`/`Hash` on external token types.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ClassSet {
    items: Vec<TokenClass>,
}

impl ClassSet {
    fn new() -> Self {
        Self { items: Vec::new() }
    }

    fn singleton(class: TokenClass) -> Self {
        let mut set = Self::new();
        set.insert(class);
        set
    }

    fn insert(&mut self, class: TokenClass) {
        if !self.items.contains(&class) {
            self.items.push(class);
        }
    }

    fn contains(&self, class: &TokenClass) -> bool {
        self.items.contains(class)
    }

    fn contains_any(&self) -> bool {
        self.items.contains(&TokenClass::Any)
    }

    fn extend(&mut self, other: impl IntoIterator<Item = TokenClass>) {
        for class in other {
            self.insert(class);
        }
    }

    fn with_epsilon(&self) -> Self {
        let mut result = self.clone();
        result.insert(TokenClass::Epsilon);
        result
    }

    fn without_epsilon(&self) -> Self {
        Self {
            items: self
                .items
                .iter()
                .filter(|c| **c != TokenClass::Epsilon)
                .cloned()
                .collect(),
        }
    }

    fn is_subset_of(&self, other: &Self) -> bool {
        if other.contains_any() {
            return true;
        }
        self.items.iter().all(|c| other.contains(c))
    }

    fn iter(&self) -> impl Iterator<Item = &TokenClass> {
        self.items.iter()
    }

    fn difference<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = &'a TokenClass> {
        self.items.iter().filter(move |c| !other.contains(c))
    }
}

/// Validate that a macro rule's matcher respects follow-set invariants.
pub fn validate_rule(
    rule: &MacroRule,
    interner: &yelang_interner::Interner,
) -> Result<(), MatcherError> {
    validate_ops(&rule.attr_args, interner)?;
    validate_ops(&rule.matcher, interner)
}

fn validate_ops(
    ops: &[MatcherOp],
    interner: &yelang_interner::Interner,
) -> Result<(), MatcherError> {
    // Invariant 1: successive token-tree sequences.
    for window in ops.windows(2) {
        let left = &window[0];
        let right = &window[1];
        let follow_left = follow_op(left, interner);
        let first_right = first_op(right, interner);
        if !first_right.is_subset_of(&follow_left.with_epsilon()) {
            let bad = first_right
                .difference(&follow_left.with_epsilon())
                .next()
                .cloned()
                .unwrap_or(TokenClass::Epsilon);
            return Err(MatcherError::FollowSetViolation {
                fragment: fragment_from_op(left).unwrap_or(FragmentKind::Tt),
                followed_by: describe_class(&bad, interner),
            });
        }
    }

    // Invariants 2 and 3: repetitions, plus recursion.
    for op in ops {
        match op {
            MatcherOp::Repeat {
                kind,
                sep,
                ops: inner,
            } => {
                let follow_inner = follow_ops(inner, interner);
                let first_inner = first_ops(inner, interner);

                let last_inner_fragment = inner.last().and_then(fragment_from_op);

                if let Some(sep_tree) = sep {
                    // Invariant 2: separator must be in FOLLOW(contents).
                    let sep_class =
                        TokenClass::Terminal(SimpleToken::from_token_tree(sep_tree, interner));
                    if !follow_inner.contains(&sep_class) && !follow_inner.contains_any() {
                        return Err(MatcherError::FollowSetViolation {
                            fragment: last_inner_fragment.unwrap_or(FragmentKind::Tt),
                            followed_by: describe_class(&sep_class, interner),
                        });
                    }
                } else if matches!(kind, RepetitionKind::ZeroOrMore | RepetitionKind::OneOrMore) {
                    // Invariant 3: unseparated * / + contents must be able to follow themselves.
                    let first_no_epsilon = first_inner.without_epsilon();
                    if !first_no_epsilon.is_subset_of(&follow_inner) {
                        let bad = first_no_epsilon
                            .difference(&follow_inner)
                            .next()
                            .cloned()
                            .unwrap_or(TokenClass::Epsilon);
                        return Err(MatcherError::FollowSetViolation {
                            fragment: last_inner_fragment.unwrap_or(FragmentKind::Tt),
                            followed_by: describe_class(&bad, interner),
                        });
                    }
                }

                validate_ops(inner, interner)?;
            }
            MatcherOp::Group { ops: inner, .. } => validate_ops(inner, interner)?,
            _ => {}
        }
    }

    Ok(())
}

fn first_op(op: &MatcherOp, interner: &yelang_interner::Interner) -> ClassSet {
    match op {
        MatcherOp::Terminal(tree) => ClassSet::singleton(TokenClass::Terminal(
            SimpleToken::from_token_tree(tree, interner),
        )),
        MatcherOp::Metavar { fragment, .. } => ClassSet::singleton(TokenClass::Fragment(*fragment)),
        MatcherOp::Group { delimiter, .. } => {
            ClassSet::singleton(TokenClass::Terminal(SimpleToken::GroupOpen(*delimiter)))
        }
        MatcherOp::Repeat {
            kind,
            sep,
            ops: inner,
        } => {
            let mut set = first_ops(inner, interner);
            if matches!(kind, RepetitionKind::ZeroOrMore | RepetitionKind::ZeroOrOne) {
                set.insert(TokenClass::Epsilon);
            }
            if let Some(sep_tree) = sep
                && set.contains(&TokenClass::Epsilon)
            {
                set.insert(TokenClass::Terminal(SimpleToken::from_token_tree(
                    sep_tree, interner,
                )));
            }
            set
        }
    }
}

fn first_ops(ops: &[MatcherOp], interner: &yelang_interner::Interner) -> ClassSet {
    let mut result = ClassSet::new();
    let mut all_epsilon = true;
    for op in ops {
        let first = first_op(op, interner);
        for class in first.iter().filter(|c| **c != TokenClass::Epsilon) {
            result.insert(class.clone());
        }
        if !first.contains(&TokenClass::Epsilon) {
            all_epsilon = false;
            break;
        }
    }
    if all_epsilon {
        result.insert(TokenClass::Epsilon);
    }
    result
}

fn last_op(op: &MatcherOp, interner: &yelang_interner::Interner) -> ClassSet {
    match op {
        MatcherOp::Terminal(tree) => ClassSet::singleton(TokenClass::Terminal(
            SimpleToken::from_token_tree(tree, interner),
        )),
        MatcherOp::Metavar { fragment, .. } => ClassSet::singleton(TokenClass::Fragment(*fragment)),
        MatcherOp::Group { delimiter, .. } => {
            ClassSet::singleton(TokenClass::Terminal(SimpleToken::GroupClose(*delimiter)))
        }
        MatcherOp::Repeat {
            kind,
            sep,
            ops: inner,
        } => {
            let mut set = last_ops(inner, interner);
            match kind {
                RepetitionKind::ZeroOrMore => {
                    if !set.contains(&TokenClass::Epsilon) {
                        set.insert(TokenClass::Epsilon);
                    }
                    if let Some(sep_tree) = sep {
                        set.insert(TokenClass::Terminal(SimpleToken::from_token_tree(
                            sep_tree, interner,
                        )));
                    }
                }
                RepetitionKind::OneOrMore => {
                    if let Some(sep_tree) = sep {
                        set.insert(TokenClass::Terminal(SimpleToken::from_token_tree(
                            sep_tree, interner,
                        )));
                    }
                }
                RepetitionKind::ZeroOrOne => {
                    set.insert(TokenClass::Epsilon);
                }
            }
            set
        }
    }
}

fn last_ops(ops: &[MatcherOp], interner: &yelang_interner::Interner) -> ClassSet {
    let mut result = ClassSet::new();
    let mut all_epsilon = true;
    for op in ops.iter().rev() {
        let last = last_op(op, interner);
        for class in last.iter().filter(|c| **c != TokenClass::Epsilon) {
            result.insert(class.clone());
        }
        if !last.contains(&TokenClass::Epsilon) {
            all_epsilon = false;
            break;
        }
    }
    if all_epsilon {
        result.insert(TokenClass::Epsilon);
    }
    result
}

fn follow_op(op: &MatcherOp, interner: &yelang_interner::Interner) -> ClassSet {
    follow_ops(std::slice::from_ref(op), interner)
}

fn follow_ops(ops: &[MatcherOp], interner: &yelang_interner::Interner) -> ClassSet {
    let last = last_ops(ops, interner);
    let mut result = ClassSet::new();
    let mut has_non_epsilon = false;
    for class in last.iter() {
        if *class == TokenClass::Epsilon {
            continue;
        }
        has_non_epsilon = true;
        result.extend(follow_class(class));
    }
    if !has_non_epsilon {
        result.insert(TokenClass::Epsilon);
    }
    result
}

fn follow_class(class: &TokenClass) -> impl Iterator<Item = TokenClass> + use<'_> {
    let mut vec = Vec::new();
    match class {
        TokenClass::Fragment(fragment) => {
            if let Some(tokens) = fragment_follow_set(*fragment) {
                for token in tokens {
                    vec.push(TokenClass::Terminal(token));
                }
            } else {
                vec.push(TokenClass::Any);
            }
        }
        // A literal terminal token imposes no follow restriction: any token may
        // follow it.  This matches the Rust Reference treatment of terminals in
        // the FIRST/LAST/FOLLOW invariants.
        TokenClass::Terminal(_) | TokenClass::Any => {
            vec.push(TokenClass::Any);
        }
        TokenClass::Epsilon => {
            vec.push(TokenClass::Epsilon);
        }
    }
    vec.into_iter()
}

fn fragment_from_op(op: &MatcherOp) -> Option<FragmentKind> {
    match op {
        MatcherOp::Metavar { fragment, .. } => Some(*fragment),
        MatcherOp::Group { ops, .. } | MatcherOp::Repeat { ops, .. } => {
            ops.last().and_then(fragment_from_op)
        }
        MatcherOp::Terminal(_) => None,
    }
}

fn describe_class(class: &TokenClass, _interner: &yelang_interner::Interner) -> String {
    match class {
        TokenClass::Epsilon => "end of matcher".to_string(),
        TokenClass::Fragment(f) => format!("`{:?}` fragment", f),
        TokenClass::Any => "any token".to_string(),
        TokenClass::Terminal(t) => match t {
            SimpleToken::Ident(s) => format!("`{}`", s),
            SimpleToken::Punct(ch) => format!("`{}`", ch),
            SimpleToken::Literal => "literal".to_string(),
            SimpleToken::GroupOpen(d) => format!("opening `{:?}`", d),
            SimpleToken::GroupClose(d) => format!("closing `{:?}`", d),
        },
    }
}

/// Hard-coded follow sets based on the Rust Reference, modern (non-legacy) edition.
/// `None` means the fragment has no restrictions (ANYTOKEN).
fn fragment_follow_set(fragment: FragmentKind) -> Option<Vec<SimpleToken>> {
    use yelang_macro_core::token_tree::Delimiter;
    match fragment {
        FragmentKind::Expr | FragmentKind::Stmt => Some(vec![
            SimpleToken::Punct('='), // approximates `=>`
            SimpleToken::Punct(','),
            SimpleToken::Punct(';'),
        ]),
        // Modern Rust 2021+ `pat` follow set: `|` is excluded because or-patterns
        // are part of the `pat` nonterminal itself.
        FragmentKind::Pat => Some(vec![
            SimpleToken::Punct('='),
            SimpleToken::Punct(','),
            SimpleToken::Ident("if".to_string()),
            SimpleToken::Ident("in".to_string()),
        ]),
        FragmentKind::Ty | FragmentKind::Path => Some(vec![
            SimpleToken::GroupOpen(Delimiter::Brace),
            SimpleToken::GroupOpen(Delimiter::Bracket),
            SimpleToken::Punct(','),
            SimpleToken::Punct('='),
            SimpleToken::Punct('|'),
            SimpleToken::Punct(';'),
            SimpleToken::Punct(':'),
            SimpleToken::Punct('>'),
            SimpleToken::Ident("as".to_string()),
            SimpleToken::Ident("where".to_string()),
            // `block` nonterminals are also valid follows; we approximate by
            // allowing any group opener (brace already included above).
        ]),
        // block, ident, tt, item, literal have no restrictions.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{MacroKind, TranscriberOp};
    use super::*;
    use yelang_interner::Interner;
    use yelang_macro_core::token_tree::{Punct, Spacing, Span, TokenTree};

    fn punct(ch: char) -> TokenTree {
        TokenTree::Punct(Punct::new(ch, Spacing::Alone, Span::default()))
    }

    fn rule_with_matcher(matcher: Vec<MatcherOp>) -> MacroRule {
        MacroRule {
            kind: MacroKind::FunctionLike,
            is_unsafe: false,
            attr_args: vec![],
            matcher,
            transcriber: vec![TranscriberOp::Subst(Interner::new().get_or_intern("x"))],
        }
    }

    #[test]
    fn expr_followed_by_comma_is_valid() {
        let interner = Interner::new();
        let rule = rule_with_matcher(vec![
            MatcherOp::Metavar {
                name: interner.get_or_intern("x"),
                fragment: FragmentKind::Expr,
            },
            MatcherOp::Terminal(punct(',')),
        ]);
        assert!(validate_rule(&rule, &interner).is_ok());
    }

    #[test]
    fn expr_followed_by_lbracket_is_invalid() {
        let interner = Interner::new();
        let rule = rule_with_matcher(vec![
            MatcherOp::Metavar {
                name: interner.get_or_intern("x"),
                fragment: FragmentKind::Expr,
            },
            MatcherOp::Terminal(punct('[')),
        ]);
        assert!(matches!(
            validate_rule(&rule, &interner),
            Err(MatcherError::FollowSetViolation { .. })
        ));
    }

    #[test]
    fn terminal_has_no_follow_restriction() {
        let interner = Interner::new();
        // A literal terminal `]` may be followed by anything, including another
        // `]` which would normally be illegal after an `expr` fragment.
        let rule = rule_with_matcher(vec![
            MatcherOp::Terminal(punct(']')),
            MatcherOp::Terminal(punct(']')),
        ]);
        assert!(validate_rule(&rule, &interner).is_ok());
    }

    #[test]
    fn pat_follow_excludes_pipe() {
        let interner = Interner::new();
        let rule = rule_with_matcher(vec![
            MatcherOp::Metavar {
                name: interner.get_or_intern("p"),
                fragment: FragmentKind::Pat,
            },
            MatcherOp::Terminal(punct('|')),
        ]);
        assert!(matches!(
            validate_rule(&rule, &interner),
            Err(MatcherError::FollowSetViolation { .. })
        ));
    }

    #[test]
    fn pat_follow_allows_comma() {
        let interner = Interner::new();
        let rule = rule_with_matcher(vec![
            MatcherOp::Metavar {
                name: interner.get_or_intern("p"),
                fragment: FragmentKind::Pat,
            },
            MatcherOp::Terminal(punct(',')),
        ]);
        assert!(validate_rule(&rule, &interner).is_ok());
    }
}
