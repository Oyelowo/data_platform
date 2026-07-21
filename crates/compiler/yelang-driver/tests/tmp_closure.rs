use yelang_driver::Driver;

#[test]
fn closure_param_from_fn_ptr() {
    let src = r#"
        fn foo(f: fn(i32) -> bool) -> bool { f(3) }
        fn test() -> bool { foo(|x| x > 2) }
        fn main() {}
    "#;
    Driver::new().compile_or_eval_main(src).expect("compile");
}
