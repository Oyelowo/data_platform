use yelang_ast::{ItemKind, TokenKind};
use yelang_interner::Interner;
use yelang_macro::{InProcessExecutor, InProcessProcMacro, MacroExpander};
use yelang_proc_macro::{Diagnostic, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use yelang_proc_macro_bridge::protocol::ProcMacroKind;

fn parse_program(src: &str) -> (yelang_ast::Program, Interner) {
    let mut interner = Interner::new();
    let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
    let program = stream.parse::<yelang_ast::Program>().unwrap();
    (program, interner)
}

struct EchoFnLike;

impl InProcessProcMacro for EchoFnLike {
    fn kind(&self) -> ProcMacroKind {
        ProcMacroKind::FunctionLike
    }

    fn name(&self) -> &str {
        "echo"
    }

    fn expand_fn_like(&self, input: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        (input, Vec::new())
    }

    fn expand_attr(&self, _args: TokenStream, item: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        (item, Vec::new())
    }

    fn expand_derive(&self, _item: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        (TokenStream::new(), Vec::new())
    }
}

struct ReplaceWithLiteral;

impl InProcessProcMacro for ReplaceWithLiteral {
    fn kind(&self) -> ProcMacroKind {
        ProcMacroKind::FunctionLike
    }

    fn name(&self) -> &str {
        "answer"
    }

    fn expand_fn_like(&self, _input: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        let mut stream = TokenStream::new();
        stream.push(TokenTree::Literal(Literal::integer(
            "42",
            Span::call_site(),
        )));
        (stream, Vec::new())
    }

    fn expand_attr(&self, _args: TokenStream, item: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        (item, Vec::new())
    }

    fn expand_derive(&self, _item: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        (TokenStream::new(), Vec::new())
    }
}

struct StripAttribute;

impl InProcessProcMacro for StripAttribute {
    fn kind(&self) -> ProcMacroKind {
        ProcMacroKind::Attribute
    }

    fn name(&self) -> &str {
        "strip"
    }

    fn expand_fn_like(&self, input: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        (input, Vec::new())
    }

    fn expand_attr(&self, _args: TokenStream, item: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        // Return the item unchanged; the test verifies the attribute macro is invoked.
        (item, Vec::new())
    }

    fn expand_derive(&self, _item: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        (TokenStream::new(), Vec::new())
    }
}

struct GenerateConstItem;

impl InProcessProcMacro for GenerateConstItem {
    fn kind(&self) -> ProcMacroKind {
        ProcMacroKind::Derive
    }

    fn name(&self) -> &str {
        "GenerateConst"
    }

    fn expand_fn_like(&self, input: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        (input, Vec::new())
    }

    fn expand_attr(&self, _args: TokenStream, item: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        (item, Vec::new())
    }

    fn expand_derive(&self, _item: TokenStream) -> (TokenStream, Vec<Diagnostic>) {
        // Generate: const _DERIVE_OUTPUT: i32 = 0;
        let mut output = TokenStream::new();
        output.push(TokenTree::Ident(Ident::new("const", Span::call_site())));
        output.push(TokenTree::Ident(Ident::new(
            "_DERIVE_OUTPUT",
            Span::call_site(),
        )));
        output.push(TokenTree::Punct(Punct::new(
            ':',
            Spacing::Alone,
            Span::call_site(),
        )));
        output.push(TokenTree::Ident(Ident::new("i32", Span::call_site())));
        output.push(TokenTree::Punct(Punct::new(
            '=',
            Spacing::Alone,
            Span::call_site(),
        )));
        output.push(TokenTree::Literal(Literal::integer("0", Span::call_site())));
        output.push(TokenTree::Punct(Punct::new(
            ';',
            Spacing::Alone,
            Span::call_site(),
        )));
        (output, Vec::new())
    }
}

#[test]
fn function_like_proc_macro_replaces_input() {
    let (program, interner) = parse_program(
        r#"
        fn main() {
            let x = answer!();
        }
    "#,
    );
    let mut executor = InProcessExecutor::new();
    executor.register(Box::new(ReplaceWithLiteral));
    let mut expander = MacroExpander::new(&interner).with_in_process_proc_macros(executor);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let fn_item = &result.program.items[0];
    let ItemKind::Fn(func) = &fn_item.kind else {
        panic!("expected fn")
    };
    assert_eq!(func.body.statements.len(), 1);
}

#[test]
fn function_like_proc_macro_echoes_input() {
    let (program, interner) = parse_program(
        r#"
        fn main() {
            let x = echo!(1 + 2);
        }
    "#,
    );
    let mut executor = InProcessExecutor::new();
    executor.register(Box::new(EchoFnLike));
    let mut expander = MacroExpander::new(&interner).with_in_process_proc_macros(executor);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let fn_item = &result.program.items[0];
    let ItemKind::Fn(func) = &fn_item.kind else {
        panic!("expected fn")
    };
    assert_eq!(func.body.statements.len(), 1);
}

#[test]
fn attribute_proc_macro_is_invoked() {
    let (program, interner) = parse_program(
        r#"
        @strip
        fn main() {}
    "#,
    );
    let mut executor = InProcessExecutor::new();
    executor.register(Box::new(StripAttribute));
    let mut expander = MacroExpander::new(&interner).with_in_process_proc_macros(executor);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.program.items.len(), 1);
    let fn_item = &result.program.items[0];
    let ItemKind::Fn(func) = &fn_item.kind else {
        panic!("expected fn")
    };
    assert_eq!(interner.resolve(&func.name.symbol), "main");
}

