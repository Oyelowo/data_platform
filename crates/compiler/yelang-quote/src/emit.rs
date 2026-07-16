//! Code generation for the `quote!` macro.

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

use crate::parse::{Fragment, Template};

/// Expand a parsed template into Rust code that builds a Yelang token stream.
pub fn expand(input: TokenStream) -> TokenStream {
    match crate::parse::parse(input) {
        Ok(template) => emit_template(&template),
        Err(message) => error_stream(&message),
    }
}

fn emit_template(template: &Template) -> TokenStream {
    let mut code = String::new();
    let mut group_counter = 0usize;
    code.push_str("{\n");
    code.push_str("    let mut __yelang_quote_stream = ::yelang_proc_macro::TokenStream::new();\n");
    emit_fragments(
        &template.fragments,
        &mut code,
        &[],
        "__yelang_quote_stream",
        &mut group_counter,
    );
    code.push_str("    __yelang_quote_stream\n");
    code.push('}');
    code.parse()
        .unwrap_or_else(|_| error_stream("generated invalid Rust code"))
}

/// Emit Rust statements that append the fragments to `stream_var`.
///
/// `loops` maps an interpolation expression (as its rendered source) to the
/// loop variable name that should be used when it appears inside a repetition.
fn emit_fragments(
    fragments: &[Fragment],
    code: &mut String,
    loops: &[(String, String)],
    stream_var: &str,
    group_counter: &mut usize,
) {
    for fragment in fragments {
        emit_fragment(fragment, code, loops, stream_var, group_counter);
    }
}

fn emit_fragment(
    fragment: &Fragment,
    code: &mut String,
    loops: &[(String, String)],
    stream_var: &str,
    group_counter: &mut usize,
) {
    match fragment {
        Fragment::Ident(text) => {
            code.push_str(&format!(
                "    {stream_var}.push(::yelang_proc_macro::TokenTree::Ident(\n"
            ));
            code.push_str(&format!(
                "        ::yelang_proc_macro::Ident::new({:?}, ::yelang_proc_macro::Span::call_site())\n",
                text
            ));
            code.push_str("    ));\n");
        }
        Fragment::Punct { ch, spacing } => {
            let spacing_path = spacing_path(*spacing);
            code.push_str(&format!(
                "    {stream_var}.push(::yelang_proc_macro::TokenTree::Punct(\n"
            ));
            code.push_str(&format!(
                "        ::yelang_proc_macro::Punct::new({:?}, {}, ::yelang_proc_macro::Span::call_site())\n",
                ch, spacing_path
            ));
            code.push_str("    ));\n");
        }
        Fragment::Lit(text) => {
            code.push_str(&format!(
                "    {stream_var}.push(::yelang_proc_macro::TokenTree::Literal(\n"
            ));
            code.push_str(&format!(
                "        ::yelang_proc_macro::Literal::from_source_text({:?}, ::yelang_proc_macro::Span::call_site())\n",
                text
            ));
            code.push_str("    ));\n");
        }
        Fragment::Group { delimiter, inner } => {
            let delim_path = delimiter_path(*delimiter);
            *group_counter += 1;
            let group_var = format!("__yelang_quote_group{}", group_counter);
            code.push_str("    {\n");
            code.push_str(&format!(
                "        let mut {group_var} = ::yelang_proc_macro::TokenStream::new();\n"
            ));
            emit_fragments(inner, code, loops, &group_var, group_counter);
            code.push_str(&format!(
                "        {stream_var}.push(::yelang_proc_macro::TokenTree::Group(\n"
            ));
            code.push_str(&format!(
                "            ::yelang_proc_macro::Group::new({delim_path}, {group_var}, ::yelang_proc_macro::Span::call_site())\n"
            ));
            code.push_str("        ));\n");
            code.push_str("    }\n");
        }
        Fragment::Interpolate { expr } => {
            let expr_string = tokens_to_string(expr);
            if let Some((_, var)) = loops.iter().rev().find(|(e, _)| *e == expr_string) {
                code.push_str(&format!(
                    "    ::yelang_proc_macro::ToTokens::to_tokens(&{var}, &mut {stream_var});\n"
                ));
            } else {
                code.push_str(&format!(
                    "    ::yelang_proc_macro::ToTokens::to_tokens(&({expr_string}), &mut {stream_var});\n"
                ));
            }
        }
        Fragment::Repeat {
            iterable,
            inner,
            separator,
        } => {
            let depth = loops.len();
            let var = format!("__yelang_quote_item{depth}");
            let first_flag = format!("__yelang_quote_first{depth}");
            let iterable_string = tokens_to_string(iterable);
            code.push_str(&format!("    let mut {first_flag} = true;\n"));
            code.push_str(&format!(
                "    for {var} in ({iterable_string}).into_iter() {{\n"
            ));
            if !separator.is_empty() {
                code.push_str(&format!("        if !{first_flag} {{\n"));
                let sep_expr = separator_expr(separator);
                code.push_str(&format!(
                    "            {stream_var}.extend(::yelang_proc_macro::TokenStream::from_iter({sep_expr}));\n"
                ));
                code.push_str("        }\n");
            }
            code.push_str(&format!("        {first_flag} = false;\n"));
            let mut next_loops = loops.to_vec();
            next_loops.push((iterable_string, var.clone()));
            emit_fragments(inner, code, &next_loops, stream_var, group_counter);
            code.push_str("    }\n");
        }
    }
}

