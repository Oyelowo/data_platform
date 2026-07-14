//! Struct and enum field definitions.

use yelang_ast::Ident;
use yelang_lexer::Span;

use crate::hir_ty::Ty;

/// The shape of a struct or enum variant.
#[derive(Debug, Clone)]
pub enum VariantData {
    /// Named fields: `struct Point { x: i32, y: i32 }`
    Struct { fields: Vec<FieldDef> },
    /// Tuple fields: `struct Point(i32, i32)`
    Tuple { fields: Vec<StructField> },
    /// Unit struct: `struct Point;`
    Unit,
}

/// A field in a struct with a name.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub ident: Ident,
    pub ty: Ty,
    pub span: Span,
    pub vis: crate::hir::Visibility,
}

/// A field in a tuple struct / tuple variant (positional).
#[derive(Debug, Clone)]
pub struct StructField {
    pub ty: Ty,
    pub span: Span,
    pub vis: crate::hir::Visibility,
}
