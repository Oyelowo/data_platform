use crate::{Codegen, Interner};
use crate::{FieldPattern, Mutability, Pattern, PatternKind};
use std::fmt::{self, Write};

// --- Patterns ---

impl Codegen for Pattern {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match &self.pattern {
            PatternKind::Absent => write!(f, "_"),
            PatternKind::Binding {
                name,
                mutability,
                subpattern,
            } => {
                if *mutability == Mutability::Mutable {
                    write!(f, "mut ")?;
                }
                write!(f, "{}", interner.resolve(&name.symbol))?;
                if let Some(sub) = subpattern {
                    write!(f, " @ ")?;
                    sub.codegen(f, interner)?;
                }
                Ok(())
            }
            PatternKind::Wildcard => write!(f, "_"),
            PatternKind::Path(path) => path.codegen(f, interner),
            PatternKind::Literal(lit) => lit.codegen(f, interner),
            PatternKind::Tuple { patterns } => {
                write!(f, "(")?;
                for (i, pat) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    pat.codegen(f, interner)?;
                }
                write!(f, ")")
            }
            PatternKind::Struct { path, fields, rest } => {
                path.codegen(f, interner)?;
                write!(f, " {{")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    field.codegen(f, interner)?;
                }
                if *rest {
                    write!(f, ", ..")?;
                }
                write!(f, "}}")
            }
            PatternKind::Record { fields, rest } => {
                write!(f, "{{")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    field.codegen(f, interner)?;
                }
                if *rest {
                    write!(f, ", ..")?;
                }
                write!(f, "}}")
            }
            PatternKind::TupleStruct { path, patterns } => {
                path.codegen(f, interner)?;
                write!(f, "(")?;
                for (i, pat) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    pat.codegen(f, interner)?;
                }
                write!(f, ")")
            }
            PatternKind::Slice { patterns } => {
                write!(f, "[")?;
                for (i, pat) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    pat.codegen(f, interner)?;
                }
                write!(f, "]")
            }
            PatternKind::Ref { pattern, is_mut } => {
                write!(f, "&")?;
                if *is_mut {
                    write!(f, "mut ")?;
                }
                pattern.codegen(f, interner)
            }
            PatternKind::Or(patterns) => {
                for (i, pat) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    pat.codegen(f, interner)?;
                }
                Ok(())
            }
            PatternKind::Rest { name } => {
                write!(f, "..")?;
                if let Some(name) = name {
                    write!(f, "{}", interner.resolve(&name.symbol))?;
                }
                Ok(())
            }
            PatternKind::Range(range) => range.codegen(f, interner),
            PatternKind::Grouped(pattern) => {
                write!(f, "(")?;
                pattern.codegen(f, interner)?;
                write!(f, ")")
            }
        }
    }
}

impl Codegen for FieldPattern {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if self.is_shorthand {
            write!(f, "{}", interner.resolve(&self.name.symbol))
        } else {
            write!(f, "{}: ", interner.resolve(&self.name.symbol))?;
            self.pattern.codegen(f, interner)
        }
    }
}
