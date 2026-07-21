//! Main entry point for HIR → THIR lowering.

use yelang_hir::ids::BodyId;
use yelang_hir::Crate as HirCrate;
use yelang_interner::Interner;
use yelang_resolve::lang_items::LangItems;
use yelang_tycheck::TypeckResults;

use crate::context::LoweringContext;
use crate::errors::LoweringError;
use crate::ids::ThirBodyId;

/// Lower a single HIR body to THIR.
///
/// The caller supplies the HIR crate, the type-check results for the body, the
/// language-item registry, and the string interner.
pub fn lower_body(
    hir: &HirCrate,
    typeck_results: &TypeckResults,
    lang_items: &LangItems,
    interner: &Interner,
    body_id: BodyId,
) -> Result<ThirBodyId, LoweringError> {
    let mut ctx = LoweringContext::new(hir, typeck_results, lang_items, interner);
    ctx.lower_body(body_id)
}
