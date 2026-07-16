//! Expansion of `#[yelang_proc_macro::macro_export]` and friends.
//!
//! This module scans the fully-expanded AST for functions annotated with the
//! proc-macro export attributes and emits the C ABI wrappers, descriptor table,
//! allocator, and entry point that the proc-macro server uses to load and
//! invoke the macros.

use yelang_ast::{Attribute, FnDef, FnRefType, Item, ItemKind, Program, TokenKind, Type, TypeKind};
use yelang_interner::Interner;
use yelang_lexer::Span;

/// Result of expanding proc-macro exports in a program.
pub struct ExportExpansionResult {
    pub program: Program,
    pub errors: Vec<String>,
}

/// Kind of a procedural macro being exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcMacroExportKind {
    FunctionLike,
    Attribute,
    Derive,
}

/// A collected export before code generation.
#[derive(Debug, Clone)]
struct CollectedExport {
    name: String,
}

/// Scan `program` for proc-macro export attributes and generate the dylib ABI
/// surface (wrappers, descriptors, allocator, and entry point).
pub fn expand_proc_macro_exports(program: &Program, interner: &Interner) -> ExportExpansionResult {
    let mut collector = ExportCollector::new(interner);

    for item in &program.items {
        collector.process_item(item);
    }

    collector.into_result()
}

struct ExportCollector<'a> {
    interner: &'a Interner,
    exports: Vec<CollectedExport>,
    new_items: Vec<Item>,
    errors: Vec<String>,
}

impl<'a> ExportCollector<'a> {
    fn new(interner: &'a Interner) -> Self {
        Self {
            interner,
            exports: vec![],
            new_items: vec![],
            errors: vec![],
        }
    }

    fn process_item(&mut self, item: &Item) {
        if let ItemKind::Fn(func) = &item.kind {
            if let Some((kind, stripped_item)) = self.try_strip_export_attribute(item, func) {
                if let Err(e) = self.validate_signature(kind, &stripped_item) {
                    self.errors.push(e);
                    // Keep the original item unmodified on error.
                    self.new_items.push(item.clone());
                    return;
                }

                let name = self.interner.resolve(&func.name.symbol).to_string();
                self.exports.push(CollectedExport { name: name.clone() });

                // Keep the user-written macro implementation.
                self.new_items.push(stripped_item.clone());

                // Generate the C ABI wrapper for this macro.
                self.generate_wrapper(kind, &name);

                // Generate the name static and descriptor static.
                self.generate_name_static(&name);
                self.generate_descriptor(kind, &name);
                return;
            }
        }

        self.new_items.push(item.clone());
    }

    /// If `item` carries one of the proc-macro export attributes, return the
    /// export kind and a copy of the item with that attribute removed.
    fn try_strip_export_attribute(
        &self,
        item: &Item,
        _func: &FnDef,
    ) -> Option<(ProcMacroExportKind, Item)> {
        let attr_idx = item
            .attributes
            .iter()
            .position(|attr| matches!(self.classify_export_attribute(attr), Some(_)))?;

        let attr = &item.attributes[attr_idx];
        let kind = self.classify_export_attribute(attr)?;

        let mut stripped = item.clone();
        stripped.attributes.remove(attr_idx);

        Some((kind, stripped))
    }

    fn classify_export_attribute(&self, attr: &Attribute) -> Option<ProcMacroExportKind> {
        let last = attr.path.last()?;
        let name = self.interner.resolve(&last.symbol);
        match name {
            "macro_export" => Some(ProcMacroExportKind::FunctionLike),
            "macro_export_attribute" => Some(ProcMacroExportKind::Attribute),
            "macro_export_derive" => Some(ProcMacroExportKind::Derive),
            _ => None,
        }
    }

    /// Ensure the function signature matches the export kind.
    fn validate_signature(&self, kind: ProcMacroExportKind, item: &Item) -> Result<(), String> {
        let func = match &item.kind {
            ItemKind::Fn(f) => f,
            _ => unreachable!(),
        };

        let expected_params = match kind {
            ProcMacroExportKind::FunctionLike | ProcMacroExportKind::Derive => 1,
            ProcMacroExportKind::Attribute => 2,
        };

        if func.sig.params.len() != expected_params {
            return Err(format!(
                "{} macro `{}` expects {} TokenStream parameter(s), found {}",
                kind.label(),
                self.interner.resolve(&func.name.symbol),
                expected_params,
                func.sig.params.len()
            ));
        }

        for (i, param) in func.sig.params.iter().enumerate() {
            if !self.is_token_stream(&param.ty) {
                return Err(format!(
                    "{} macro `{}` parameter {} must be `TokenStream`, found `{}`",
                    kind.label(),
                    self.interner.resolve(&func.name.symbol),
                    i,
                    self.type_to_string(&param.ty)
                ));
            }
        }

        match &func.sig.return_type {
            FnRefType::Type(ty) if self.is_token_stream(ty) => {}
            _ => {
                return Err(format!(
                    "{} macro `{}` must return `TokenStream`",
                    kind.label(),
                    self.interner.resolve(&func.name.symbol)
                ));
            }
        }

        Ok(())
    }

