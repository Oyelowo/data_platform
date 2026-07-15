use yelang_interner::Interner;
use yelang_macro_core::{
    CrateId, SyntaxContextId,
    token_tree::{Group, Ident, IdentOrigin, Literal, Punct, Spacing, TokenStream, TokenTree},
};

use crate::matcher::types::{MetavarExpr, RepetitionKind, TranscriberOp};
use crate::matcher::{Binding, Bindings};

/// Transcribe a macro rule's transcriber ops with the captured bindings.
///
/// `generated_ctx` is applied to every token that originates from the macro
/// definition body (terminals and implicit punctuation). Captured argument
/// tokens keep their original hygiene context.
///
/// `defining_crate` is the crate in which the macro being expanded was defined;
/// it is used to resolve `$crate` tokens in the transcriber.
pub fn transcribe(
    ops: &[TranscriberOp],
    bindings: &Bindings,
    interner: &Interner,
    generated_ctx: SyntaxContextId,
    defining_crate: CrateId,
) -> Result<TokenStream, String> {
    let mut env = Vec::new();
    env.push(bindings.clone());
    let mut repeat_stack = Vec::new();
    transcribe_ops(
        ops,
        &mut env,
        &mut repeat_stack,
        interner,
        generated_ctx,
        defining_crate,
    )
}

struct RepeatFrame {
    index: usize,
    len: usize,
}

fn transcribe_ops(
    ops: &[TranscriberOp],
    env: &mut Vec<Bindings>,
    repeat_stack: &mut Vec<RepeatFrame>,
    interner: &Interner,
    generated_ctx: SyntaxContextId,
    defining_crate: CrateId,
) -> Result<TokenStream, String> {
    let mut out = TokenStream::new();
    for op in ops {
        out.extend(transcribe_op(
            op,
            env,
            repeat_stack,
            interner,
            generated_ctx,
            defining_crate,
        )?);
    }
    Ok(out)
}

fn transcribe_op(
    op: &TranscriberOp,
    env: &mut Vec<Bindings>,
    repeat_stack: &mut Vec<RepeatFrame>,
    interner: &Interner,
    generated_ctx: SyntaxContextId,
    defining_crate: CrateId,
) -> Result<TokenStream, String> {
    match op {
        TranscriberOp::Terminal(tree) => {
            let mut tree = tree.clone();
            apply_ctx_to_tree(&mut tree, generated_ctx);
            if let TokenTree::Ident(ident) = &mut tree {
                resolve_crate_origin(ident, defining_crate);
            }
            Ok(TokenStream::from_vec(vec![tree]))
        }
        TranscriberOp::Group { delimiter, ops } => {
            let inner = transcribe_ops(
                ops,
                env,
                repeat_stack,
                interner,
                generated_ctx,
                defining_crate,
            )?;
            let mut group = TokenTree::Group(Group::new(*delimiter, inner, Default::default()));
            apply_ctx_to_tree(&mut group, generated_ctx);
            Ok(TokenStream::from_vec(vec![group]))
        }
        TranscriberOp::Subst(name) => {
            let binding = lookup_binding(env, *name).ok_or_else(|| {
                format!("unknown metavariable `{}`", resolve_name(interner, *name))
            })?;
            let stream = binding.expect_single(&resolve_name(interner, *name))?;
            Ok(stream)
        }
        TranscriberOp::MetavarExpr(expr) => evaluate_metavar_expr(
            expr,
            repeat_stack,
            env,
            interner,
            generated_ctx,
            defining_crate,
        ),
        TranscriberOp::DollarDollar => Ok(TokenStream::from_vec(vec![TokenTree::Punct(
            Punct::new('$', Spacing::Alone, Default::default()),
        )])),
        TranscriberOp::Repeat { kind, sep, ops } => {
            let names = referenced_names(ops);
            let count = resolve_repeat_count(*kind, &names, env, interner)?;

            let mut out = TokenStream::new();
            for i in 0..count {
                let mut frame = Bindings::new();
                for name in &names {
                    if let Some(binding) = lookup_binding(env, *name) {
                        let iter_binding = match binding {
                            Binding::Repeat(list) => list.get(i).cloned(),
                            Binding::Single(_) if i == 0 => Some(binding.clone()),
                            Binding::Single(_) => None,
                        };
                        if let Some(b) = iter_binding {
                            frame.insert(*name, b);
                        }
                    }
                }
                env.push(frame);
                repeat_stack.push(RepeatFrame {
                    index: i,
                    len: count,
                });
                let part = transcribe_ops(
                    ops,
                    env,
                    repeat_stack,
                    interner,
                    generated_ctx,
                    defining_crate,
                )?;
                repeat_stack.pop();
                env.pop();
                out.extend(part);

                if i + 1 < count {
                    if let Some(sep_tree) = sep {
                        let mut sep = sep_tree.clone();
                        apply_ctx_to_tree(&mut sep, generated_ctx);
                        out.push(sep);
                    }
                }
            }
            Ok(out)
        }
    }
}

