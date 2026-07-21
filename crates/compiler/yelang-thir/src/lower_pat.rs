//! Pattern lowering: HIR `Pat` → THIR `ThirPat`.

use yelang_hir::ids::PatId;

use crate::context::LoweringContext;
use crate::ids::ThirPatId;
use crate::pat::ThirPat;

impl<'a> LoweringContext<'a> {
    /// Lower a HIR pattern to a THIR pattern.
    pub fn lower_pat(&mut self, pat_id: PatId) -> ThirPatId {
        let Some(pat) = self.hir.pat(pat_id) else {
            return self.alloc_pat(ThirPat::Err);
        };

        let thir_pat = match pat {
            yelang_hir::hir::pat::Pat::Wild => ThirPat::Wild,

            yelang_hir::hir::pat::Pat::Binding {
                mode: _,
                name,
                subpat,
            } => ThirPat::Binding {
                name: *name,
                subpat: subpat.map(|p| self.lower_pat(p)),
            },

            yelang_hir::hir::pat::Pat::Struct { res, fields, rest } => ThirPat::Struct {
                res: *res,
                fields: fields
                    .iter()
                    .map(|f| (f.ident.symbol, self.lower_pat(f.pat)))
                    .collect(),
                rest: *rest,
            },

            yelang_hir::hir::pat::Pat::Tuple { pats } => ThirPat::Tuple {
                pats: pats.iter().map(|&p| self.lower_pat(p)).collect(),
            },

            yelang_hir::hir::pat::Pat::TupleStruct { res, pats } => ThirPat::TupleStruct {
                res: *res,
                pats: pats.iter().map(|&p| self.lower_pat(p)).collect(),
            },

            yelang_hir::hir::pat::Pat::Ref { pat, mutability } => ThirPat::Ref {
                mutability: mutability.clone(),
                pat: self.lower_pat(*pat),
            },

            yelang_hir::hir::pat::Pat::Path { res } => ThirPat::Path { res: *res },

            yelang_hir::hir::pat::Pat::Lit { lit } => ThirPat::Lit { lit: lit.clone() },

            yelang_hir::hir::pat::Pat::Range {
                start,
                end,
                end_inclusive,
            } => ThirPat::Range {
                start: start.map(|p| self.lower_pat(p)),
                end: end.map(|p| self.lower_pat(p)),
                end_inclusive: *end_inclusive,
            },

            yelang_hir::hir::pat::Pat::Or { pats } => ThirPat::Or {
                pats: pats.iter().map(|&p| self.lower_pat(p)).collect(),
            },

            yelang_hir::hir::pat::Pat::Slice {
                prefix,
                middle,
                suffix,
            } => ThirPat::Slice {
                prefix: prefix.iter().map(|&p| self.lower_pat(p)).collect(),
                middle: middle.map(|p| self.lower_pat(p)),
                suffix: suffix.iter().map(|&p| self.lower_pat(p)).collect(),
            },

            yelang_hir::hir::pat::Pat::Rest { name: _ } => ThirPat::Rest,

            yelang_hir::hir::pat::Pat::Err => ThirPat::Err,
        };

        let thir_pat_id = self.alloc_pat(thir_pat);
        if matches!(self.pats.get(thir_pat_id), Some(ThirPat::Binding { .. })) {
            self.local_pats.insert(pat_id, thir_pat_id);
        }
        thir_pat_id
    }
}
