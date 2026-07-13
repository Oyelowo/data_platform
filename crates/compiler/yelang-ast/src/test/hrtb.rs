use super::harness::*;

#[test]
fn test_hrtb_where_predicate_forall_parses_and_codegen_keeps_binder() {
    use crate::item::WherePredicate;

    let mut interner = Interner::new();
    let input = "for<T> T: Clone";
    let mut tokens = TokenKind::tokenize(input, &mut interner).expect("Tokenize failed");
    let pred = tokens
        .parse::<WherePredicate>()
        .expect("Parse WherePredicate failed");

    match &pred {
        WherePredicate::ForAll {
            params, predicate, ..
        } => {
            assert_eq!(params.num_ty_params(), 1);
            assert_eq!(params.num_const_params(), 0);
            match &**predicate {
                WherePredicate::TraitBound { .. } => {}
                other => panic!("expected inner TraitBound predicate, got: {other:?}"),
            }
        }
        other => panic!("expected ForAll predicate, got: {other:?}"),
    }

    let mut output = String::new();
    pred.codegen(&mut output, &interner)
        .expect("Codegen failed");
    assert!(
        output.starts_with("for<"),
        "codegen dropped binder: {output}"
    );

    // Ensure codegen output still parses.
    let mut tokens2 = TokenKind::tokenize(&output, &mut interner).expect("Tokenize 2 failed");
    let _pred2 = tokens2
        .parse::<WherePredicate>()
        .expect("Parse 2 WherePredicate failed");
}
#[test]
fn test_hrtb_trait_bound_binder_parses_and_codegen_keeps_binder() {
    use crate::item::WherePredicate;

    let mut interner = Interner::new();
    let input = "X: for<U> Trait<U>";
    let mut tokens = TokenKind::tokenize(input, &mut interner).expect("Tokenize failed");
    let pred = tokens
        .parse::<WherePredicate>()
        .expect("Parse WherePredicate failed");

    match &pred {
        WherePredicate::TraitBound { bounds, .. } => {
            assert_eq!(bounds.len(), 1);
            assert!(
                bounds[0].binder.is_some(),
                "expected binder on the first trait bound"
            );
        }
        other => panic!("expected TraitBound predicate, got: {other:?}"),
    }

    let mut output = String::new();
    pred.codegen(&mut output, &interner)
        .expect("Codegen failed");
    assert!(
        output.contains("for<"),
        "codegen dropped binder from trait bound: {output}"
    );

    // Ensure codegen output still parses.
    let mut tokens2 = TokenKind::tokenize(&output, &mut interner).expect("Tokenize 2 failed");
    let _pred2 = tokens2
        .parse::<WherePredicate>()
        .expect("Parse 2 WherePredicate failed");
}
#[test]
fn test_hrtb_nested_forall_parses_and_codegen_is_stable() {
    use crate::item::WherePredicate;

    let mut interner = Interner::new();
    let input = "for<T> for<U> T: Trait<U>";
    let mut tokens = TokenKind::tokenize(input, &mut interner).expect("Tokenize failed");
    let pred = tokens
        .parse::<WherePredicate>()
        .expect("Parse WherePredicate failed");

    match &pred {
        WherePredicate::ForAll { predicate, .. } => match &**predicate {
            WherePredicate::ForAll { predicate, .. } => match &**predicate {
                WherePredicate::TraitBound { .. } => {}
                other => panic!("expected inner-most TraitBound, got: {other:?}"),
            },
            other => panic!("expected nested ForAll, got: {other:?}"),
        },
        other => panic!("expected outer ForAll, got: {other:?}"),
    }

    let mut output = String::new();
    pred.codegen(&mut output, &interner)
        .expect("Codegen failed");
    assert!(output.starts_with("for<"));
    assert!(output.contains("for<"));

    // Stronger stability check: codegen should be idempotent.
    assert_round_trip::<WherePredicate>(input);
}
#[test]
fn test_forall_type_parses_and_codegen_is_stable() {
    use crate::Type;

    let mut interner = Interner::new();
    let input = "for<T> fn(T) -> T";
    let mut tokens = TokenKind::tokenize(input, &mut interner).expect("Tokenize failed");
    let ty = tokens.parse::<Type>().expect("Parse Type failed");

    let mut output = String::new();
    ty.codegen(&mut output, &interner).expect("Codegen failed");
    assert_eq!(output, input);
}
#[test]
fn test_forall_type_with_bounded_binder_param_parses_and_codegen_is_stable() {
    use crate::Type;

    let mut interner = Interner::new();
    let input = "for<T: Clone + Debug> fn(T) -> T";
    let mut tokens = TokenKind::tokenize(input, &mut interner).expect("Tokenize failed");
    let ty = tokens.parse::<Type>().expect("Parse Type failed");

    let mut output = String::new();
    ty.codegen(&mut output, &interner).expect("Codegen failed");

    // Strong stability check: printing is idempotent.
    assert_round_trip::<Type>(input);
    assert_eq!(output, input);
}

// --- TESTS ---
