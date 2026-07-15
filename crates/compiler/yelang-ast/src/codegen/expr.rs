use crate::expr::{DocumentField, Expr};
use crate::{Codegen, Interner};
use crate::{ExprKind, StringPart};
use std::fmt::{self, Write};

// --- Expressions ---

impl Codegen for Expr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        match &self.kind {
            ExprKind::Literal(lit) => lit.codegen(f, interner),
            ExprKind::InterpolatedString(parts) => {
                write!(f, "\"")?;
                for part in parts {
                    match part {
                        StringPart::Literal(s) => write!(f, "{}", interner.resolve(s))?,
                        StringPart::Expr(expr) => {
                            write!(f, "${{")?;
                            expr.codegen(f, interner)?;
                            write!(f, "}}")?;
                        }
                    }
                }
                write!(f, "\"")
            }
            ExprKind::Path(path) => path.codegen(f, interner),
            ExprKind::Underscore => write!(f, "_"),
            ExprKind::Binary(bin) => bin.codegen(f, interner),
            ExprKind::Unary(unary) => unary.codegen(f, interner),
            ExprKind::AssignEq(assign) => assign.codegen(f, interner),
            ExprKind::AssignOp(assign) => assign.codegen(f, interner),
            ExprKind::DestructureAssign(assign) => assign.codegen(f, interner),
            ExprKind::Try(expr) => expr.codegen(f, interner),
            ExprKind::If(if_expr) => if_expr.codegen(f, interner),
            ExprKind::Let(let_expr) => {
                write!(f, "let ")?;
                let_expr.pattern.codegen(f, interner)?;
                write!(f, " = ")?;
                let_expr.expr.codegen(f, interner)
            }
            ExprKind::Match(match_expr) => match_expr.codegen(f, interner),
            ExprKind::Ternary(_) => write!(f, "/* ternary not implemented */"),
            ExprKind::Loop(loop_expr) => loop_expr.codegen(f, interner),
            ExprKind::While(while_expr) => while_expr.codegen(f, interner),
            ExprKind::ForLoop(for_expr) => for_expr.codegen(f, interner),
            ExprKind::Break(break_expr) => {
                write!(f, "break")?;
                if let Some(label) = &break_expr.label {
                    write!(f, " '{}'", interner.resolve(&label.symbol))?;
                }
                if let Some(val) = &break_expr.value {
                    write!(f, " ")?;
                    val.codegen(f, interner)?;
                }
                Ok(())
            }
            ExprKind::Continue(continue_expr) => {
                write!(f, "continue")?;
                if let Some(label) = &continue_expr.label {
                    write!(f, " '{}'", interner.resolve(&label.symbol))?;
                }
                Ok(())
            }
            ExprKind::Return(value) => {
                write!(f, "return")?;
                if let Some(val) = value {
                    write!(f, " ")?;
                    val.codegen(f, interner)?;
                }
                Ok(())
            }
            ExprKind::TypeCast(cast) => cast.codegen(f, interner),
            ExprKind::TypeAscription(ascription) => {
                ascription.expr.codegen(f, interner)?;
                write!(f, ": ")?;
                ascription.ty.codegen(f, interner)
            }
            ExprKind::IsType(is_type) => is_type.codegen(f, interner),
            ExprKind::Struct(struct_expr) => struct_expr.codegen(f, interner),
            ExprKind::Array(arr) => arr.codegen(f, interner),
            ExprKind::Object(obj) => obj.codegen(f, interner),
            ExprKind::Tuple(exprs) => {
                write!(f, "(")?;
                for (i, expr) in exprs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    expr.codegen(f, interner)?;
                }
                write!(f, ")")
            }
            ExprKind::Range(range) => range.codegen(f, interner),
            ExprKind::Comprehension(_) => write!(f, "/* comprehension not implemented */"),
            ExprKind::MemberAccess(access) => access.codegen(f, interner),
            ExprKind::ArrayAccess(access) => access.codegen(f, interner),
            ExprKind::DocumentAccess(doc) => {
                doc.base.codegen(f, interner)?;
                write!(f, ".{{")?;
                for (i, field) in doc.object.fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match field {
                        DocumentField::KeyVal(kv) => {
                            write!(f, "{}", interner.resolve(&kv.key.symbol))?;
                            write!(f, ": ")?;
                            kv.value.codegen(f, interner)?;
                        }
                        DocumentField::KeyOnly(ko) => {
                            write!(f, "{}", interner.resolve(&ko.key.symbol))?;
                        }
                        DocumentField::Spread(sp) => {
                            write!(f, "..")?;
                            sp.expr.codegen(f, interner)?;
                        }
                    }
                }
                write!(f, "}}")
            }
            ExprKind::BindAt(bind) => bind.codegen(f, interner),
            ExprKind::Call(call) => call.codegen(f, interner),
            ExprKind::MethodCall(method) => method.codegen(f, interner),
            ExprKind::Lambda(lambda) => lambda.codegen(f, interner),
            ExprKind::Block(block) => block.codegen(f, interner),
            ExprKind::Query(query) => query.codegen(f, interner),
            ExprKind::Grouped(g) => {
                write!(f, "(")?;
                g.expr.codegen(f, interner)?;
                write!(f, ")")
            }
            ExprKind::Async(async_expr) => async_expr.codegen(f, interner),
            ExprKind::Gen(expr) => {
                write!(f, "gen ")?;
                expr.codegen(f, interner)
            }
            ExprKind::Await(expr) => {
                expr.codegen(f, interner)?;
                write!(f, ".await")
            }
            ExprKind::MacroInvocation(inv) => {
                inv.path.codegen(f, interner)?;
                write!(f, "!")?;
                inv.args.codegen(f, interner)
            }
            ExprKind::Err => write!(f, "/* error */"),
            ExprKind::Dummy => write!(f, "/* dummy */"),
        }
    }
}
