use crate::canonical::{CanonicalVarKind, CanonicalTyVarKind, Response, Certainty};
use crate::ty::UniverseIndex;
use crate::list::List;

#[test]
fn canonical_var_kind_variants() {
    let k1 = CanonicalVarKind::Ty(CanonicalTyVarKind::General(UniverseIndex(0)));
    let k2 = CanonicalVarKind::Int;
    let k3 = CanonicalVarKind::Float;
    let k4 = CanonicalVarKind::Const;
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
