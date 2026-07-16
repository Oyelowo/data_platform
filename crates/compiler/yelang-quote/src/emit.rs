//! Code generation for the `quote!` macro.

use proc_macro::{Delimiter, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

use crate::parse::{Node, RepKind, Template};

/// Expand a parsed `quote!` template into Rust code that builds a Yelang token
/// stream.
pub fn expand(input: TokenStream) -> TokenStream {
    match crate::parse::parse(input) {
        Ok(template) => emit_template(&template, None),
        Err(message) => error_stream(&message),
    }
}

/// Expand a parsed `quote_spanned!` template.
pub fn expand_spanned(input: TokenStream) -> TokenStream {
    match crate::parse::parse_spanned(input) {
        Ok((span_expr, template)) => emit_template(&template, Some(tokens_to_string(&span_expr))),
        Err(message) => error_stream(&message),
    }
}

fn emit_template(template: &Template, span_expr: Option<String>) -> TokenStream {
    let mut code = String::new();
    code.push_str("{\n");

    // Evaluate the span expression once. For `quote!` this is just
    // `Span::call_site()`; for `quote_spanned!` it is the user-provided span.
    if let Some(span) = &span_expr {
        code.push_str(&format!(
            "    let __yelang_quote_span: ::yelang_proc_macro::Span = ({span});\n"
        ));
    } else {
        code.push_str("    let __yelang_quote_span: ::yelang_proc_macro::Span = ::yelang_proc_macro::Span::call_site();\n");
    }

    code.push_str("    let mut __yelang_quote_stream = ::yelang_proc_macro::TokenStream::new();\n");
    let mut group_counter = 0usize;
    emit_nodes(
        &template.nodes,
        &mut code,
        &[],
        "__yelang_quote_stream",
        0,
        &mut group_counter,
    );
    code.push_str("    __yelang_quote_stream\n");
    code.push('}');

    code.parse()
        .unwrap_or_else(|_| error_stream("generated invalid Rust code"))
}

/// Emit Rust statements that append `nodes` to `stream_var`.
///
/// `loops` maps interpolation expression source strings to the loop variable
/// that should be used when that expression appears inside a repetition.
/// `depth` is the current repetition nesting level, used to generate unique
/// variable names. `group_counter` is used to generate unique temporary group
/// stream variables.
fn emit_nodes(
    nodes: &[Node],
    code: &mut String,
    loops: &[(String, String)],
    stream_var: &str,
    depth: usize,
    group_counter: &mut usize,
) {
    for node in nodes {
        emit_node(node, code, loops, stream_var, depth, group_counter);
    }
}

fn emit_node(
    node: &Node,
    code: &mut String,
    loops: &[(String, String)],
    stream_var: &str,
    depth: usize,
    group_counter: &mut usize,
) {
    match node {
        Node::Literal(tt) => {
            emit_literal_token(tt, code, stream_var, group_counter);
        }
        Node::Group { delimiter, nodes } => {
            let delim_path = delimiter_path(*delimiter);
            *group_counter += 1;
            let group_var = format!("__yelang_quote_group{group_counter}");
            code.push_str("    {\n");
            code.push_str(&format!(
                "        let mut {group_var} = ::yelang_proc_macro::TokenStream::new();\n"
            ));
            emit_nodes(nodes, code, loops, &group_var, depth, group_counter);
            code.push_str(&format!(
                "        {stream_var}.push(::yelang_proc_macro::TokenTree::Group(\n"
            ));
            code.push_str(&format!(
                "            ::yelang_proc_macro::Group::new({delim_path}, {group_var}, __yelang_quote_span)\n"
            ));
            code.push_str("        ));\n");
            code.push_str("    }\n");
        }
        Node::Interpolate { expr } => {
            let expr_string = tokens_to_string(expr);
            if let Some((_, var)) = loops.iter().rev().find(|(e, _)| *e == expr_string) {
                code.push_str(&format!(
                    "    ::yelang_proc_macro::ToTokens::to_tokens({var}, &mut {stream_var});\n"
                ));
            } else {
                code.push_str(&format!(
                    "    ::yelang_proc_macro::ToTokens::to_tokens(&({expr_string}), &mut {stream_var});\n"
                ));
            }
        }
        Node::Repetition {
            body,
            separator,
            kind,
        } => {
            emit_repetition(
                body,
                separator,
                *kind,
                code,
                loops,
                stream_var,
                depth,
                group_counter,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_repetition(
    body: &[Node],
    separator: &Option<Punct>,
    kind: RepKind,
    code: &mut String,
    loops: &[(String, String)],
    stream_var: &str,
    depth: usize,
    group_counter: &mut usize,
) {
    // Collect every interpolation expression appearing in the body. These are
    // the iterables that drive the repetition.
    let interpolations = collect_unique_interpolations(body);
    assert!(
        !interpolations.is_empty(),
        "parser should have rejected repetition without interpolation"
    );

    let prefix = format!("__yelang_quote_d{depth}");

    // Bind each iterable to a Vec so we can iterate by index. We use `.iter()`
    // rather than `.into_iter()` so the original collection is borrowed, not
    // consumed; this is essential for nested repetitions that reference the
    // same outer iterable multiple times. `ToTokens` is implemented for
    // references, so the loop variable can be `&T`.
    for (i, expr) in interpolations.iter().enumerate() {
        code.push_str(&format!(
            "    let {prefix}_iter{i} = ({expr}).iter().collect::<::std::vec::Vec<_>>();\n"
        ));
    }

    // Length check: all iterables must have the same length.
    code.push_str(&format!("    let {prefix}_len = {prefix}_iter0.len();\n"));
    for i in 1..interpolations.len() {
        code.push_str(&format!(
            "    if {prefix}_len != {prefix}_iter{i}.len() {{\n"
        ));
        code.push_str(&format!(
            "        panic!(\"`quote!` repetition interpolations have different lengths ({{}} vs {{}})\", {prefix}_len, {prefix}_iter{i}.len());\n"
        ));
        code.push_str("    }\n");
    }

    // `+` requires a non-empty iterator.
    if kind == RepKind::Plus {
        code.push_str(&format!("    if {prefix}_len == 0 {{\n"));
        code.push_str(
            "        panic!(\"`quote!` `+` repetition requires a non-empty iterator\");\n",
        );
        code.push_str("    }\n");
    }

    // Separator flag.
    let first_flag = format!("{prefix}_first");
    code.push_str(&format!("    let mut {first_flag} = true;\n"));

    // Loop by index so every interpolation gets the same position.
    code.push_str(&format!("    for {prefix}_idx in 0..{prefix}_len {{\n"));
    for (i, _) in interpolations.iter().enumerate() {
        code.push_str(&format!(
            "        let {prefix}_i{i} = &{prefix}_iter{i}[{prefix}_idx];\n"
        ));
    }

    // Emit separator before each iteration except the first.
    if let Some(sep) = separator {
        let ch = sep.as_char();
        // A separator is always emitted as a standalone punct, regardless of
        // the spacing it had in the template (where it may have looked joint
        // with the trailing `*`/`+`).
        code.push_str(&format!("        if !{first_flag} {{\n"));
        code.push_str(&format!(
            "            {stream_var}.push(::yelang_proc_macro::TokenTree::Punct(\n"
        ));
        code.push_str(&format!(
            "                ::yelang_proc_macro::Punct::new({ch:?}, ::yelang_proc_macro::Spacing::Alone, __yelang_quote_span)\n"
        ));
        code.push_str("            ));\n");
        code.push_str("        }\n");
    }
    code.push_str(&format!("        {first_flag} = false;\n"));

    // Emit the body with loop-variable substitutions.
    let mut next_loops = loops.to_vec();
    for (i, expr) in interpolations.iter().enumerate() {
        next_loops.push((expr.clone(), format!("{prefix}_i{i}")));
    }
    emit_nodes(
        body,
        code,
        &next_loops,
        stream_var,
        depth + 1,
        group_counter,
    );

    code.push_str("    }\n");
}

fn collect_unique_interpolations(nodes: &[Node]) -> Vec<String> {
    let mut seen = Vec::new();
    for node in nodes {
        collect_interpolations(node, &mut seen);
    }
    seen
}

fn collect_interpolations(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::Interpolate { expr } => {
            let s = tokens_to_string(expr);
            if !out.contains(&s) {
                out.push(s);
            }
        }
        Node::Repetition { body, .. } | Node::Group { nodes: body, .. } => {
            for child in body {
                collect_interpolations(child, out);
            }
        }
        Node::Literal(_) => {}
    }
}

fn emit_literal_token(
    tt: &TokenTree,
    code: &mut String,
    stream_var: &str,
    group_counter: &mut usize,
) {
    match tt {
        TokenTree::Group(g) => {
            let delim_path = delimiter_path(g.delimiter());
            *group_counter += 1;
            let group_var = format!("__yelang_quote_group{group_counter}");
            code.push_str("    {\n");
            code.push_str(&format!(
                "        let mut {group_var} = ::yelang_proc_macro::TokenStream::new();\n"
            ));
            for inner in g.stream() {
                emit_literal_token(&inner, code, &group_var, group_counter);
            }
            code.push_str(&format!(
                "        {stream_var}.push(::yelang_proc_macro::TokenTree::Group(\n"
            ));
            code.push_str(&format!(
                "            ::yelang_proc_macro::Group::new({delim_path}, {group_var}, __yelang_quote_span)\n"
            ));
            code.push_str("        ));\n");
            code.push_str("    }\n");
        }
        TokenTree::Ident(i) => {
            let text = i.to_string();
            code.push_str(&format!(
                "    {stream_var}.push(::yelang_proc_macro::TokenTree::Ident(\n"
            ));
            code.push_str(&format!(
                "        ::yelang_proc_macro::Ident::new({text:?}, __yelang_quote_span)\n"
            ));
            code.push_str("    ));\n");
        }
        TokenTree::Punct(p) => {
            let ch = p.as_char();
            let spacing_path = spacing_path(p.spacing());
            code.push_str(&format!(
                "    {stream_var}.push(::yelang_proc_macro::TokenTree::Punct(\n"
            ));
            code.push_str(&format!(
                "        ::yelang_proc_macro::Punct::new({ch:?}, {spacing_path}, __yelang_quote_span)\n"
            ));
            code.push_str("    ));\n");
        }
        TokenTree::Literal(l) => {
            let text = l.to_string();
            code.push_str(&format!(
                "    {stream_var}.push(::yelang_proc_macro::TokenTree::Literal(\n"
            ));
            code.push_str(&format!(
                "        ::yelang_proc_macro::Literal::from_source_text({text:?}, __yelang_quote_span)\n"
            ));
            code.push_str("    ));\n");
        }
    }
}

fn tokens_to_string(stream: &TokenStream) -> String {
    stream
        .clone()
        .into_iter()
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

fn error_stream(message: &str) -> TokenStream {
    let body: TokenStream = [TokenTree::Literal(Literal::string(message))]
        .into_iter()
        .collect();

    let group: TokenStream = [
        TokenTree::Ident(proc_macro::Ident::new("compile_error", Span::call_site())),
        TokenTree::Punct(Punct::new('!', Spacing::Alone)),
        TokenTree::Group(proc_macro::Group::new(Delimiter::Parenthesis, body)),
        TokenTree::Punct(Punct::new(';', Spacing::Alone)),
    ]
    .into_iter()
    .collect();
    group
}
