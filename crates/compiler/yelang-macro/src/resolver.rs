use yelang_ast::{Item, ItemKind, MacroDef, ModKind, Visibility};
use yelang_interner::Interner;
use yelang_macro_core::{
    CrateId, MacroDefArena, MacroDefData, MacroDefId, MacroKind as CoreMacroKind,
};

use crate::error::ExpandError;
use crate::matcher::{DeclarativeMacro, MacroKind, MatcherError, parse_rules};
use std::collections::HashMap;

/// A single scope in the declarative-macro namespace.
#[derive(Debug, Default)]
struct Scope {
    /// Macros defined in this exact module.
    macros: HashMap<String, DeclarativeMacro>,
    /// Child scopes keyed by module name.
    children: HashMap<String, Scope>,
}

impl Scope {
    fn ensure_child(&mut self, name: &str) -> &mut Scope {
        self.children.entry(name.to_string()).or_default()
    }

    fn follow_path_mut(&mut self, path: &[String]) -> &mut Scope {
        let mut scope = self;
        for segment in path {
            scope = scope.ensure_child(segment);
        }
        scope
    }

    fn follow_path(&self, path: &[String]) -> Option<&Scope> {
        let mut scope = self;
        for segment in path {
            scope = scope.children.get(segment)?;
        }
        Some(scope)
    }
}

/// Collection of declarative macros visible during expansion.
///
/// Macros are scoped to the module in which they are defined. A macro is
/// visible in its defining module and in any child module. `pub` macros are
/// additionally visible in all ancestor modules, mirroring the way items are
/// exported outward from a module.
#[derive(Debug, Default)]
pub struct MacroResolver {
    root: Scope,
    def_arena: MacroDefArena,
    /// Crate id to assign to locally-defined macros. In multi-crate builds this
    /// is supplied by the driver; until then all macros are treated as local.
    local_crate: CrateId,
}

impl MacroResolver {
    pub fn new() -> Self {
        Self {
            root: Scope::default(),
            def_arena: MacroDefArena::new(),
            local_crate: CrateId::new(1),
        }
    }

    pub fn with_local_crate(local_crate: CrateId) -> Self {
        Self {
            root: Scope::default(),
            def_arena: MacroDefArena::new(),
            local_crate,
        }
    }

    /// Collect all `macro` definitions from the program, removing them from
    /// the item list and registering them in the appropriate module scope.
    pub fn collect_from_program(
        &mut self,
        program: &mut yelang_ast::Program,
        interner: &Interner,
    ) -> Vec<ExpandError> {
        let mut errors = Vec::new();
        let items = std::mem::take(&mut program.items);
        program.items = self.collect_items(items, interner, &mut errors, &[]);
        errors
    }

    fn collect_items(
        &mut self,
        items: Vec<Item>,
        interner: &Interner,
        errors: &mut Vec<ExpandError>,
        path: &[String],
    ) -> Vec<Item> {
        let mut kept = Vec::with_capacity(items.len());
        for item in items {
            match item.kind {
                ItemKind::MacroDef(def) => {
                    if let Err(e) = self.register_def(&def, interner, path, &item.visibility) {
                        errors.push(ExpandError::macro_def_error(
                            interner.resolve(&def.name.symbol).to_string(),
                            e.to_string(),
                            def.span,
                        ));
                    }
                }
                ItemKind::Module(mut m) => {
                    let mod_name = interner.resolve(&m.name.symbol).to_string();
                    if let ModKind::Inline { ref mut items } = m.kind {
                        let mod_items = std::mem::take(items);
                        let mut child_path = path.to_vec();
                        child_path.push(mod_name);
                        *items = self.collect_items(mod_items, interner, errors, &child_path);
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

    fn register_def(
        &mut self,
        def: &MacroDef,
        interner: &Interner,
        path: &[String],
        visibility: &Visibility,
    ) -> Result<(), MatcherError> {
        let name = interner.resolve(&def.name.symbol).to_string();
        let rules = parse_rules(&def.body, interner)?;
        let kind = if rules.iter().any(|r| r.kind == MacroKind::Attribute) {
            CoreMacroKind::Attribute
        } else if rules.iter().any(|r| r.kind == MacroKind::Derive) {
            CoreMacroKind::Derive
        } else {
            CoreMacroKind::Declarative
        };
        let def_id = MacroDefId::from_arena_key(self.def_arena.insert(MacroDefData {
            name: def.name.symbol,
            span: def.span,
            kind,
            defining_crate: self.local_crate,
        }));
        let mac = DeclarativeMacro {
            name: name.clone(),
            rules,
            def_id,
            defining_crate: self.local_crate,
        };

        if self
            .root
            .follow_path(path)
            .is_some_and(|s| s.macros.contains_key(&name))
        {
            return Err(MatcherError::InvalidMatcher(format!(
                "duplicate macro definition `{}` in module `{}`",
                name,
                if path.is_empty() {
                    "crate root".to_string()
                } else {
                    path.join("::")
                }
            )));
        }

        self.root
            .follow_path_mut(path)
            .macros
            .insert(name.clone(), mac.clone());

        // Public macros are also visible from every ancestor scope.
        if visibility.is_public() {
            for i in 0..path.len() {
                self.root
                    .follow_path_mut(&path[..i])
                    .macros
                    .insert(name.clone(), mac.clone());
            }
        }

        Ok(())
    }

    /// Resolve a macro name from the given module path, walking up through
    /// parent scopes until a match is found.
    pub fn resolve(&self, name: &str, module_path: &[String]) -> Option<&DeclarativeMacro> {
        let mut scope = &self.root;
        // Check the root scope first (path is empty) and then each successive
        // child scope. This finds the nearest definition in the lexical scope.
        for segment in module_path.iter() {
            if let Some(mac) = scope.macros.get(name) {
                return Some(mac);
            }
            scope = scope.children.get(segment)?;
        }
        scope.macros.get(name)
    }

    pub fn macro_def_data(&self, def_id: MacroDefId) -> Option<&MacroDefData> {
        self.def_arena.get(def_id.as_arena_key())
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
        assert!(resolver.resolve("id", &[]).is_some());
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
        assert!(resolver.resolve("id", &["inner".to_string()]).is_some());
        assert!(resolver.resolve("id", &[]).is_none());
    }

    #[test]
    fn resolver_public_macro_visible_in_parent_scope() {
        let (mut program, interner) = parse_program(
            r#"
            mod inner {
                pub macro id { ($x:expr) => { $x }; }
            }
            fn main() {}
        "#,
        );
        let mut resolver = MacroResolver::new();
        let errors = resolver.collect_from_program(&mut program, &interner);
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(resolver.resolve("id", &["inner".to_string()]).is_some());
        assert!(resolver.resolve("id", &[]).is_some());
    }

    #[test]
    fn resolver_private_macro_not_visible_in_sibling_scope() {
        let (mut program, interner) = parse_program(
            r#"
            mod a {
                macro id { ($x:expr) => { $x }; }
            }
            mod b {
                fn dummy() {}
            }
        "#,
        );
        let mut resolver = MacroResolver::new();
        let errors = resolver.collect_from_program(&mut program, &interner);
        assert!(errors.is_empty(), "{:?}", errors);
        assert!(resolver.resolve("id", &["a".to_string()]).is_some());
        assert!(resolver.resolve("id", &["b".to_string()]).is_none());
    }
}
