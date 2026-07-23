//! MIR body: locals, basic blocks, and the CFG.

use yelang_arena::{Id, IndexVec};
use yelang_interner::Symbol;
use yelang_lexer::Span;
use yelang_ty::ty::TyId;

use crate::ops::Operand;
use crate::place::Place;
use crate::terminator::Terminator;

// ---------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------

/// Tag for [`Local`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagLocal;

/// A local variable in a MIR body.
///
/// Local 0 is always the return pointer. Locals 1..=arg_count are arguments.
/// Remaining locals are temporaries.
pub type Local = Id<TagLocal>;

/// Tag for [`BasicBlock`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TagBasicBlock;

/// A basic block in the control flow graph.
pub type BasicBlock = Id<TagBasicBlock>;

// ---------------------------------------------------------------------------
// Body
// ---------------------------------------------------------------------------

/// A MIR function body.
///
/// Contains locals (variables + temporaries) and basic blocks (CFG nodes).
/// The entry block is always `BasicBlock(0)`.
#[derive(Debug, Clone)]
pub struct Body {
    /// All locals in this body. Index 0 = return pointer.
    pub locals: IndexVec<Local, LocalDecl>,
    /// All basic blocks. Index 0 = entry block.
    pub basic_blocks: IndexVec<BasicBlock, BasicBlockData>,
    /// Number of argument locals (locals 1..=arg_count).
    pub arg_count: usize,
    /// The span of the source function.
    pub span: Span,
    /// Name of the function (for diagnostics).
    pub name: Option<Symbol>,
}

impl Body {
    /// Create a new body with the given number of arguments and return type.
    pub fn new(arg_count: usize, return_ty: TyId, span: Span) -> Self {
        let mut locals = IndexVec::new();
        // Local 0: return pointer.
        locals.push(LocalDecl {
            ty: return_ty,
            kind: LocalKind::ReturnPointer,
            name: None,
        });
        // Locals 1..=arg_count: arguments (type filled during lowering).
        for _ in 0..arg_count {
            locals.push(LocalDecl {
                ty: return_ty, // placeholder, filled during lowering
                kind: LocalKind::Arg,
                name: None,
            });
        }
        // Entry block.
        let mut basic_blocks = IndexVec::new();
        basic_blocks.push(BasicBlockData {
            statements: Vec::new(),
            terminator: Terminator {
                kind: crate::terminator::TerminatorKind::Unreachable,
                span,
            },
        });
        Self {
            locals,
            basic_blocks,
            arg_count,
            span,
            name: None,
        }
    }

    /// Allocate a new temporary local.
    pub fn new_temp(&mut self, ty: TyId) -> Local {
        self.locals.push(LocalDecl {
            ty,
            kind: LocalKind::Temp,
            name: None,
        })
    }

    /// Allocate a new basic block.
    pub fn new_block(&mut self) -> BasicBlock {
        self.basic_blocks.push(BasicBlockData {
            statements: Vec::new(),
            terminator: Terminator {
                kind: crate::terminator::TerminatorKind::Unreachable,
                span: self.span,
            },
        })
    }

    /// The return pointer local (always Local 0).
    pub fn return_local(&self) -> Local {
        Local::new(0)
    }

    /// The entry basic block (always BasicBlock 0).
    pub fn entry_block(&self) -> BasicBlock {
        BasicBlock::new(0)
    }

    /// Iterate over all successor blocks of a given block.
    pub fn successors(&self, block: BasicBlock) -> Vec<BasicBlock> {
        let data = &self.basic_blocks[block];
        data.terminator.kind.successors()
    }
}

// ---------------------------------------------------------------------------
// LocalDecl
// ---------------------------------------------------------------------------

/// Declaration of a local variable.
#[derive(Debug, Clone)]
pub struct LocalDecl {
    /// The type of this local.
    pub ty: TyId,
    /// What kind of local this is.
    pub kind: LocalKind,
    /// Optional source name (for diagnostics).
    pub name: Option<Symbol>,
}

/// The kind of a local variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalKind {
    /// The return pointer (Local 0).
    ReturnPointer,
    /// A function argument.
    Arg,
    /// A compiler-generated temporary.
    Temp,
}

// ---------------------------------------------------------------------------
// BasicBlockData
// ---------------------------------------------------------------------------

/// A basic block: a sequence of statements followed by a terminator.
#[derive(Debug, Clone)]
pub struct BasicBlockData {
    /// Statements executed in order.
    pub statements: Vec<Statement>,
    /// The terminator (control flow transfer).
    pub terminator: Terminator,
}

// ---------------------------------------------------------------------------
// Statement
// ---------------------------------------------------------------------------

/// A MIR statement.
#[derive(Debug, Clone)]
pub enum Statement {
    /// `_place = _rvalue`
    Assign(Place, Rvalue),
    /// No operation (placeholder for removed statements).
    Nop,
}

/// An rvalue in an assignment.
#[derive(Debug, Clone)]
pub enum Rvalue {
    /// Use an operand directly.
    Use(Operand),
    /// Binary operation: `_a OP _b`.
    BinaryOp(BinOp, Operand, Operand),
    /// Unary operation: `OP _a`.
    UnaryOp(UnOp, Operand),
    /// Reference: `&_place` or `&mut _place`.
    Ref(Place),
    /// Address of: `addr(_place)`.
    AddressOf(Place),
    /// Aggregate construction: struct, tuple, array, enum variant.
    Aggregate(AggregateKind, Vec<Operand>),
    /// Type cast: `_a as Ty`.
    Cast(Operand, TyId),
    /// Array repeat: `[_a; N]`.
    Repeat(Operand, usize),
    /// Length of a place: `len(_place)`.
    Len(Place),
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
}

/// What kind of aggregate is being constructed.
#[derive(Debug, Clone)]
pub enum AggregateKind {
    /// A struct: `StructName { field1: val1, field2: val2 }`.
    Struct(yelang_arena::DefId),
    /// A tuple: `(val1, val2, ...)`.
    Tuple,
    /// An array: `[val1, val2, ...]`.
    Array(TyId),
    /// An enum variant: `VariantName(val1, val2, ...)`.
    EnumVariant(yelang_arena::DefId, usize),
}
