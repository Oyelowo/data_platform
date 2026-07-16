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
