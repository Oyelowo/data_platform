//! Serialization and reconstruction of macro hygiene data for the proc-macro
//! server boundary.
//!
//! The compiler and the out-of-process proc-macro server do not share the same
//! `HygieneData` arena. Instead, every expansion request carries a
//! `WireHygienePayload` that describes the syntax contexts and expansion data
//! referenced by the input tokens. The server copies the raw context IDs onto
//! the public `Span`s, then returns a payload describing the contexts that
//! actually appear in the macro output. This module builds the outbound payload
//! and merges a returned payload back into the compiler-side arena.

use std::collections::{HashMap, HashSet};

use yelang_macro_core::{
    ExpnData, ExpnId, ExpnKind, HygieneData, SyntaxContextData, SyntaxContextId, Transparency,
    token_tree::{TokenStream, TokenTree},
};
use yelang_proc_macro_bridge::protocol::token::{
    WireExpnData, WireExpnKind, WireHygienePayload, WireSpan, WireSyntaxContext, WireTransparency,
};

/// Build a wire hygiene payload describing every syntax context reachable from
/// `stream` plus any additional `spans` supplied by the caller.
pub fn payload_from_stream_with_spans(
    stream: &TokenStream,
    spans: &[yelang_lexer::Span],
    hygiene: &HygieneData,
) -> WireHygienePayload {
    let mut used = HashSet::new();
    collect_contexts_from_stream(stream, &mut used);
    for span in spans {
        used.insert(SyntaxContextId::new(span.syntax_context()));
    }

    let mut contexts = Vec::new();
    let mut expansions = Vec::new();
    let mut seen_ctx = HashSet::new();
    let mut seen_expn = HashSet::new();

    for ctx in used {
        add_context(
            ctx,
            hygiene,
            &mut contexts,
            &mut expansions,
            &mut seen_ctx,
            &mut seen_expn,
        );
    }

    WireHygienePayload {
        contexts,
        expansions,
    }
}

/// Merge a hygiene payload returned by the proc-macro server into the compiler's
/// local `HygieneData`.
///
/// Context IDs are raw `u32`s, so this is a pure ID-level merge: existing
/// entries are overwritten with the returned data. Expansions are inserted into
/// the arena and their parent links are patched in a second pass so that the
/// opaque `ArenaKey`s line up correctly.
pub fn merge_payload(hygiene: &HygieneData, payload: &WireHygienePayload) {
    // First pass: insert every expansion and record the mapping from raw id to
    // the arena key that was allocated for it.
    let mut expn_map: HashMap<u64, ExpnId> = HashMap::new();
    for expn in &payload.expansions {
        let id = hygiene.insert_expn(ExpnData {
            parent: ExpnId::default(), // patched below
            call_site: span_from_wire(expn.call_site),
            def_site: span_from_wire(expn.def_site),
            kind: expn_kind_from_wire(expn.kind),
            desc: String::new(),
        });
        expn_map.insert(expn.id, id);
    }

    // Second pass: patch parent links using the raw->arena mapping.
    for expn in &payload.expansions {
        if expn.parent == 0 {
            continue;
        }
        let Some(id) = expn_map.get(&expn.id).copied() else {
            continue;
        };
        let Some(parent_id) = expn_map.get(&expn.parent).copied() else {
            continue;
        };
        hygiene.update_expn(id, |data| data.parent = parent_id);
    }

    // Third pass: insert contexts. Parent and outer-expansion links use the raw
    // ids from the wire, so resolve them through the maps built above.
    for ctx in &payload.contexts {
        let data = SyntaxContextData {
            parent: ctx.parent.map(SyntaxContextId::new),
            outer_expn: ctx.outer_expn.and_then(|id| expn_map.get(&id).copied()),
            transparency: transparency_from_wire(ctx.transparency),
        };
        hygiene.insert_syntax_context(SyntaxContextId::new(ctx.id), data);
    }
}

fn collect_contexts_from_stream(stream: &TokenStream, out: &mut HashSet<SyntaxContextId>) {
    for tree in stream.iter() {
        collect_contexts_from_tree(tree, out);
    }
}