/// For `$crate` / `$package` identifiers, fill in the defining crate reference
/// so name resolution can route the path correctly.
fn resolve_crate_origin(ident: &mut Ident, defining_crate: CrateId) {
    match ident.origin {
        IdentOrigin::Crate if ident.crate_ref.is_none() => {
            ident.crate_ref = Some(defining_crate);
        }
        _ => {}
    }
}

fn evaluate_metavar_expr(
    expr: &MetavarExpr,
    repeat_stack: &[RepeatFrame],
    env: &[Bindings],
    interner: &Interner,
    generated_ctx: SyntaxContextId,
    _defining_crate: CrateId,
) -> Result<TokenStream, String> {
    match expr {
        MetavarExpr::Count { name, depth } => {
            let binding = lookup_binding(env, *name).ok_or_else(|| {
                format!(
                    "metavariable expression `${{count({})}}` refers to unknown metavariable",
                    resolve_name(interner, *name)
                )
            })?;
            let value = match depth {
                Some(d) => count_at_depth(binding, *d),
                None => count_total(binding),
            };
            Ok(int_literal_stream(value, interner, generated_ctx))
        }
        MetavarExpr::Index { depth } => {
            let frame = frame_at_depth(repeat_stack, depth.unwrap_or(0))?;
            Ok(int_literal_stream(frame.index, interner, generated_ctx))
        }
        MetavarExpr::Len { depth } => {
            let frame = frame_at_depth(repeat_stack, depth.unwrap_or(0))?;
            Ok(int_literal_stream(frame.len, interner, generated_ctx))
        }
        MetavarExpr::Ignore { .. } => Ok(TokenStream::new()),
    }
}

fn frame_at_depth(repeat_stack: &[RepeatFrame], depth: usize) -> Result<&RepeatFrame, String> {
    if repeat_stack.is_empty() {
        return Err(
            "metavariable expression requires a repetition context, but none is active".to_string(),
        );
    }
    repeat_stack
        .iter()
        .rev()
        .nth(depth)
        .ok_or_else(|| format!("repetition depth {} is out of range", depth))
}

fn count_total(binding: &Binding) -> usize {
    match binding {
        Binding::Single(_) => 1,
        Binding::Repeat(list) => list.iter().map(count_total).sum(),
    }
}

fn count_at_depth(binding: &Binding, depth: usize) -> usize {
    if depth == 0 {
        match binding {
            Binding::Single(_) => 1,
            Binding::Repeat(list) => list.len(),
        }
    } else {
        match binding {
            Binding::Single(_) => 0,
            Binding::Repeat(list) => list.iter().map(|b| count_at_depth(b, depth - 1)).sum(),
        }
    }
}

