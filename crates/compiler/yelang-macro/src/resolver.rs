use yelang_ast::{Item, ItemKind, MacroDef, ModKind};
use yelang_interner::Interner;

use crate::error::ExpandError;
use crate::matcher::{DeclarativeMacro, MatcherError, parse_rules};
use std::collections::HashMap;

/// Collection of declarative macros visible during expansion.
#[derive(Debug, Clone, Default)]
pub struct MacroResolver {
    macros: HashMap<String, DeclarativeMacro>,
}

impl MacroResolver {
    pub fn new() -> Self {
        Self {
            macros: HashMap::new(),
        }
    }

    /// Collect all `macro` definitions from the program, removing them from
    /// the item list and registering them by name.
    pub fn collect_from_program(
        &mut self,
        program: &mut yelang_ast::Program,
        interner: &Interner,
    ) -> Vec<ExpandError> {
        let mut errors = Vec::new();
        let items = std::mem::take(&mut program.items);
        program.items = self.collect_items(items, interner, &mut errors);
        errors
    }

    fn collect_items(
        &mut self,
        items: Vec<Item>,
        interner: &Interner,
        errors: &mut Vec<ExpandError>,
    ) -> Vec<Item> {
        let mut kept = Vec::with_capacity(items.len());
        for item in items {
            match item.kind {
                ItemKind::MacroDef(def) => {
                    if let Err(e) = self.register_def(&def, interner) {
                        errors.push(ExpandError::MacroDefError {
                            name: interner.resolve(&def.name.symbol).to_string(),
                            reason: e.to_string(),
                            span: def.span,
                        });
                    }
                }
                ItemKind::Module(mut m) => {
                    if let ModKind::Inline { ref mut items } = m.kind {
                        let mod_items = std::mem::take(items);
                        *items = self.collect_items(mod_items, interner, errors);
                    }
                    kept.push(Item {
                        kind: ItemKind::Module(m),
                        ..item
                    });
                }
                _ => kept.push(item),
            }
        }
        kept
    }

    fn register_def(&mut self, def: &MacroDef, interner: &Interner) -> Result<(), MatcherError> {
        let name = interner.resolve(&def.name.symbol).to_string();
        let rules = parse_rules(&def.body, interner)?;
        self.macros.insert(
            name,
            DeclarativeMacro {
                name: interner.resolve(&def.name.symbol).to_string(),
                rules,
            },
        );
        Ok(())
    }

    pub fn resolve(&self, name: &str) -> Option<&DeclarativeMacro> {
        self.macros.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ast::{ItemKind, TokenKind};
    use yelang_interner::Interner;

    fn parse_program(src: &str) -> (yelang_ast::Program, Interner) {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
        let program = stream.parse::<yelang_ast::Program>().unwrap();
        (program, interner)
    }

    #[test]
    fn resolver_collects_simple_macro() {
        let (mut program, interner) = parse_program(
            r#"
            macro id { ($x:expr) => { $x }; }
            fn main() {}
        "#,
        );
        let mut resolver = MacroResolver::new();
        let errors = resolver.collect_from_program(&mut program, &interner);
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(resolver.resolve("id").is_some());
        assert!(
            program
                .items
                .iter()
                .all(|i| !matches!(i.kind, ItemKind::MacroDef(_)))
        );
    }

    #[test]
    fn resolver_collects_module_local_macro() {
        let (mut program, interner) = parse_program(
            r#"
            mod inner {
                macro id { ($x:expr) => { $x }; }
            }
            fn main() {}
        "#,
        );
        let mut resolver = MacroResolver::new();
        let errors = resolver.collect_from_program(&mut program, &interner);
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(resolver.resolve("id").is_some());
    }
}
