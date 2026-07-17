use yelang_lexer::Span;
use yelang_macro_core::{HygieneData, SyntaxContextId, Transparency};

use crate::namespaces::Namespace;

/// Determine whether a local binding defined at `def_span` is visible from a
/// use site at `use_span`, taking macro hygiene into account.
///
/// The check walks the syntax-context chain from the use site upward. Each
/// macro-expansion mark crossed is inspected:
///
/// * `Transparent` marks are fully transparent: the use site can see bindings
///   from the surrounding context.
/// * `Opaque` marks are barriers: generated identifiers cannot see bindings
///   from outside the macro expansion.
/// * `Mixed` marks behave like `Transparent` when resolving names in the type
///   namespace (so generated types can refer to type parameters and items in
///   the macro definition scope) and like `Opaque` for value and macro
///   namespaces (so generated code cannot capture local variables from the
///   call site).
///
/// When no hygiene data is available, every binding is considered visible so
/// that callers can opt into hygiene-aware resolution without forcing all
/// existing consumers to supply hygiene tables.
pub fn is_visible(
    hygiene: Option<&HygieneData>,
    use_span: Span,
    def_span: Span,
    ns: Namespace,
) -> bool {
    is_visible_with_policy(hygiene, use_span, def_span, transparency_for_namespace(ns))
}

/// Determine whether a module-level item defined at `def_span` is visible from
/// a use site at `use_span`.
///
/// Module items (functions, types, modules, etc.) are visible through
/// `Mixed` macro-expansion marks because macro-generated code is expected to
/// refer to items from the macro's definition scope.
pub fn is_visible_for_item(hygiene: Option<&HygieneData>, use_span: Span, def_span: Span) -> bool {
    is_visible_with_policy(hygiene, use_span, def_span, |_| true)
}

fn is_visible_with_policy(
    hygiene: Option<&HygieneData>,
    use_span: Span,
    def_span: Span,
    mut is_transparent: impl FnMut(Transparency) -> bool,
) -> bool {
    let Some(hygiene) = hygiene else {
        return true;
    };
    is_visible_by_context(
        hygiene,
        SyntaxContextId::new(use_span.syntax_context()),
        SyntaxContextId::new(def_span.syntax_context()),
        &mut is_transparent,
    )
}

fn is_visible_by_context(
    hygiene: &HygieneData,
    mut use_ctx: SyntaxContextId,
    def_ctx: SyntaxContextId,
    is_transparent: &mut dyn FnMut(Transparency) -> bool,
) -> bool {
    loop {
        if use_ctx == def_ctx {
            return true;
        }

        let Some(data) = hygiene.syntax_context_data(use_ctx) else {
            // Missing hygiene data is a compiler bug, but treating it as a
            // barrier is the safe default.
            return false;
        };

        let Some(parent) = data.parent else {
            // Reached the root context without finding the definition context.
            return false;
        };

        if !is_transparent(data.transparency) {
            return false;
        }

        use_ctx = parent;
    }
}

fn transparency_for_namespace(ns: Namespace) -> impl Fn(Transparency) -> bool {
    move |t| match t {
        Transparency::Transparent => true,
        Transparency::Opaque => false,
        Transparency::Mixed => matches!(ns, Namespace::Type),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_lexer::Span;
    use yelang_macro_core::{ExpnData, ExpnKind, HygieneData, Transparency};

    fn ctx_span(id: u32) -> Span {
        Span::default().with_syntax_context(id)
    }

    #[test]
    fn same_context_is_always_visible() {
        let hygiene = HygieneData::new();
        let root = hygiene.root_syntax_context();
        assert!(is_visible(
            Some(&hygiene),
            ctx_span(root.raw()),
            ctx_span(root.raw()),
            Namespace::Value
        ));
    }

    #[test]
    fn opaque_mark_hides_outer_binding() {
        let hygiene = HygieneData::new();
        let expn = hygiene.fresh_expn(ExpnData {
            parent: hygiene.root_expn(),
            call_site: Span::default(),
            def_site: Span::default(),
            kind: ExpnKind::Macro,
            desc: "opaque".to_string(),
        });
        let inner = hygiene.apply_mark(hygiene.root_syntax_context(), expn, Transparency::Opaque);

        assert!(!is_visible(
            Some(&hygiene),
            ctx_span(inner.raw()),
            ctx_span(hygiene.root_syntax_context().raw()),
            Namespace::Value
        ));
        assert!(!is_visible(
            Some(&hygiene),
            ctx_span(inner.raw()),
            ctx_span(hygiene.root_syntax_context().raw()),
            Namespace::Type
        ));
    }

    #[test]
    fn transparent_mark_allows_outer_binding() {
        let hygiene = HygieneData::new();
        let expn = hygiene.fresh_expn(ExpnData {
            parent: hygiene.root_expn(),
            call_site: Span::default(),
            def_site: Span::default(),
            kind: ExpnKind::Macro,
            desc: "transparent".to_string(),
        });
        let inner = hygiene.apply_mark(
            hygiene.root_syntax_context(),
            expn,
            Transparency::Transparent,
        );

        assert!(is_visible(
            Some(&hygiene),
            ctx_span(inner.raw()),
            ctx_span(hygiene.root_syntax_context().raw()),
            Namespace::Value
        ));
    }

    #[test]
    fn mixed_is_transparent_for_type_namespace_only() {
        let hygiene = HygieneData::new();
        let expn = hygiene.fresh_expn(ExpnData {
            parent: hygiene.root_expn(),
            call_site: Span::default(),
            def_site: Span::default(),
            kind: ExpnKind::MacroRules,
            desc: "mixed".to_string(),
        });
        let inner = hygiene.apply_mark(hygiene.root_syntax_context(), expn, Transparency::Mixed);

        assert!(is_visible(
            Some(&hygiene),
            ctx_span(inner.raw()),
            ctx_span(hygiene.root_syntax_context().raw()),
            Namespace::Type
        ));
        assert!(!is_visible(
            Some(&hygiene),
            ctx_span(inner.raw()),
            ctx_span(hygiene.root_syntax_context().raw()),
            Namespace::Value
        ));
        assert!(!is_visible(
            Some(&hygiene),
            ctx_span(inner.raw()),
            ctx_span(hygiene.root_syntax_context().raw()),
            Namespace::Macro
        ));
    }

    #[test]
    fn mixed_item_visibility_is_transparent_for_all_namespaces() {
        let hygiene = HygieneData::new();
        let expn = hygiene.fresh_expn(ExpnData {
            parent: hygiene.root_expn(),
            call_site: Span::default(),
            def_site: Span::default(),
            kind: ExpnKind::MacroRules,
            desc: "mixed".to_string(),
        });
        let inner = hygiene.apply_mark(hygiene.root_syntax_context(), expn, Transparency::Mixed);

        assert!(is_visible_for_item(
            Some(&hygiene),
            ctx_span(inner.raw()),
            ctx_span(hygiene.root_syntax_context().raw()),
        ));
    }

    #[test]
    fn no_hygiene_data_defaults_to_visible() {
        assert!(is_visible(
            None,
            Span::default(),
            Span::default(),
            Namespace::Value
        ));
    }

    #[test]
    fn unknown_definition_context_is_not_visible() {
        let hygiene = HygieneData::new();
        let unknown = SyntaxContextId::new(999);
        assert!(!is_visible(
            Some(&hygiene),
            ctx_span(hygiene.root_syntax_context().raw()),
            ctx_span(unknown.raw()),
            Namespace::Value
        ));
    }
}