fn int_literal_stream(value: usize, interner: &Interner, ctx: SyntaxContextId) -> TokenStream {
    let symbol = interner.get_or_intern(&value.to_string());
    let mut lit = TokenTree::Literal(Literal::int(symbol, Default::default()));
    apply_ctx_to_tree(&mut lit, ctx);
    TokenStream::from_vec(vec![lit])
}

fn lookup_binding(env: &[Bindings], name: yelang_interner::Symbol) -> Option<&Binding> {
    env.iter().rev().find_map(|frame| frame.get(name))
}

/// Determine how many times a transcriber repetition should be emitted.
///
/// For `*`/`+` every referenced metavariable must be a `Binding::Repeat`; the
/// counts must all match.  For `?` the binding may be absent (zero iterations),
/// a single capture (one iteration), or a repeat of length 0/1.
fn resolve_repeat_count(
    kind: RepetitionKind,
    names: &[yelang_interner::Symbol],
    env: &[Bindings],
    interner: &Interner,
) -> Result<usize, String> {
    match kind {
        RepetitionKind::ZeroOrOne => {
            if names.is_empty() {
                // No metavariables: the optional body either matched once or not
                // at all; with no signal we conservatively emit nothing.
                return Ok(0);
            }

            let mut counts = Vec::new();
            for name in names {
                match lookup_binding(env, *name) {
                    None => counts.push(0),
                    Some(Binding::Single(_)) => counts.push(1),
                    Some(Binding::Repeat(list)) => {
                        if list.len() > 1 {
                            return Err(format!(
                                "metavariable `{}` used with `?` is repeated {} times",
                                resolve_name(interner, *name),
                                list.len()
                            ));
                        }
                        counts.push(list.len());
                    }
                }
            }

            let count = counts[0];
            if counts.iter().any(|&c| c != count) {
                return Err(
                    "repetition counts of referenced metavariables do not match".to_string()
                );
            }
            Ok(count)
        }
        RepetitionKind::ZeroOrMore | RepetitionKind::OneOrMore => {
            if names.is_empty() {
                return Err(format!(
                    "`{}` repetition does not reference any repeated metavariable",
                    if matches!(kind, RepetitionKind::ZeroOrMore) {
                        '*'
                    } else {
                        '+'
                    }
                ));
            }

            let mut counts = Vec::new();
            for name in names {
                match lookup_binding(env, *name) {
                    None | Some(Binding::Single(_)) => {
                        return Err(format!(
                            "metavariable `{}` used with `{}` is not repeated",
                            resolve_name(interner, *name),
                            if matches!(kind, RepetitionKind::ZeroOrMore) {
                                '*'
                            } else {
                                '+'
                            }
                        ));
                    }
                    Some(Binding::Repeat(list)) => counts.push(list.len()),
                }
            }

            let count = counts[0];
            if counts.iter().any(|&c| c != count) {
                return Err(
                    "repetition counts of referenced metavariables do not match".to_string()
                );
            }

            if matches!(kind, RepetitionKind::OneOrMore) && count == 0 {
                return Err("`+` repetition matched zero times".to_string());
            }

            Ok(count)
        }
    }
}

fn resolve_name(interner: &Interner, sym: yelang_interner::Symbol) -> String {
    interner.resolve(&sym).to_string()
}

fn referenced_names(ops: &[TranscriberOp]) -> Vec<yelang_interner::Symbol> {
    let mut names = Vec::new();
    for op in ops {
        match op {
            TranscriberOp::Subst(name) => names.push(*name),
            TranscriberOp::Group { ops, .. } | TranscriberOp::Repeat { ops, .. } => {
                names.extend(referenced_names(ops));
            }
            TranscriberOp::MetavarExpr(expr) => {
                if let MetavarExpr::Count { name, .. } | MetavarExpr::Ignore { name } = expr {
                    names.push(*name);
                }
            }
            TranscriberOp::Terminal(_) | TranscriberOp::DollarDollar => {}
        }
    }
    names.sort();
    names.dedup();
    names
}

