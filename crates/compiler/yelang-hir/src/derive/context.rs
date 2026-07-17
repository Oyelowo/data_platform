//! Context passed to built-in derive implementations.

use yelang_arena::DefId;
use yelang_interner::Symbol;
use yelang_lexer::Span;

use crate::hir::{EnumDef, Generics, ItemKind, VariantData};
use crate::hir_item::Item as HirItem;
use crate::ids::TyId;
use crate::lowering::LoweringContext;
use crate::res::Res;

use super::error::DeriveError;

/// Information about the ADT (struct or enum) being derived for.
#[derive(Debug, Clone)]
pub struct AdtInfo<'a> {
    /// The HIR item for the ADT.
    pub item: &'a HirItem,
    /// The struct/enum's `DefId`.
    pub def_id: DefId,
    /// The struct/enum's identifier.
    pub ident: yelang_ast::Ident,
    /// The generic parameters of the ADT.
    pub generics: &'a Generics,
    /// The shape of the ADT.
    pub shape: AdtShape<'a>,
}

/// Shape of an algebraic data type.
#[derive(Debug, Clone)]
pub enum AdtShape<'a> {
    Struct(&'a VariantData),
    Enum(&'a EnumDef),
}

impl<'a> AdtInfo<'a> {
    /// Build `Self` as a HIR type reference.
    ///
    /// For now this produces a monomorphic reference (`Point`) because HIR does
    /// not yet assign `DefId`s to generic type parameters. Once parameter
    /// references are represented in HIR, this will be extended to
    /// `Point<T, U>`.
    pub fn self_ty(&self, ctx: &mut DeriveContext<'_, '_>) -> TyId {
        let span = self.ident.span();
        ctx.ctx.crate_hir.alloc_ty(
            crate::hir_ty::Ty::Path {
                res: Res::Def {
                    def_id: self.def_id,
                },
                args: vec![],
            },
            span,
        )
    }
}

/// Context available during built-in derive expansion.
pub struct DeriveContext<'a, 'b: 'a> {
    /// The HIR item being derived for.
    pub hir_item: &'a HirItem,
    /// The AST item being derived for.
    pub ast_item: &'a yelang_ast::Item,
    /// The lowering context, used to allocate IDs and access the resolver.
    pub ctx: &'a mut LoweringContext<'b>,
    /// Span of the `@derive(...)` attribute that triggered this expansion.
    pub derive_span: Span,
    /// Name of the derive being expanded.
    pub derive_name: Symbol,
}

impl<'a, 'b: 'a> DeriveContext<'a, 'b> {
    /// Try to extract ADT information from the item being derived for.
    pub fn adt_info(&self) -> Result<AdtInfo<'a>, DeriveError> {
        let (generics, shape) = match &self.hir_item.kind {
            ItemKind::Struct { data, generics } => (generics, AdtShape::Struct(data)),
            ItemKind::Enum { def, generics } => (generics, AdtShape::Enum(def)),
            other => {
                return Err(DeriveError::UnsupportedItem {
                    derive: self.derive_name,
                    item_kind: item_kind_name(other),
                    span: self.derive_span,
                });
            }
        };
        Ok(AdtInfo {
            item: self.hir_item,
            def_id: self.hir_item.def_id,
            ident: self.hir_item.ident,
            generics,
            shape,
        })
    }

    /// Look up a trait `DefId` by name.
    ///
    /// User-defined items in the current module shadow the prelude, but prelude
    /// traits are available as a fallback because they are not inserted into
    /// module namespace tables.
    pub fn trait_def_id(&mut self, name: &str) -> Result<DefId, DeriveError> {
        let symbol = self.ctx.interner.get_or_intern(name);
        self.resolve_in_module_or_prelude(yelang_resolve::Namespace::Type, symbol)
            .ok_or_else(|| DeriveError::MissingTrait {
                derive: self.derive_name,
                trait_name: symbol,
                span: self.derive_span,
            })
    }

    /// Look up a `DefId` in the current module namespace, falling back to the
    /// built-in prelude.
    pub fn resolve_in_module_or_prelude(
        &self,
        ns: yelang_resolve::Namespace,
        name: Symbol,
    ) -> Option<DefId> {
        if let Some(module) = self
            .ctx
            .resolved
            .module_tree
            .modules
            .get(&self.ctx.current_module)
        {
            if let Some(def_id) = module.get_item(ns, name) {
                return Some(def_id);
            }
        }
        self.ctx
            .resolved
            .prelude
            .as_ref()
            .and_then(|p| p.items.get(&ns).and_then(|m| m.get(&name)).copied())
    }

    /// Look up the `DefId` of an enum variant by name.
    pub fn variant_def_id(&self, enum_def_id: DefId, name: Symbol) -> Option<DefId> {
        self.ctx
            .resolved
            .enum_variants
            .get(&enum_def_id)
            .and_then(|m| m.get(&name).copied())
    }

    /// Record a derive error.
    pub fn error(&mut self, err: DeriveError) {
        self.ctx.error(err.into());
    }

    /// Allocate a fresh synthetic `DefId` for a compiler-generated item.
    pub fn next_synthetic_def_id(&mut self) -> DefId {
        self.ctx.next_synthetic_def_id()
    }

    /// Intern a string into a symbol.
    pub fn intern(&self, s: &str) -> Symbol {
        self.ctx.interner.get_or_intern(s)
    }

    /// Build a `Res::Def` for a path to the given definition.
    pub fn res_def(&self, def_id: DefId) -> Res {
        Res::Def { def_id }
    }
}

pub(crate) fn item_kind_name(kind: &ItemKind) -> &'static str {
    match kind {
        ItemKind::Fn { .. } => "function",
        ItemKind::Struct { .. } => "struct",
        ItemKind::Enum { .. } => "enum",
        ItemKind::Union { .. } => "union",
        ItemKind::Trait { .. } => "trait",
        ItemKind::Impl { .. } => "impl",
        ItemKind::TyAlias { .. } => "type alias",
        ItemKind::Const { .. } => "const",
        ItemKind::Static { .. } => "static",
        ItemKind::Mod { .. } => "module",
        ItemKind::Use { .. } => "use",
    }
}
