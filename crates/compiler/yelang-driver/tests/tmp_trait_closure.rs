use yelang_driver::Driver;

#[test]
fn trait_method_closure_param() {
    let src = r#"
        trait Foo<T> {
            fn bar(self, f: fn(&T) -> bool) -> bool;
        }
        struct Wrap<T> { x: T }
        impl<T> Foo<T> for Wrap<T> {
            fn bar(self, f: fn(&T) -> bool) -> bool { f(&self.x) }
        }
        fn test(w: Wrap<i32>) -> bool { w.bar(|x| *x > 2) }
        fn main() {}
    "#;
    Driver::new().compile_or_eval_main(src).expect("compile");
}