fn apply_ctx_to_tree(tree: &mut TokenTree, ctx: SyntaxContextId) {
    match tree {
        TokenTree::Group(g) => {
            g.span = g.span.with_ctx(ctx);
            apply_ctx_to_stream(&mut g.stream, ctx);
        }
        TokenTree::Ident(i) => i.span = i.span.with_ctx(ctx),
        TokenTree::Punct(p) => p.span = p.span.with_ctx(ctx),
        TokenTree::Literal(l) => l.span = l.span.with_ctx(ctx),
    }
}

fn apply_ctx_to_stream(stream: &mut TokenStream, ctx: SyntaxContextId) {
    for tree in stream.trees_mut() {
        apply_ctx_to_tree(tree, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::types::RepetitionKind;
    use yelang_interner::Interner;
    use yelang_macro_core::token_tree::{Ident, Punct, Spacing, Span, TokenTree};

    fn single_ident_binding(_name: &str, value: &str, interner: &Interner) -> Binding {
        Binding::Single(TokenStream::from_vec(vec![TokenTree::Ident(Ident::new(
            interner.get_or_intern(value),
            Span::default(),
        ))]))
    }

    #[test]
    fn substitute_single_capture() {
        let interner = Interner::new();
        let mut bindings = Bindings::new();
        bindings.insert(
            interner.get_or_intern("x"),
            single_ident_binding("x", "foo", &interner),
        );
        let ops = vec![TranscriberOp::Subst(interner.get_or_intern("x"))];
        let out = transcribe(
            &ops,
            &bindings,
            &interner,
            SyntaxContextId::default(),
            CrateId::new(1),
        )
        .unwrap();
        assert_eq!(out.render(&interner), "foo");
    }

    #[test]
    fn transcribe_unknown_metavariable_errors() {
        let interner = Interner::new();
        let bindings = Bindings::new();
        let ops = vec![TranscriberOp::Subst(interner.get_or_intern("x"))];
        assert!(
            transcribe(
                &ops,
                &bindings,
                &interner,
                SyntaxContextId::default(),
                CrateId::new(1)
            )
            .is_err()
        );
    }

    #[test]
    fn transcribe_dollar_dollar_escapes_to_literal_dollar() {
        let interner = Interner::new();
        let bindings = Bindings::new();
        let ops = vec![TranscriberOp::DollarDollar];
        let out = transcribe(
            &ops,
            &bindings,
            &interner,
            SyntaxContextId::default(),
            CrateId::new(1),
        )
        .unwrap();
        let trees: Vec<_> = out.into_iter().collect();
        assert_eq!(trees.len(), 1);
        assert!(matches!(
            trees[0],
            TokenTree::Punct(Punct {
                ch: '$',
                spacing: Spacing::Alone,
                ..
            })
        ));
    }

    #[test]
    fn transcribe_group_wrapper() {
        let interner = Interner::new();
        let mut bindings = Bindings::new();
        bindings.insert(
            interner.get_or_intern("x"),
            single_ident_binding("x", "42", &interner),
        );
        let ops = vec![TranscriberOp::Group {
            delimiter: yelang_macro_core::token_tree::Delimiter::Parenthesis,
            ops: vec![TranscriberOp::Subst(interner.get_or_intern("x"))],
        }];
        let out = transcribe(
            &ops,
            &bindings,
            &interner,
            SyntaxContextId::default(),
            CrateId::new(1),
        )
        .unwrap();
        assert_eq!(out.render(&interner), "(42)");
    }

    #[test]
    fn transcribe_star_repetition() {
        let interner = Interner::new();
        let mut bindings = Bindings::new();
        let repeated: Vec<Binding> = vec![
            single_ident_binding("x", "1", &interner),
            single_ident_binding("x", "2", &interner),
        ];
        bindings.insert(interner.get_or_intern("x"), Binding::Repeat(repeated));
        let ops = vec![TranscriberOp::Repeat {
            kind: RepetitionKind::ZeroOrMore,
            sep: Some(TokenTree::Punct(Punct::new('+', Spacing::Alone, Span::default())).clone()),
            ops: vec![TranscriberOp::Subst(interner.get_or_intern("x"))],
        }];
        let out = transcribe(
            &ops,
            &bindings,
            &interner,
            SyntaxContextId::default(),
            CrateId::new(1),
        )
        .unwrap();
        assert_eq!(out.render(&interner), "1+2");
    }

    #[test]
    fn transcribe_optional_present() {
        let interner = Interner::new();
        let mut bindings = Bindings::new();
        bindings.insert(
            interner.get_or_intern("y"),
            single_ident_binding("y", "2", &interner),
        );
        let ops = vec![TranscriberOp::Repeat {
            kind: RepetitionKind::ZeroOrOne,
            sep: None,
            ops: vec![TranscriberOp::Subst(interner.get_or_intern("y"))],
        }];
        let out = transcribe(
            &ops,
            &bindings,
            &interner,
            SyntaxContextId::default(),
            CrateId::new(1),
        )
        .unwrap();
        assert_eq!(out.render(&interner), "2");
    }

    #[test]
    fn transcribe_optional_absent() {
        let interner = Interner::new();
        let bindings = Bindings::new();
        let ops = vec![TranscriberOp::Repeat {
            kind: RepetitionKind::ZeroOrOne,
            sep: None,
            ops: vec![TranscriberOp::Subst(interner.get_or_intern("y"))],
        }];
        let out = transcribe(
            &ops,
            &bindings,
            &interner,
            SyntaxContextId::default(),
            CrateId::new(1),
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn transcribe_count_total_repetitions() {
        let interner = Interner::new();
        let mut bindings = Bindings::new();
        let repeated: Vec<Binding> = vec![
            single_ident_binding("x", "a", &interner),
            single_ident_binding("x", "b", &interner),
            single_ident_binding("x", "c", &interner),
        ];
        bindings.insert(interner.get_or_intern("x"), Binding::Repeat(repeated));
        let ops = vec![TranscriberOp::MetavarExpr(MetavarExpr::Count {
            name: interner.get_or_intern("x"),
            depth: None,
        })];
        let out = transcribe(
            &ops,
            &bindings,
            &interner,
            SyntaxContextId::default(),
            CrateId::new(1),
        )
        .unwrap();
        assert_eq!(out.render(&interner), "3");
    }

    #[test]
    fn transcribe_tuple_group() {
        let interner = Interner::new();
        let mut bindings = Bindings::new();
        bindings.insert(
            interner.get_or_intern("x"),
            single_ident_binding("x", "42", &interner),
        );
        let ops = vec![TranscriberOp::Group {
            delimiter: yelang_macro_core::token_tree::Delimiter::Parenthesis,
            ops: vec![
                TranscriberOp::Subst(interner.get_or_intern("x")),
                TranscriberOp::Terminal(TokenTree::Punct(Punct::new(
                    ',',
                    Spacing::Alone,
                    Span::default(),
                ))),
                TranscriberOp::Subst(interner.get_or_intern("x")),
            ],
        }];
        let out = transcribe(
            &ops,
            &bindings,
            &interner,
            SyntaxContextId::default(),
            CrateId::new(1),
        )
        .unwrap();
        assert_eq!(out.render(&interner), "(42, 42)");
    }

    #[test]
    fn transcribe_plus_requires_non_empty() {
        let interner = Interner::new();
        let bindings = Bindings::new();
        let ops = vec![TranscriberOp::Repeat {
            kind: RepetitionKind::OneOrMore,
            sep: None,
            ops: vec![TranscriberOp::Subst(interner.get_or_intern("x"))],
        }];
        assert!(
            transcribe(
                &ops,
                &bindings,
                &interner,
                SyntaxContextId::default(),
                CrateId::new(1)
            )
            .is_err()
        );
    }
}
