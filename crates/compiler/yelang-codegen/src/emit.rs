//! IR emission abstraction.
//!
//! The `IrEmitter` trait is the interface for emitting low-level IR.
//! Implementations target specific backends: LLVM, Cranelift, or a
//! custom database-specific IR (like Umbra's).
//!
//! The emitter is used by both the MIR→IR path (regular code) and the
//! produce/consume path (query pipelines).

use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

/// An opaque IR value handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrValue(pub u64);

/// An opaque IR basic block handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IrBlock(pub u64);

/// The interface for emitting low-level IR.
///
/// Implementations target specific backends. The produce/consume
/// traversal and the MIR lowering both emit through this interface.
pub trait IrEmitter {
    /// Create a new IR function.
    fn new_function(&mut self, name: Symbol, return_ty: TyId) -> IrValue;

    /// Create a new basic block in the current function.
    fn new_block(&mut self) -> IrBlock;

    /// Set the current insertion block.
    fn set_block(&mut self, block: IrBlock);

    /// Emit a binary operation: `result = lhs OP rhs`.
    fn emit_binop(&mut self, op: BinOpCode, lhs: IrValue, rhs: IrValue, ty: TyId) -> IrValue;

    /// Emit a unary operation: `result = OP operand`.
    fn emit_unop(&mut self, op: UnOpCode, operand: IrValue, ty: TyId) -> IrValue;

    /// Emit a function call: `result = func(args...)`.
    fn emit_call(&mut self, func: IrValue, args: &[IrValue], return_ty: TyId) -> IrValue;

    /// Emit a load from a pointer: `result = *ptr`.
    fn emit_load(&mut self, ptr: IrValue, ty: TyId) -> IrValue;

    /// Emit a store to a pointer: `*ptr = value`.
    fn emit_store(&mut self, ptr: IrValue, value: IrValue);

    /// Emit an alloca (stack allocation): `result = alloca ty`.
    fn emit_alloca(&mut self, ty: TyId) -> IrValue;

    /// Emit a constant integer.
    fn emit_const_int(&mut self, value: i128, ty: TyId) -> IrValue;

    /// Emit a constant float.
    fn emit_const_float(&mut self, value: f64, ty: TyId) -> IrValue;

    /// Emit a constant bool.
    fn emit_const_bool(&mut self, value: bool) -> IrValue;

    /// Emit a conditional branch: `if cond then bb_true else bb_false`.
    fn emit_cond_br(&mut self, cond: IrValue, bb_true: IrBlock, bb_false: IrBlock);

    /// Emit an unconditional branch: `goto bb`.
    fn emit_br(&mut self, bb: IrBlock);

    /// Emit a return: `return value`.
    fn emit_ret(&mut self, value: Option<IrValue>);

    /// Emit a struct field access: `result = base.field`.
    fn emit_field_access(&mut self, base: IrValue, field: Symbol, ty: TyId) -> IrValue;

    /// Emit an array index: `result = base[index]`.
    fn emit_index(&mut self, base: IrValue, index: IrValue, ty: TyId) -> IrValue;

    /// Emit a struct construction: `result = Struct { field1: val1, ... }`.
    fn emit_struct(&mut self, fields: &[(Symbol, IrValue)], ty: TyId) -> IrValue;

    /// Emit a tuple construction: `result = (val1, val2, ...)`.
    fn emit_tuple(&mut self, values: &[IrValue], ty: TyId) -> IrValue;

    /// Emit an array construction: `result = [val1, val2, ...]`.
    fn emit_array(&mut self, values: &[IrValue], ty: TyId) -> IrValue;

    /// Emit a cast: `result = value as ty`.
    fn emit_cast(&mut self, value: IrValue, ty: TyId) -> IrValue;

    /// Emit a comparison: `result = lhs CMP rhs`.
    fn emit_cmp(&mut self, op: CmpOp, lhs: IrValue, rhs: IrValue) -> IrValue;

    /// Finish the current function and return its handle.
    fn finish_function(&mut self) -> IrValue;
}

/// Binary operation codes for IR emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOpCode {
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
}

/// Unary operation codes for IR emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOpCode {
    Not,
    Neg,
}

/// Comparison operation codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}
