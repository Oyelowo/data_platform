//! Items in HIR.

use yelang_ast::Ident;
use yelang_lexer::Span;

use crate::hir::{
    EnumDef, FnSig, Generics, Mutability, UseKind, UsePath, VariantData,
    Visibility,
};
use crate::ids::{BodyId, DefId, TyId};

/// An item in the HIR.
#[derive(Debug, Clone)]
pub struct Item {
    pub def_id: DefId,
    pub ident: Ident,
    pub kind: ItemKind,
    pub vis: Visibility,
    pub attrs: Vec<crate::hir::Attribute>,
    pub span: Span,
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
        items: Vec<crate::hir::TraitItem>,
        generics: Generics,
        super_traits: Vec<crate::hir::TraitRef>,
    },
    /// Impl block.
    Impl {
        items: Vec<crate::hir::ImplItem>,
        generics: Generics,
        self_ty: TyId,
        of_trait: Option<crate::hir::TraitRef>,
        polarity: crate::hir::ImplPolarity,
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
