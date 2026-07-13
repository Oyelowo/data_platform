use crate::expr::{Array, BlockExpr, CallExpr, IfExpr, Object, Path, UnaryExpr};
use crate::{
    AngleBracketedArg, AngleBracketedArgs, CallArgument, GenericArgs, Literal, PathSegment, UnaryOp,
};
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

// --- Literals ---

impl Codegen for Literal {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            Literal::Int(i) => write!(f, "{}", interner.resolve(&i.value)),
            Literal::Str(s) => write!(f, "\"{}\"", interner.resolve(&s.value)),
            Literal::Bool(b) => write!(f, "{}", b),
            Literal::Float(fl) => write!(f, "{}", interner.resolve(&fl.value)),
            _ => write!(f, "literal"),
        }
    }
}

impl Codegen for UnaryExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self.op {
            UnaryOp::Bang => write!(f, "!")?,
            UnaryOp::Minus => write!(f, "-")?,
        }
        self.expr.codegen(f, interner)
    }
}

// --- Call Expressions ---

impl Codegen for CallArgument {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            CallArgument::Positional(expr) => expr.codegen(f, interner),
            CallArgument::Named(name, expr) => {
                name.codegen(f, interner)?;
                write!(f, ": ")?;
                expr.codegen(f, interner)
            }
        }
    }
}

impl Codegen for CallExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.callee.codegen(f, interner)?;
        write!(f, "(")?;
        for (i, arg) in self.args.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            match arg {
                CallArgument::Positional(expr) => expr.codegen(f, interner)?,
                CallArgument::Named(ident, expr) => {
                    write!(f, "{}: ", interner.resolve(&ident.symbol))?;
                    expr.codegen(f, interner)?;
                }
            }
        }
        write!(f, ")")
    }
}

// --- Arrays ---

impl Codegen for Array {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        use crate::expr::ArrayKind;
        match &self.kind {
            ArrayKind::List(elements) => {
                write!(f, "[")?;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    elem.codegen(f, interner)?;
                }
                write!(f, "]")
            }
            ArrayKind::Repeat { value, count } => {
                write!(f, "[")?;
                value.codegen(f, interner)?;
                write!(f, "; ")?;
                count.codegen(f, interner)?;
                write!(f, "]")
            }
        }
    }
}

// --- Objects ---

impl Codegen for Object {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "{{")?;
        for (i, field) in self.fields.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: ", interner.resolve(&field.key.symbol))?;
            field.val.codegen(f, interner)?;
        }
        write!(f, "}}")
    }
}

// --- If Expressions ---

impl Codegen for IfExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "if ")?;
        self.condition.codegen(f, interner)?;
        write!(f, " ")?;
        self.then_block.codegen(f, interner)?;
        if let Some(else_expr) = &self.else_expr {
            write!(f, " else ")?;
            else_expr.codegen(f, interner)?;
        }
        Ok(())
    }
}

// --- Block Expressions ---

impl Codegen for BlockExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "{{")?;
        for stmt in &self.statements {
            write!(f, "\n    ")?;
            stmt.codegen(f, interner)?;
        }
        if self.statements.is_empty() {
            write!(f, "}}")
        } else {
            write!(f, "\n}}")
        }
    }
}

// --- Paths ---

impl Codegen for Path {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        if let Some(qself) = &self.qself {
            write!(f, "<")?;
            qself.ty.codegen(f, interner)?;
            if let Some(trait_path) = &qself.as_trait {
                write!(f, " as ")?;
                trait_path.codegen(f, interner)?;
            }
            write!(f, ">::")?;

            for (i, segment) in self.segments.iter().enumerate() {
                if i > 0 {
                    write!(f, "::")?;
                }
                write!(f, "{}", interner.resolve(&segment.ident.symbol))?;
                if let Some(args) = &segment.args {
                    args.codegen(f, interner)?;
                }
            }
            return Ok(());
        }

        if self.is_absolute {
            write!(f, "::")?;
        }
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                write!(f, "::")?;
            }
            write!(f, "{}", interner.resolve(&segment.ident.symbol))?;

            // Handle generic arguments if present
            if let Some(args) = &segment.args {
                args.codegen(f, interner)?;
            }
        }
        if self.segments.is_empty() {
            write!(f, "/* empty path */")?;
        }
        Ok(())
    }
}

impl Codegen for PathSegment {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        self.ident.codegen(f, interner)?;
        if let Some(args) = &self.args {
            args.codegen(f, interner)?;
        }
        Ok(())
    }
}

impl Codegen for GenericArgs {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            GenericArgs::AngleBracketed(args) => args.codegen(f, interner),
            GenericArgs::Parenthesized(args) => args.codegen(f, interner),
        }
    }
}

impl Codegen for AngleBracketedArgs {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        write!(f, "<")?;
        for (i, arg) in self.args.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            arg.codegen(f, interner)?;
        }
        write!(f, ">")
    }
}

impl Codegen for AngleBracketedArg {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match self {
            AngleBracketedArg::Type(ty) => ty.codegen(f, interner),
            AngleBracketedArg::Const(expr) => expr.codegen(f, interner),
            AngleBracketedArg::AssociatedType { name, ty } => {
                name.codegen(f, interner)?;
                write!(f, " = ")?;
                ty.codegen(f, interner)
            }
        }
    }
}
