//! Tests for QIR identifier types.

use yelang_qir::ids::{BinderId, ExecId, LirId, PirId, QExprId};

#[test]
fn ids_are_copy_and_hashable() {
    let a = LirId(0);
    let b = LirId(0);
    let c = LirId(1);
    assert_eq!(a, b);
    assert_ne!(a, c);

    let mut set = std::collections::HashSet::new();
    set.insert(a);
    set.insert(c);
    assert_eq!(set.len(), 2);
}

#[test]
fn all_id_types_implement_display() {
    assert_eq!(format!("{}", LirId(7)), "7");
    assert_eq!(format!("{}", PirId(7)), "7");
    assert_eq!(format!("{}", ExecId(7)), "7");
    assert_eq!(format!("{}", QExprId(7)), "7");
    assert_eq!(format!("{}", BinderId(7)), "7");
}

#[test]
fn ids_round_trip_through_usize() {
    use yelang_arena::index_vec::Idx;

    assert_eq!(LirId::from_usize(3).index(), 3);
    assert_eq!(PirId::from_usize(5).index(), 5);
}
