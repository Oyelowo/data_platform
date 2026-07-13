use crate::expr::{
    ArrayAccess, ArrayIndex, AssignEqExpr, AssignOpExpr, AsyncExpr, BindAtExpr,
    DestructureAssignExpr, Index, IsTypeExpr, MemberAccess, MethodCallExpr, RangeExpr, RangeItem,
    RangeOp, StructExpr, TrySafeAccess, TypeCast,
};
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

// --- Range Expressions ---

impl Codegen for RangeExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if let Some(start) = &self.start {
            start.codegen(f, interner)?;
        }
        match self.op {
            RangeOp::Exclusive => write!(f, "..")?,
            RangeOp::Inclusive => write!(f, "..=")?,
        }
        if let Some(end) = &self.end {
            end.codegen(f, interner)?;
        }
        Ok(())
    }
}

// --- Member Access ---

impl Codegen for MemberAccess {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.base.codegen(f, interner)?;
        write!(f, ".{}", interner.resolve(&self.member.symbol))
    }
}

// --- Array Access ---

impl Codegen for ArrayAccess {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.base.codegen(f, interner)?;
        write!(f, "[")?;
        self.index.codegen(f, interner)?;
        write!(f, "]")
    }
}

impl Codegen for ArrayIndex {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            ArrayIndex::Single(index) => index.codegen(f, interner),
            ArrayIndex::Range(range) => range.codegen(f, interner),
            ArrayIndex::Stars { stars } => write!(f, "{}", "*".repeat(*stars)),
            ArrayIndex::Filter(expr) => {
                write!(f, "where ")?;
                expr.codegen(f, interner)
            }
            ArrayIndex::OrderBy(clause) => {
                write!(f, "order by ")?;
                for (idx, part) in clause.orders.iter().enumerate() {
                    if idx > 0 {
                        write!(f, ", ")?;
                    }
                    part.codegen(f, interner)?;
                }
                Ok(())
            }
            ArrayIndex::GroupBy(selector) => {
                write!(f, "group by {{")?;
                for (idx, key) in selector.keys.iter().enumerate() {
                    if idx > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: ", interner.resolve(&key.name.symbol))?;
                    key.expr.codegen(f, interner)?;
                }
                write!(f, "}}")
            }
            ArrayIndex::Enumerate => write!(f, "enumerate"),
            ArrayIndex::Distinct => write!(f, "distinct"),
            ArrayIndex::DistinctBy(expr) => {
                write!(f, "distinct by ")?;
                expr.codegen(f, interner)
            }
        }
    }
}

impl Codegen for Index {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.0.codegen(f, interner)
    }
}

impl Codegen for RangeItem {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if let Some(start) = &self.start {
            start.codegen(f, interner)?;
        }
        if self.inclusive {
            write!(f, "..=")?;
        } else {
            write!(f, "..")?;
        }
        if let Some(end) = &self.end {
            end.codegen(f, interner)?;
        }
        Ok(())
    }
}

// --- Method Call ---

impl Codegen for MethodCallExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.receiver.codegen(f, interner)?;
        write!(f, ".")?;
        self.segment.codegen(f, interner)?;
        write!(f, "(")?;
        for (i, arg) in self.arguments.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            arg.codegen(f, interner)?;
        }
        write!(f, ")")
    }
}

// --- Try (`?`) ---

impl Codegen for TrySafeAccess {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.base.codegen(f, interner)?;
        write!(f, "?")
    }
}

// --- Assignment ---

impl Codegen for AssignEqExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.target.codegen(f, interner)?;
        write!(f, " = ")?;
        self.value.codegen(f, interner)
    }
}

impl Codegen for AssignOpExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.target.codegen(f, interner)?;
        write!(f, " {} ", self.op)?;
        self.value.codegen(f, interner)
    }
}

impl Codegen for DestructureAssignExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.pattern.codegen(f, interner)?;
        write!(f, " = ")?;
        self.value.codegen(f, interner)
    }
}

// --- Type Cast ---

impl Codegen for TypeCast {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.base.codegen(f, interner)?;
        write!(f, " as ")?;
        self.ty.codegen(f, interner)
    }
}

// --- Is Type ---

impl Codegen for IsTypeExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.expr.codegen(f, interner)?;
        write!(f, " is ")?;
        self.ty.codegen(f, interner)
    }
}

// --- Struct Expression ---

impl Codegen for StructExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.path.codegen(f, interner)?;
        write!(f, " {{")?;
        for (i, field) in self.fields.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: ", interner.resolve(&field.name.symbol))?;
            field.value.codegen(f, interner)?;
        }
        if let Some(rest) = &self.rest {
            write!(f, ", ..")?;
            rest.codegen(f, interner)?;
        }
        write!(f, "}}")
    }
}

// --- Bind At ---

impl Codegen for BindAtExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.base.codegen(f, interner)?;
        write!(f, "@{}", interner.resolve(&self.at.symbol))
    }
}

// --- Async Expression ---

impl Codegen for AsyncExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "async ")?;
        self.block.codegen(f, interner)
    }
}
