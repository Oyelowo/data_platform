//! MIR terminators: control flow transfers at the end of basic blocks.

use yelang_lexer::Span;

use crate::body::BasicBlock;
use crate::ops::Operand;
use crate::place::Place;

/// A terminator with its source span.
#[derive(Debug, Clone)]
pub struct Terminator {
    /// The kind of terminator.
    pub kind: TerminatorKind,
    /// Source span for diagnostics.
    pub span: Span,
}

/// The kind of a terminator.
#[derive(Debug, Clone)]
pub enum TerminatorKind {
    /// `goto bb;` — unconditional jump.
    Goto { target: BasicBlock },

    /// `switchInt(discr) { val1 => bb1, val2 => bb2, _ => otherwise }`
    SwitchInt {
        discr: Operand,
        targets: SwitchTargets,
    },

    /// `return` — return from the function.
    Return,

    /// `call func(args) -> destination; goto target`
    Call {
        func: Operand,
        args: Vec<Operand>,
        destination: Place,
        target: BasicBlock,
    },

    /// `drop(place); goto target`
    Drop {
        place: Place,
        target: BasicBlock,
    },

    /// `assert(cond, expected); goto target`
    Assert {
        cond: Operand,
        expected: bool,
        kind: AssertKind,
        target: BasicBlock,
    },

    /// `unreachable` — indicates undefined behavior if reached.
    Unreachable,
}

impl TerminatorKind {
    /// Get all successor basic blocks.
    pub fn successors(&self) -> Vec<BasicBlock> {
        match self {
            TerminatorKind::Goto { target } => vec![*target],
            TerminatorKind::SwitchInt { targets, .. } => targets.all_targets(),
            TerminatorKind::Return => vec![],
            TerminatorKind::Call { target, .. } => vec![*target],
            TerminatorKind::Drop { target, .. } => vec![*target],
            TerminatorKind::Assert { target, .. } => vec![*target],
            TerminatorKind::Unreachable => vec![],
        }
    }
}

/// Switch targets: (value, block) pairs + an otherwise block.
#[derive(Debug, Clone)]
pub struct SwitchTargets {
    /// (value, target block) pairs.
    pub branches: Vec<(u128, BasicBlock)>,
    /// The fallback block.
    pub otherwise: BasicBlock,
}

impl SwitchTargets {
    /// All target blocks (branches + otherwise).
    pub fn all_targets(&self) -> Vec<BasicBlock> {
        let mut targets: Vec<BasicBlock> = self.branches.iter().map(|&(_, bb)| bb).collect();
        targets.push(self.otherwise);
        targets
    }
}

/// The kind of assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertKind {
    /// Array bounds check: `index < len`.
    BoundsCheck,
    /// Division by zero check.
    DivisionByZero,
    /// Remainder by zero check.
    RemainderByZero,
    /// Overflow check for arithmetic.
    Overflow(crate::body::BinOp),
    /// A custom assertion message.
    Custom(yelang_interner::Symbol),
}