    fn is_token_stream(&self, ty: &Type) -> bool {
        match &ty.kind {
            TypeKind::Named(path) if path.segments.len() == 1 => {
                let name = self.interner.resolve(&path.segments[0].ident.symbol);
                name == "TokenStream"
            }
            TypeKind::Named(path) if path.segments.len() == 2 => {
                let first = self.interner.resolve(&path.segments[0].ident.symbol);
                let second = self.interner.resolve(&path.segments[1].ident.symbol);
                (first == "yelang_proc_macro" || first == "proc_macro") && second == "TokenStream"
            }
            _ => false,
        }
    }

    fn type_to_string(&self, ty: &Type) -> String {
        let mut buf = String::new();
        let _ = yelang_ast::Codegen::codegen(ty, &mut buf, self.interner);
        buf
    }

    fn generate_wrapper(&mut self, kind: ProcMacroExportKind, name: &str) {
        let wrapper_name = format!("yelang_macro_{}", name);
        let src = match kind {
            ProcMacroExportKind::FunctionLike => format!(
                "@no_mangle\n\
                 pub extern \"C-unwind\" fn {wrapper_name}(\n\
                     input: *const u8,\n\
                     input_len: usize,\n\
                     output: *mut *mut u8,\n\
                     output_len: *mut usize,\n\
                 ) {{\n\
                     yelang_proc_macro::run_fn_like_macro({name}, input, input_len, output, output_len);\n\
                 }}"
            ),
            ProcMacroExportKind::Attribute => format!(
                "@no_mangle\n\
                 pub extern \"C-unwind\" fn {wrapper_name}(\n\
                     args: *const u8,\n\
                     args_len: usize,\n\
                     item: *const u8,\n\
                     item_len: usize,\n\
                     output: *mut *mut u8,\n\
                     output_len: *mut usize,\n\
                 ) {{\n\
                     yelang_proc_macro::run_attr_macro({name}, args, args_len, item, item_len, output, output_len);\n\
                 }}"
            ),
            ProcMacroExportKind::Derive => format!(
                "@no_mangle\n\
                 pub extern \"C-unwind\" fn {wrapper_name}(\n\
                     item: *const u8,\n\
                     item_len: usize,\n\
                     output: *mut *mut u8,\n\
                     output_len: *mut usize,\n\
                 ) {{\n\
                     yelang_proc_macro::run_derive_macro({name}, item, item_len, output, output_len);\n\
                 }}"
            ),
        };

        self.parse_and_append(&src);
    }

    fn generate_name_static(&mut self, name: &str) {
        let static_name = format!("YELANG_MACRO_NAME_{}", name.to_ascii_uppercase());
        let bytes: Vec<String> = name.bytes().map(|b| b.to_string()).collect();
        let src = format!(
            "static {static_name}: [u8; {len}] = [{bytes}];",
            len = bytes.len(),
            bytes = bytes.join(", ")
        );
        self.parse_and_append(&src);
    }

