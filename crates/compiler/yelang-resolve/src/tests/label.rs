use crate::*;
use crate::tests::parse_program;

#[test]
fn break_without_label_in_loop_ok() {
    let src = r#"
        fn main() {
            loop { break; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_without_label_in_while_ok() {
    let src = r#"
        fn main() {
            while true { break; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_without_label_in_for_ok() {
    let src = r#"
        fn main() {
            for x in 0..10 { break; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn continue_without_label_in_loop_ok() {
    let src = r#"
        fn main() {
            loop { continue; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_label_resolves_to_loop() {
    let src = r#"
        fn main() {
            'outer: loop { break 'outer; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_label_resolves_to_while() {
    let src = r#"
        fn main() {
            'outer: while true { break 'outer; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_label_resolves_to_for() {
    let src = r#"
        fn main() {
            'outer: for x in 0..10 { break 'outer; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn continue_label_resolves_to_loop() {
    let src = r#"
        fn main() {
            'outer: loop { continue 'outer; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_label_not_found() {
    let src = r#"
        fn main() {
            loop { break 'missing; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::LabelError { .. })),
        "expected label error: {:?}",
        resolved.errors
    );
}

#[test]
fn continue_label_not_found() {
    let src = r#"
        fn main() {
            loop { continue 'missing; }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::LabelError { .. })),
        "expected label error: {:?}",
        resolved.errors
    );
}

#[test]
fn nested_loops_different_labels() {
    let src = r#"
        fn main() {
            'outer: loop {
                'inner: loop {
                    break 'outer;
                }
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn label_shadowing_inner_shadows_outer() {
    let src = r#"
        fn main() {
            'outer: loop {
                'outer: loop {
                    break 'outer;
                }
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_outside_loop_error() {
    let src = r#"
        fn main() {
            break;
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::BreakOutsideLoop { .. })),
        "expected break outside loop error: {:?}",
        resolved.errors
    );
}

#[test]
fn continue_outside_loop_error() {
    let src = r#"
        fn main() {
            continue;
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::ContinueOutsideLoop { .. })),
        "expected continue outside loop error: {:?}",
        resolved.errors
    );
}

#[test]
fn break_label_not_across_function() {
    let src = r#"
        fn foo() {
            'outer: loop { break 'outer; }
        }
        fn main() {
            'outer: loop {
                foo();
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_unlabeled_innermost_loop() {
    let src = r#"
        fn main() {
            loop {
                loop {
                    break;
                }
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn continue_unlabeled_innermost_loop() {
    let src = r#"
        fn main() {
            loop {
                loop {
                    continue;
                }
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_with_value_in_loop() {
    let src = r#"
        fn main() {
            let x = 'outer: loop { break 'outer 42; };
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn break_label_not_found_in_block() {
    let src = r#"
        fn main() {
            'block: {
                break 'missing;
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::LabelError { .. })),
        "expected label error: {:?}",
        resolved.errors
    );
}

#[test]
fn break_resolves_to_labeled_block() {
    let src = r#"
        fn main() {
            'block: {
                break 'block;
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(resolved.errors.is_empty(), "errors: {:?}", resolved.errors);
}

#[test]
fn continue_cannot_target_block() {
    let src = r#"
        fn main() {
            'block: {
                continue 'block;
            }
        }
    "#;
    let (program, interner) = parse_program(src);
    let resolved = resolve_crate(&program, &interner);
    assert!(
        resolved.errors.iter().any(|e| matches!(e, ResolutionError::LabelError { .. })),
        "expected label error: {:?}",
        resolved.errors
    );
}