fn collect_contexts_from_tree(tree: &TokenTree, out: &mut HashSet<SyntaxContextId>) {
    match tree {
        TokenTree::Group(group) => {
            out.insert(group.span.ctx);
            collect_contexts_from_stream(&group.stream, out);
        }
        TokenTree::Ident(ident) => {
            out.insert(ident.span.ctx);
        }
        TokenTree::Punct(punct) => {
            out.insert(punct.span.ctx);
        }
        TokenTree::Literal(lit) => {
            out.insert(lit.span.ctx);
        }
    }
}

fn add_context(
    id: SyntaxContextId,
    hygiene: &HygieneData,
    contexts: &mut Vec<WireSyntaxContext>,
    expansions: &mut Vec<WireExpnData>,
    seen_ctx: &mut HashSet<SyntaxContextId>,
    seen_expn: &mut HashSet<ExpnId>,
) {
    if !seen_ctx.insert(id) {
        return;
    }

    let Some(data) = hygiene.syntax_context_data(id) else {
        return;
    };

    contexts.push(WireSyntaxContext {
        id: id.raw(),
        parent: data.parent.map(|p| p.raw()),
        outer_expn: data.outer_expn.map(|e| e.raw()),
        transparency: transparency_into_wire(data.transparency),
    });

    if let Some(parent) = data.parent {
        add_context(parent, hygiene, contexts, expansions, seen_ctx, seen_expn);
    }
    if let Some(outer_expn) = data.outer_expn {
        add_expn(
            outer_expn, hygiene, contexts, expansions, seen_ctx, seen_expn,
        );
    }
}

fn add_expn(
    id: ExpnId,
    hygiene: &HygieneData,
    contexts: &mut Vec<WireSyntaxContext>,
    expansions: &mut Vec<WireExpnData>,
    seen_ctx: &mut HashSet<SyntaxContextId>,
    seen_expn: &mut HashSet<ExpnId>,
) {
    if !seen_expn.insert(id) {
        return;
    }

    let Some(data) = hygiene.expn_data(id) else {
        return;
    };

    expansions.push(WireExpnData {
        id: id.raw(),
        parent: data.parent.raw(),
        call_site: span_into_wire(data.call_site),
        def_site: span_into_wire(data.def_site),
        kind: expn_kind_into_wire(data.kind),
    });

    if data.parent != ExpnId::default() {
        add_expn(
            data.parent,
            hygiene,
            contexts,
            expansions,
            seen_ctx,
            seen_expn,
        );
    }
    add_context(
        SyntaxContextId::new(data.call_site.syntax_context()),
        hygiene,
        contexts,
        expansions,
        seen_ctx,
        seen_expn,
    );
    add_context(
        SyntaxContextId::new(data.def_site.syntax_context()),
        hygiene,
        contexts,
        expansions,
        seen_ctx,
        seen_expn,
    );
}

fn span_into_wire(span: yelang_lexer::Span) -> WireSpan {
    WireSpan {
        lo: span.start().absolute as u32,
        hi: span.end().absolute as u32,
        file: span.file_id().raw(),
        syntax_context: span.syntax_context(),
    }
}

fn span_from_wire(span: WireSpan) -> yelang_lexer::Span {
    use yelang_lexer::chars::cursor::Position;
    yelang_lexer::Span::new_with_file_id(
        Position {
            line: 1,
            column: 1,
            absolute: span.lo as usize,
        },
        Position {
            line: 1,
            column: 1,
            absolute: span.hi as usize,
        },
        yelang_lexer::FileId::new(span.file),
    )
    .with_syntax_context(span.syntax_context)
}

fn transparency_into_wire(t: Transparency) -> WireTransparency {
    match t {
        Transparency::Opaque => WireTransparency::Opaque,
        Transparency::Transparent => WireTransparency::Transparent,
        Transparency::Mixed => WireTransparency::Mixed,
    }
}

fn transparency_from_wire(t: WireTransparency) -> Transparency {
    match t {
        WireTransparency::Opaque => Transparency::Opaque,
        WireTransparency::Transparent => Transparency::Transparent,
        WireTransparency::Mixed => Transparency::Mixed,
    }
}

fn expn_kind_into_wire(k: ExpnKind) -> WireExpnKind {
    match k {
        ExpnKind::Root => WireExpnKind::Root,
        ExpnKind::MacroRules => WireExpnKind::MacroRules,
        ExpnKind::Macro => WireExpnKind::Macro,
        ExpnKind::ProcMacro => WireExpnKind::ProcMacro,
        ExpnKind::Comptime => WireExpnKind::Comptime,
        ExpnKind::AstPass => WireExpnKind::AstPass,
    }
}

