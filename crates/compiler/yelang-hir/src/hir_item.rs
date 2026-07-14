//! Items in HIR.

use yelang_ast::Ident;
use yelang_lexer::Span;

use crate::ids::{BodyId, DefId};
use crate::hir::{
    Body, EnumDef, FnSig, Generics, Impl, MacroDef, Mutability, Trait, Ty, UseKind,
    UsePath, VariantData, Visibility,
};

/// An item in the HIR.
#[derive(Debug, Clone)]
pub struct Item {
    pub def_id: DefId,
    pub ident: Ident,
    pub kind: ItemKind,
    pub vis: Visibility,
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
    Enum {
        def: EnumDef,
        generics: Generics,
    },
    /// Union definition.
    Union {
        data: VariantData,
        generics: Generics,
    },
    /// Trait definition.
    Trait {
        items: Vec<crate::hir::TraitItem>,
        generics: Generics,
    },
    /// Impl block.
    Impl {
        items: Vec<crate::hir::ImplItem>,
        generics: Generics,
        self_ty: Ty,
        of_trait: Option<crate::hir::TraitRef>,
    },
    /// Type alias.
    TyAlias {
        ty: Ty,
        generics: Generics,
    },
    /// Constant item.
    Const {
        ty: Ty,
        body: BodyId,
    },
    /// Static item.
    Static {
        ty: Ty,
        mutability: Mutability,
        body: BodyId,
    },
    /// Module.
    Mod {
        items: Vec<DefId>,
    },
    /// Use declaration.
    Use {
        path: UsePath,
        kind: UseKind,
    },
    /// Macro definition.
    Macro {
        def: MacroDef,
    },
}
