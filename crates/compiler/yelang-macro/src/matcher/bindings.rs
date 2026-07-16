use std::collections::HashMap;
use yelang_interner::Symbol;
use yelang_macro_core::token_tree::TokenStream;

use super::types::FragmentFields;

/// A captured value for a metavariable.
#[derive(Debug, Clone, PartialEq)]
pub enum Binding {
    /// A single capture (the default for non-repeated metavariables).
    Single {
        stream: TokenStream,
        fields: Option<FragmentFields>,
    },
    /// A repeated capture: one binding per iteration.
    Repeat(Vec<Binding>),
}

impl Binding {
    /// Create a single capture with no fragment fields (e.g. for `:tt`).
    pub fn single(stream: TokenStream) -> Self {
        Binding::Single {
            stream,
            fields: None,
        }
    }

    /// Create a single capture with pre-extracted fragment fields.
    pub fn fragment(stream: TokenStream, fields: FragmentFields) -> Self {
        Binding::Single {
            stream,
            fields: Some(fields),
        }
    }

    /// Expect this binding to be a single capture, returning its token stream.
    pub fn expect_single(&self, name: &str) -> Result<TokenStream, String> {
        match self {
            Binding::Single { stream, .. } => Ok(stream.clone()),
            Binding::Repeat(_) => Err(format!(
                "metavariable `{}` is repeated but used outside a repetition",
                name
            )),
        }
    }

    /// Expect this binding to be a repeated capture.
    #[allow(dead_code)] // Used by tests and available for consumers inspecting repetitions.
    pub fn expect_repeat(&self, name: &str) -> Result<&Vec<Binding>, String> {
        match self {
            Binding::Repeat(bindings) => Ok(bindings),
            Binding::Single { .. } => Err(format!(
                "metavariable `{}` is not repeated but used inside a repetition",
                name
            )),
        }
    }

    /// Look up a fragment field on a single capture.
    pub fn expect_field(&self, name: &str, field: &str) -> Result<TokenStream, String> {
        match self {
            Binding::Single { fields, .. } => {
                let fields = fields.as_ref().ok_or_else(|| {
                    format!("metavariable `{}` does not support fragment fields", name)
                })?;
                let stream = match field {
                    // `.name` is overloaded: identifier name, type name, or item name.
                    "name" => fields
                        .name
                        .clone()
                        .or_else(|| fields.type_name.clone())
                        .or_else(|| fields.item_name.clone()),
                    "type" | "ty" => fields.ty.clone(),
                    "type_name" | "typename" => fields.type_name.clone(),
                    "type_args" | "typeargs" | "args" => fields.type_args.clone(),
                    "vis" | "visibility" => fields.vis.clone(),
                    "item_name" => fields.item_name.clone(),
                    "attrs" | "attributes" => fields.attrs.clone(),
                    _ => None,
                };
                stream.ok_or_else(|| {
                    format!(
                        "fragment field `{}` is not available for metavariable `{}`",
                        field, name
                    )
                })
            }
            Binding::Repeat(_) => Err(format!(
                "metavariable `{}` is repeated but used outside a repetition",
                name
            )),
        }
    }
}

/// A map from metavariable name to captured binding.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bindings {
    map: HashMap<Symbol, Binding>,
}

impl Bindings {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: Symbol, binding: Binding) {
        self.map.insert(name, binding);
    }

    pub fn get(&self, name: Symbol) -> Option<&Binding> {
        self.map.get(&name)
    }

    pub fn extend(&mut self, other: Bindings) {
        self.map.extend(other.map);
    }

    /// Merge the per-iteration bindings of a repetition into a single binding.
    ///
    /// For each metavariable captured inside the repeated sub-matcher, this
    /// collects the captures across all iterations into a `Binding::Repeat`.
    pub fn from_repeat_iterations(iterations: Vec<Bindings>) -> Self {
        let mut out = Bindings::new();
        for (idx, iter_bindings) in iterations.iter().enumerate() {
            for (name, binding) in iter_bindings.map.iter() {
                match out.map.get_mut(name) {
                    Some(Binding::Repeat(list)) => list.push(binding.clone()),
                    Some(_) => {
                        // Should not happen: mixed binding types for the same name.
                    }
                    None => {
                        // Pre-fill with empty repeat if this is not the first iteration,
                        // then push previous missing captures as empty singles.
                        let mut list = Vec::with_capacity(iterations.len());
                        for _ in 0..idx {
                            list.push(Binding::single(TokenStream::new()));
                        }
                        list.push(binding.clone());
                        out.map.insert(*name, Binding::Repeat(list));
                    }
                }
            }
        }

        // Ensure every iteration contributes a binding for every name, even if
        // a given name was not captured in a particular iteration.
        let names: Vec<Symbol> = out.map.keys().copied().collect();
        for name in names {
            if let Some(Binding::Repeat(list)) = out.map.get_mut(&name) {
                for iter_bindings in iterations.iter().skip(list.len()) {
                    if iter_bindings.map.contains_key(&name) {
                        list.push(iter_bindings.map[&name].clone());
                    } else {
                        list.push(Binding::single(TokenStream::new()));
                    }
                }
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_interner::Interner;
    use yelang_macro_core::token_tree::{Ident, Span, TokenTree};

    fn ident_binding(name: &str, interner: &Interner) -> Binding {
        Binding::single(TokenStream::from_vec(vec![TokenTree::Ident(Ident::new(
            interner.get_or_intern(name),
            Span::default(),
        ))]))
    }

    #[test]
    fn merge_repeat_iterations() {
        let interner = Interner::new();
        let mut iter1 = Bindings::new();
        iter1.insert(interner.get_or_intern("x"), ident_binding("a", &interner));
        let mut iter2 = Bindings::new();
        iter2.insert(interner.get_or_intern("x"), ident_binding("b", &interner));

        let merged = Bindings::from_repeat_iterations(vec![iter1, iter2]);
        let binding = merged.get(interner.get_or_intern("x")).unwrap();
        assert!(matches!(binding, Binding::Repeat(_)));
        assert_eq!(binding.expect_repeat("x").unwrap().len(), 2);
    }
}