#[test]
fn derive_proc_macro_generates_item() {
    let (program, interner) = parse_program(
        r#"
        struct Foo;

        @derive(GenerateConst)
        struct Bar;
    "#,
    );
    let mut executor = InProcessExecutor::new();
    executor.register(Box::new(GenerateConstItem));
    let mut expander = MacroExpander::new(&interner).with_in_process_proc_macros(executor);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    // Should have Foo, Bar, and the generated const.
    assert_eq!(result.program.items.len(), 3);
}

#[test]
fn macro_export_generates_fn_like_wrapper_and_entry_point() {
    let (program, interner) = parse_program(
        r#"
        @yelang_proc_macro::macro_export
        fn echo(input: TokenStream) -> TokenStream {
            input
        }
    "#,
    );
    let mut expander = MacroExpander::new(&interner);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let names: Vec<&str> = result
        .program
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Fn(f) => Some(interner.resolve(&f.name.symbol)),
            _ => None,
        })
        .collect();

    // Original macro implementation is preserved.
    assert!(names.contains(&"echo"), "missing echo: {names:?}");
    // Generated wrapper, allocator, and entry point.
    assert!(
        names.contains(&"yelang_macro_echo"),
        "missing wrapper: {names:?}"
    );
    assert!(names.contains(&"yelang_alloc"), "missing alloc: {names:?}");
    assert!(names.contains(&"yelang_free"), "missing free: {names:?}");
    assert!(
        names.contains(&"yelang_proc_macro_entry"),
        "missing entry: {names:?}"
    );

    // The export attribute should be stripped from the original function.
    let echo_item = result
        .program
        .items
        .iter()
        .find(|item| matches!(&item.kind, ItemKind::Fn(f) if interner.resolve(&f.name.symbol) == "echo"))
        .unwrap();
    assert!(echo_item.attributes.is_empty());
}

#[test]
fn macro_export_attribute_generates_wrapper() {
    let (program, interner) = parse_program(
        r#"
        @yelang_proc_macro::macro_export_attribute
        fn strip(args: TokenStream, item: TokenStream) -> TokenStream {
            item
        }
    "#,
    );
    let mut expander = MacroExpander::new(&interner);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let names: Vec<&str> = result
        .program
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Fn(f) => Some(interner.resolve(&f.name.symbol)),
            _ => None,
        })
        .collect();

    assert!(names.contains(&"strip"), "missing strip: {names:?}");
    assert!(
        names.contains(&"yelang_macro_strip"),
        "missing wrapper: {names:?}"
    );
}

#[test]
fn macro_export_derive_generates_wrapper() {
    let (program, interner) = parse_program(
        r#"
        @yelang_proc_macro::macro_export_derive
        fn generate(item: TokenStream) -> TokenStream {
            item
        }
    "#,
    );
    let mut expander = MacroExpander::new(&interner);
    let result = expander.expand(&program);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let names: Vec<&str> = result
        .program
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::Fn(f) => Some(interner.resolve(&f.name.symbol)),
            _ => None,
        })
        .collect();

    assert!(names.contains(&"generate"), "missing generate: {names:?}");
    assert!(
        names.contains(&"yelang_macro_generate"),
        "missing wrapper: {names:?}"
    );
}

#[test]
fn macro_export_reports_signature_mismatch() {
    let (program, interner) = parse_program(
        r#"
        @yelang_proc_macro::macro_export
        fn bad(x: i32) -> i32 {
            x
        }
    "#,
    );
    let mut expander = MacroExpander::new(&interner);
    let result = expander.expand(&program);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.to_string().contains("TokenStream")),
        "expected TokenStream error, got: {:?}",
        result.errors
    );
}