fn tokens_to_string(tokens: &[TokenTree]) -> String {
    tokens
        .iter()
        .map(|tt| tt.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn delimiter_path(delimiter: Delimiter) -> &'static str {
    match delimiter {
        Delimiter::Parenthesis => "::yelang_proc_macro::Delimiter::Parenthesis",
        Delimiter::Brace => "::yelang_proc_macro::Delimiter::Brace",
        Delimiter::Bracket => "::yelang_proc_macro::Delimiter::Bracket",
        Delimiter::None => "::yelang_proc_macro::Delimiter::None",
    }
}

fn spacing_path(spacing: Spacing) -> &'static str {
    match spacing {
        Spacing::Alone => "::yelang_proc_macro::Spacing::Alone",
        Spacing::Joint => "::yelang_proc_macro::Spacing::Joint",
    }
}

fn separator_expr(separator: &[TokenTree]) -> String {
    let parts: Vec<String> = separator.iter().map(|tt| token_expr(tt)).collect();
    if parts.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", parts.join(", "))
    }
}

fn token_expr(tt: &TokenTree) -> String {
    match tt {
        TokenTree::Group(g) => {
            let inner: Vec<String> = g.stream().into_iter().map(|t| token_expr(&t)).collect();
            format!(
                "::yelang_proc_macro::TokenTree::Group(::yelang_proc_macro::Group::new({}, ::yelang_proc_macro::TokenStream::from_iter([{}]), ::yelang_proc_macro::Span::call_site()))",
                delimiter_path(g.delimiter()),
                inner.join(", ")
            )
        }
        TokenTree::Ident(i) => format!(
            "::yelang_proc_macro::TokenTree::Ident(::yelang_proc_macro::Ident::new({:?}, ::yelang_proc_macro::Span::call_site()))",
            i.to_string()
        ),
        TokenTree::Punct(p) => format!(
            "::yelang_proc_macro::TokenTree::Punct(::yelang_proc_macro::Punct::new({:?}, {}, ::yelang_proc_macro::Span::call_site()))",
            p.as_char(),
            spacing_path(p.spacing())
        ),
        TokenTree::Literal(l) => format!(
            "::yelang_proc_macro::TokenTree::Literal(::yelang_proc_macro::Literal::from_source_text({:?}, ::yelang_proc_macro::Span::call_site()))",
            l.to_string()
        ),
    }
}

fn error_stream(message: &str) -> TokenStream {
    let body: TokenStream = [TokenTree::Literal(Literal::string(message))]
        .into_iter()
        .collect();

    let group: TokenStream = [
        TokenTree::Ident(Ident::new("compile_error", Span::call_site())),
        TokenTree::Punct(Punct::new('!', Spacing::Alone)),
        TokenTree::Group(Group::new(Delimiter::Parenthesis, body)),
        TokenTree::Punct(Punct::new(';', Spacing::Alone)),
    ]
    .into_iter()
    .collect();
    group
}
