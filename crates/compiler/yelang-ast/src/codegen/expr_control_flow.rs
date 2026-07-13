use crate::expr::{ForLoopExpr, LambdaExpr, LoopExpr, MatchArm, MatchExpr, WhileExpr};
use crate::types::TypeKind;
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

// --- Match Expressions ---

impl Codegen for MatchExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "match ")?;
        self.scrutinee.codegen(f, interner)?;
        write!(f, " {{")?;
        for arm in &self.arms {
            write!(f, "\n    ")?;
            arm.pattern.codegen(f, interner)?;
            if let Some(guard) = &arm.guard {
                write!(f, " if ")?;
                guard.codegen(f, interner)?;
            }
            write!(f, " => ")?;
            arm.body.codegen(f, interner)?;
            write!(f, ",")?;
        }
        write!(f, "\n}}")
    }
}

impl Codegen for MatchArm {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.pattern.codegen(f, interner)?;
        if let Some(guard) = &self.guard {
            write!(f, " if ")?;
            guard.codegen(f, interner)?;
        }
        write!(f, " => ")?;
        self.body.codegen(f, interner)
    }
}

// --- Loop Expressions ---

impl Codegen for LoopExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if let Some(label) = &self.label {
            write!(f, "'{}: ", interner.resolve(&label.symbol))?;
        }
        write!(f, "loop ")?;
        self.body.codegen(f, interner)
    }
}

// --- While Expressions ---

impl Codegen for WhileExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "while ")?;
        self.condition.codegen(f, interner)?;
        write!(f, " ")?;
        self.body.codegen(f, interner)
    }
}

// --- For Loop Expressions ---

impl Codegen for ForLoopExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if let Some(label) = &self.label {
            write!(f, "'{}: ", interner.resolve(&label.symbol))?;
        }
        write!(f, "for ")?;
        self.pat.codegen(f, interner)?;
        write!(f, " in ")?;
        self.iter.codegen(f, interner)?;
        write!(f, " ")?;
        self.body.codegen(f, interner)
    }
}

// --- Lambda Expressions ---

impl Codegen for LambdaExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if self.fn_sig.is_async {
            write!(f, "async ")?;
        }
        write!(f, "|")?;
        for (i, param) in self.fn_sig.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            param.pattern.codegen(f, interner)?;
            if !matches!(param.ty.kind, TypeKind::Infer) {
                write!(f, ": ")?;
                param.ty.codegen(f, interner)?;
            }
        }
        write!(f, "| ")?;
        self.body.codegen(f, interner)
    }
}
