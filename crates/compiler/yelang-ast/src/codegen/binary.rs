use crate::expr::{BinaryExpr, Expr, Precedence};
use crate::{Associativity, ExprKind, PrecedenceExt};
use crate::{Codegen, Interner};
use std::fmt::{self, Write};

// --- Binary Expressions (The Tricky Part) ---

impl Codegen for BinaryExpr {
    fn codegen(&self, f: &mut dyn Write, interner: &Interner) -> fmt::Result {
        let parent_prec = self.op.precedence();

        // Handle Left Side
        wrap_if_needed(f, interner, &self.left, parent_prec, true)?;

        // Handle operator
        write!(f, " {} ", self.op)?;

        // Handle Right Side
        wrap_if_needed(f, interner, &self.right, parent_prec, false)
    }
}

/// Helper function to determine if parentheses are needed.
/// This keeps the logic out of the BinaryExpr struct definition.
fn wrap_if_needed(
    f: &mut dyn Write,
    interner: &Interner,
    child: &Expr,
    parent_prec: Precedence,
    is_left: bool,
) -> fmt::Result {
    let needs_parens = match &child.kind {
        ExprKind::Binary(bin) => {
            let child_prec = bin.op.precedence();
            if child_prec < parent_prec {
                true
            } else if child_prec > parent_prec {
                false
            } else {
                // Precedence is equal. Check associativity.
                // If we are Left Associative (e.g. 1 - 2 - 3):
                // Left child (1-2) doesn't need parens. Right child (3) DOES need parens if it was (1-(2-3)).
                match bin.op.associativity() {
                    Associativity::Left => !is_left, // Right child needs parens
                    Associativity::Right => is_left, // Left child needs parens
                    Associativity::NonAssociative => true,
                }
            }
        }
        // You might also need to handle Ternary or Casts here if they have precedence
        _ => false,
    };

    if needs_parens {
        write!(f, "(")?;
        child.codegen(f, interner)?;
        write!(f, ")")
    } else {
        child.codegen(f, interner)
    }
}
