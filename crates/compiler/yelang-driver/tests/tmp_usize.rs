use yelang_driver::Driver;

#[test]
fn usize_literal_arg() {
    let src = r#"
        fn foo(n: usize) -> usize { n }
        fn test() -> usize { foo(3) }
        fn main() {}
    "#;
    Driver::new().compile_or_eval_main(src).expect("compile");
}
