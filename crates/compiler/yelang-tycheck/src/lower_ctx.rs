/*! Type-lowering context trait.
 *
 * Both the signature collector and the body type checker need to lower HIR
 * types to canonical `TyId`. This trait abstracts the small amount of state
 * required for lowering: the interner, HIR crate, item-type table, self type,
 * and the handling of `_` / missing type annotations.
 */

use yelang_arena::DefId;
use yelang_hir::Crate as HirCrate;
use yelang_hir::ids::ExprId;
use yelang_ty::interner::Interner;
use yelang_ty::primitive::{FloatTy, IntTy, UintTy};
use yelang_ty::ty::{Mutability, Ty, TyId};

/// Context used when lowering a HIR type to `TyId`.
pub trait TyLowerCtxt {
    /// The interner for creating canonical types.
    fn interner(&self) -> &Interner;

    /// The HIR crate used to look up type nodes.
    fn crate_hir(&self) -> &HirCrate;

    /// Look up the type of an item by `DefId`.
    fn item_ty(&self, def_id: DefId) -> Option<TyId>;

    /// Look up a type parameter by its `DefId`.
    fn param_ty(&self, def_id: DefId) -> Option<TyId> {
        let _ = def_id;
        None
    }

    /// The `Self` type, if inside an impl block.
    fn self_ty(&self) -> Option<TyId>;

    /// Lower an explicit `_` inference annotation.
    /// In bodies this becomes a fresh inference variable; in item signatures it
    /// is currently an error type (return-type inference is handled later).
    fn lower_infer(&mut self) -> TyId;

    /// Lower a missing type annotation.
    fn lower_missing(&mut self) -> TyId;

    /// Lower `typeof expr`. The default returns an error type; body checking
    /// overrides this to infer the expression's type.
    fn lower_typeof(&mut self, _expr: ExprId) -> TyId {
        self.mk_error()
    }

    // -----------------------------------------------------------------------
    // Convenience constructors
    // -----------------------------------------------------------------------

    fn mk_ty(&self, kind: Ty) -> TyId {
        self.interner().mk_ty(kind)
    }

    fn mk_unit(&self) -> TyId {
        self.mk_ty(Ty::Tuple(yelang_ty::list::List::empty()))
    }

    fn mk_never(&self) -> TyId {
        self.mk_ty(Ty::Never)
    }

    fn mk_error(&self) -> TyId {
        self.mk_ty(Ty::Error)
    }

    fn mk_bool(&self) -> TyId {
        self.mk_ty(Ty::Bool)
    }

    fn mk_char(&self) -> TyId {
        self.mk_ty(Ty::Char)
    }

    fn mk_str(&self) -> TyId {
        self.mk_ty(Ty::Str)
    }

    fn mk_int(&self, it: IntTy) -> TyId {
        self.mk_ty(Ty::Int(it))
    }

    fn mk_uint(&self, ut: UintTy) -> TyId {
        self.mk_ty(Ty::Uint(ut))
    }

    fn mk_float(&self, ft: FloatTy) -> TyId {
        self.mk_ty(Ty::Float(ft))
    }

    fn mk_ref(&self, ty: TyId, mutbl: Mutability) -> TyId {
        self.mk_ty(Ty::Ref(ty, mutbl))
    }
}
