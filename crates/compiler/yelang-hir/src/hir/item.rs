//! Items in HIR.

use yelang_ast::Ident;
use yelang_lexer::Span;

use crate::crate_data::Crate;
use crate::hir::core::{
    EnumDef, FnSig, Generics, Mutability, UseKind, UsePath, VariantData,
    Visibility,
};
use crate::ids::{BodyId, DefId, ItemKindId, TyId};

/// An item in the HIR.
#[derive(Debug, Clone)]
pub struct Item {
    pub def_id: DefId,
    pub ident: Ident,
    pub kind: ItemKindId,
    pub vis: Visibility,
    pub attrs: Vec<crate::hir::core::Attribute>,
    pub span: Span,
}

impl Item {
    /// Resolve the item's payload from the crate arena.
    pub fn kind<'a>(&self, krate: &'a Crate) -> &'a ItemKind {
        krate
            .item_kinds
            .get(self.kind)
            .expect("ItemKindId should be allocated")
    }
}

/// Kinds of items.
#[derive(Debug, Clone)]
pub enum ItemKind {
    /// Function definition.
    Fn {
        sig: FnSig,
        body: BodyId,
        generics: Generics,
    },
    /// Struct definition.
    Struct {
        data: VariantData,
        generics: Generics,
    },
    /// Enum definition.
    Enum { def: EnumDef, generics: Generics },
    /// Trait definition.
    Trait {
        items: Vec<crate::hir::core::TraitItem>,
        generics: Generics,
        super_traits: Vec<crate::hir::core::TraitRef>,
    },
    /// Impl block.
    Impl {
        items: Vec<crate::hir::core::ImplItem>,
        generics: Generics,
        self_ty: TyId,
        of_trait: Option<crate::hir::core::TraitRef>,
        polarity: crate::hir::core::ImplPolarity,
    },
    /// Type alias.
    TyAlias { ty: TyId, generics: Generics },
    /// Constant item.
    Const { ty: TyId, body: BodyId },
    /// Static item.
    Static {
        ty: TyId,
        mutability: Mutability,
        body: BodyId,
    },
    /// Module.
    Mod { items: Vec<DefId> },
    /// Use declaration.
    Use { path: UsePath, kind: UseKind },
}
