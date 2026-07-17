use crate::canonical::{CanonicalTyVarKind, CanonicalVarKind, Certainty, Response};
use crate::list::List;
use crate::ty::UniverseIndex;

#[test]
fn canonical_var_kind_variants() {
    let k1 = CanonicalVarKind::Ty(CanonicalTyVarKind::General(UniverseIndex(0)));
    let k2 = CanonicalVarKind::Int;
    let k3 = CanonicalVarKind::Float;
    let _k4 = CanonicalVarKind::Const;
    assert_ne!(k1, k2);
    assert_ne!(k2, k3);
}

#[test]
fn response_certainty() {
    let r = Response {
        certainty: Certainty::Yes,
        goals: List::empty(),
    };
    assert_eq!(r.certainty, Certainty::Yes);
}