    fn generate_descriptor(&mut self, kind: ProcMacroExportKind, name: &str) {
        let static_name = format!("YELANG_MACRO_DESCRIPTOR_{}", name.to_ascii_uppercase());
        let name_static = format!("YELANG_MACRO_NAME_{}", name.to_ascii_uppercase());
        let wrapper_name = format!("yelang_macro_{}", name);
        let kind_path = match kind {
            ProcMacroExportKind::FunctionLike => {
                "yelang_proc_macro::YelangProcMacroKind::FunctionLike"
            }
            ProcMacroExportKind::Attribute => "yelang_proc_macro::YelangProcMacroKind::Attribute",
            ProcMacroExportKind::Derive => "yelang_proc_macro::YelangProcMacroKind::Derive",
        };

        let (fn_like_init, attr_init, derive_init) = match kind {
            ProcMacroExportKind::FunctionLike => (
                wrapper_name.clone(),
                "0 as yelang_proc_macro::YelangAttrMacro".to_string(),
                "0 as yelang_proc_macro::YelangDeriveMacro".to_string(),
            ),
            ProcMacroExportKind::Attribute => (
                "0 as yelang_proc_macro::YelangFnLikeMacro".to_string(),
                wrapper_name.clone(),
                "0 as yelang_proc_macro::YelangDeriveMacro".to_string(),
            ),
            ProcMacroExportKind::Derive => (
                "0 as yelang_proc_macro::YelangFnLikeMacro".to_string(),
                "0 as yelang_proc_macro::YelangAttrMacro".to_string(),
                wrapper_name.clone(),
            ),
        };

        let src = format!(
            "static {static_name}: yelang_proc_macro::YelangMacroDescriptor = yelang_proc_macro::YelangMacroDescriptor {{\n\
             name: &{name_static}[0] as *const u8,\n\
             name_len: {name_len},\n\
             kind: {kind_path},\n\
             invoke: yelang_proc_macro::YelangMacroInvoke {{\n\
                 fn_like: {fn_like_init},\n\
                 attr: {attr_init},\n\
                 derive: {derive_init},\n\
             }},\n\
         }};",
            name_len = name.len(),
        );

        self.parse_and_append(&src);
    }

    fn generate_shared_items(&mut self) {
        if self.exports.is_empty() {
            return;
        }

        // Allocator / deallocator exported by every proc-macro dylib.
        self.parse_and_append(
            "@no_mangle\n\
             pub extern \"C\" fn yelang_alloc(size: usize) -> *mut u8 {\n\
                 yelang_proc_macro::bridge::alloc_output_buffer(size)\n\
             }\n\
             \n\
             @no_mangle\n\
             pub extern \"C\" fn yelang_free(ptr: *mut u8) {\n\
                 yelang_proc_macro::bridge::free_output_buffer(ptr)\n\
             }",
        );

        // Concatenate all macro descriptors into a single static array.
        let descriptors: Vec<String> = self
            .exports
            .iter()
            .map(|e| format!("YELANG_MACRO_DESCRIPTOR_{}", e.name.to_ascii_uppercase()))
            .collect();
        let count = descriptors.len();
        let descriptors_src = descriptors.join(", ");
        self.parse_and_append(&format!(
            "static YELANG_MACRO_DESCRIPTORS: [yelang_proc_macro::YelangMacroDescriptor; {count}] = [{descriptors_src}];"
        ));

        // Entry point queried by the proc-macro server.
        self.parse_and_append(&format!(
            "@no_mangle\n\
             pub extern \"C\" fn yelang_proc_macro_entry(abi_version: u32) -> *const yelang_proc_macro::YelangProcMacroExports {{\n\
                 static EXPORTS: yelang_proc_macro::YelangProcMacroExports = yelang_proc_macro::YelangProcMacroExports {{\n\
                     abi_version: yelang_proc_macro::CURRENT_ABI_VERSION,\n\
                     macro_count: {count},\n\
                     macros: &YELANG_MACRO_DESCRIPTORS[0] as *const yelang_proc_macro::YelangMacroDescriptor,\n\
                     alloc: yelang_alloc,\n\
                     free: yelang_free,\n\
                 }};\n\
                 &EXPORTS as *const yelang_proc_macro::YelangProcMacroExports\n\
             }}"
        ));
    }

    fn parse_and_append(&mut self, src: &str) {
        match parse_items_from_source(src, self.interner) {
            Ok(items) => self.new_items.extend(items),
            Err(e) => self.errors.push(format!(
                "internal error: generated proc-macro export source failed to parse: {e}\n---\n{src}\n---"
            )),
        }
    }

    fn into_result(mut self) -> ExportExpansionResult {
        self.generate_shared_items();
        ExportExpansionResult {
            program: Program {
                items: self.new_items,
                span: Span::default(),
            },
            errors: self.errors,
        }
    }
}

impl ProcMacroExportKind {
    fn label(&self) -> &'static str {
        match self {
            ProcMacroExportKind::FunctionLike => "function-like",
            ProcMacroExportKind::Attribute => "attribute",
            ProcMacroExportKind::Derive => "derive",
        }
    }
}

fn parse_items_from_source(src: &str, interner: &Interner) -> Result<Vec<Item>, String> {
    let mut local_interner = interner.clone();
    let mut stream =
        TokenKind::tokenize(src, &mut local_interner).map_err(|e| format!("tokenize: {e}"))?;
    let program = stream.parse::<Program>().map_err(|e| e.to_string())?;
    if !stream.is_eof() {
        return Err("trailing tokens after generated items".to_string());
    }
    let _ = local_interner;
    Ok(program.items)
}