fn expn_kind_from_wire(k: WireExpnKind) -> ExpnKind {
    match k {
        WireExpnKind::Root => ExpnKind::Root,
        WireExpnKind::MacroRules => ExpnKind::MacroRules,
        WireExpnKind::Macro => ExpnKind::Macro,
        WireExpnKind::ProcMacro => ExpnKind::ProcMacro,
        WireExpnKind::Comptime => ExpnKind::Comptime,
        WireExpnKind::AstPass => ExpnKind::AstPass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_interner::Interner;
    use yelang_macro_core::token_tree::{Ident, Span, TokenTree};

    #[test]
    fn payload_includes_reachable_contexts_and_expansions() {
        let interner = Interner::new();
        let hygiene = HygieneData::new();
        let expn = hygiene.fresh_expn(ExpnData {
            parent: hygiene.root_expn(),
            call_site: yelang_lexer::Span::default(),
            def_site: yelang_lexer::Span::default(),
            kind: ExpnKind::Macro,
            desc: "test".to_string(),
        });
        let ctx = hygiene.apply_mark(hygiene.root_syntax_context(), expn, Transparency::Opaque);

        let stream = TokenStream::from_vec(vec![TokenTree::Ident(Ident::new(
            interner.get_or_intern("x"),
            Span::default().with_ctx(ctx),
        ))]);

        let payload = payload_from_stream_with_spans(&stream, &[], &hygiene);
        assert!(payload.contexts.iter().any(|c| c.id == ctx.raw()));
        assert!(payload.expansions.iter().any(|e| e.id == expn.raw()));
    }

    #[test]
    fn payload_includes_extra_span_contexts() {
        let hygiene = HygieneData::new();
        hygiene.insert_syntax_context(SyntaxContextId::new(99), SyntaxContextData::root());
        let extra = yelang_lexer::Span::default().with_syntax_context(99);
        let payload = payload_from_stream_with_spans(&TokenStream::new(), &[extra], &hygiene);
        assert!(payload.contexts.iter().any(|c| c.id == 99));
    }

    #[test]
    fn merge_payload_round_trips_contexts_and_expansions() {
        let hygiene = HygieneData::new();
        let expn = hygiene.fresh_expn(ExpnData {
            parent: hygiene.root_expn(),
            call_site: yelang_lexer::Span::default(),
            def_site: yelang_lexer::Span::default(),
            kind: ExpnKind::Macro,
            desc: "test".to_string(),
        });
        let ctx = hygiene.apply_mark(hygiene.root_syntax_context(), expn, Transparency::Opaque);

        let payload = {
            let stream = TokenStream::from_vec(vec![TokenTree::Ident(Ident::new(
                Interner::new().get_or_intern("x"),
                Span::default().with_ctx(ctx),
            ))]);
            payload_from_stream_with_spans(&stream, &[], &hygiene)
        };

        // Simulate a fresh compiler-side arena and merge the returned payload.
        let fresh = HygieneData::new();
        merge_payload(&fresh, &payload);

        let merged_ctx = fresh.syntax_context_data(ctx).expect("context missing");
        assert_eq!(merged_ctx.transparency, Transparency::Opaque);
        let outer = merged_ctx.outer_expn.expect("outer expn missing");
        let merged_expn = fresh.expn_data(outer).expect("expn missing");
        assert_eq!(merged_expn.kind, ExpnKind::Macro);
    }

    #[test]
    fn merge_payload_with_missing_parent_does_not_panic() {
        let payload = WireHygienePayload {
            contexts: vec![WireSyntaxContext {
                id: 2,
                parent: Some(1),
                outer_expn: Some(7),
                transparency: WireTransparency::Opaque,
            }],
            expansions: vec![WireExpnData {
                id: 7,
                parent: 99,
                call_site: WireSpan {
                    lo: 0,
                    hi: 0,
                    file: 0,
                    syntax_context: 1,
                },
                def_site: WireSpan {
                    lo: 0,
                    hi: 0,
                    file: 0,
                    syntax_context: 1,
                },
                kind: WireExpnKind::Macro,
            }],
        };
        let fresh = HygieneData::new();
        // Should not panic even though parent expn 99 is missing.
        merge_payload(&fresh, &payload);
    }
}
