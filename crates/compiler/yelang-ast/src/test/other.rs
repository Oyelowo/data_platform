use super::harness::*;

#[test]
fn test_link_edge_requires_object_payload() {
    let mut interner = Interner::new();

    // Missing `{}` payload should be rejected.
    let bad = "link (a:A) -> [e@e:Edge] -> (b:B);";
    let mut bad_stream = TokenKind::tokenize(bad, &mut interner).expect("Tokenization failed");
    assert!(
        bad_stream.parse::<Stmt>().is_err(),
        "expected LINK edge without object payload to be a parse error"
    );

    // Empty `{}` payload should be accepted.
    let good = "link (a:A) -> [e@e:Edge {}] -> (b:B);";
    let mut good_stream = TokenKind::tokenize(good, &mut interner).expect("Tokenization failed");
    assert!(
        good_stream.parse::<Stmt>().is_ok(),
        "expected LINK edge with empty object payload to parse"
    );
}

/// Helper to parse a pattern and return the AST
#[allow(dead_code)]
pub(super) fn parse_pattern(input: &str) -> crate::Pattern {
    let mut interner = Interner::new();
    let mut token_stream = TokenKind::tokenize(input, &mut interner).expect("Tokenization failed");
    token_stream
        .parse::<crate::Pattern>()
        .expect("Parsing failed")
}

// 1. Helper for Round-Trip
#[track_caller] // Shows the line number of the failure in the test function
pub(super) fn assert_round_trip<T>(input: &str)
where
    T: ParseTokenStream<TokenKind> + Codegen + Clone + PartialEq + std::fmt::Debug,
{
    let mut interner = Interner::new();

    // A. Parse Original
    let mut tokens = TokenKind::tokenize(input, &mut interner).expect("Tokenize failed");
    // panic!("xxx{tokens}");
    let ast = tokens.parse::<T>().expect("Parse failed");

    // B. Codegen
    let mut output = String::new();
    ast.codegen(&mut output, &interner).expect("Codegen failed");

    // C. Parse Generated
    let mut tokens2 = TokenKind::tokenize(&output, &mut interner).expect("Tokenize 2 failed");
    let ast2 = tokens2.parse::<T>().unwrap_or_else(|e| {
        eprintln!("Original input: {}", input);
        eprintln!("Codegen output: {}", output);
        panic!("Parse 2 failed: {:?}", e);
    });

    // D. Codegen again
    let mut output2 = String::new();
    ast2.codegen(&mut output2, &interner)
        .expect("Codegen 2 failed");

    // E. Compare outputs
    assert_eq!(
        output, output2,
        "Round trip codegen mismatch for input: {}",
        input
    );
}

// 2. Helper for Snapshots
#[track_caller]
pub(super) fn assert_snapshot(name: &str, input: &str) {
    let mut interner = Interner::new();
    let mut tokens = TokenKind::tokenize(input, &mut interner).unwrap();
    let ast = tokens.parse::<Stmt>().unwrap();

    // Snapshot the Raw AST (Debug)
    insta::assert_debug_snapshot!(name, ast);
}
