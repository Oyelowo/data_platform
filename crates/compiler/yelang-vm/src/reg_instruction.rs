//! Register-based bytecode instruction set.
//!
//! Based on Lua's register VM design (2× faster than stack-based for
//! values > 8 bytes; our Value is ~40 bytes).
//!
//! Instruction encoding: 32-bit fixed width.
//!
//! Format 1: OP(6) | A(8) | B(9) | C(9)
//!   A = destination register
//!   B, C = source registers or constants (RK flag in high bit)
//!
//! Format 2: OP(6) | A(8) | Bx(18)
//!   A = destination register
//!   Bx = constant index or jump offset
//!
//! Register-or-constant (RK): if high bit of B/C is set, the remaining
//! 8 bits index into the constant pool. Otherwise, it's a register index.

/// Maximum number of registers per function.
pub const MAX_REGISTERS: usize = 256;

/// Maximum number of constants per function.
pub const MAX_CONSTANTS: usize = 256;

/// RK flag: if set, the operand is a constant index.
pub const RK_FLAG: u16 = 0x100;

/// Check if an operand is a constant reference.
pub fn is_rk(operand: u16) -> bool {
    operand & RK_FLAG != 0
}

/// Extract the index from an RK operand.
pub fn rk_index(operand: u16) -> u8 {
    (operand & 0xFF) as u8
}

/// Create an RK operand from a constant index.
pub fn make_rk(index: u8) -> u16 {
    RK_FLAG | index as u16
}

/// Register-based instruction set.
///
/// Each instruction is 32 bits. The opcode is the low 6 bits.
/// Operands are packed into the remaining 26 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegInstruction {
    // ── Data movement ──────────────────────────────────────────────────
    /// R(A) = R(B)
    Move { a: u8, b: u8 },
    /// R(A) = K(Bx) (load constant)
    LoadK { a: u8, bx: u16 },
    /// R(A) = nil
    LoadNil { a: u8 },
    /// R(A) = true/false
    LoadBool { a: u8, value: bool },

    // ── Arithmetic ─────────────────────────────────────────────────────
    /// R(A) = RK(B) + RK(C)
    Add { a: u8, b: u16, c: u16 },
    /// R(A) = RK(B) - RK(C)
    Sub { a: u8, b: u16, c: u16 },
    /// R(A) = RK(B) * RK(C)
    Mul { a: u8, b: u16, c: u16 },
    /// R(A) = RK(B) / RK(C)
    Div { a: u8, b: u16, c: u16 },
    /// R(A) = RK(B) % RK(C)
    Rem { a: u8, b: u16, c: u16 },
    /// R(A) = -RK(B)
    Neg { a: u8, b: u16 },
    /// R(A) = !RK(B)
    Not { a: u8, b: u16 },

    // ── Comparison (sets R(A) to bool) ─────────────────────────────────
    /// R(A) = RK(B) == RK(C)
    Eq { a: u8, b: u16, c: u16 },
    /// R(A) = RK(B) != RK(C)
    Ne { a: u8, b: u16, c: u16 },
    /// R(A) = RK(B) < RK(C)
    Lt { a: u8, b: u16, c: u16 },
    /// R(A) = RK(B) <= RK(C)
    Le { a: u8, b: u16, c: u16 },
    /// R(A) = RK(B) > RK(C)
    Gt { a: u8, b: u16, c: u16 },
    /// R(A) = RK(B) >= RK(C)
    Ge { a: u8, b: u16, c: u16 },

    // ── Bitwise ────────────────────────────────────────────────────────
    BitAnd { a: u8, b: u16, c: u16 },
    BitOr { a: u8, b: u16, c: u16 },
    BitXor { a: u8, b: u16, c: u16 },
    Shl { a: u8, b: u16, c: u16 },
    Shr { a: u8, b: u16, c: u16 },

    // ── Field access ───────────────────────────────────────────────────
    /// R(A) = R(B).field(K(C))
    GetField { a: u8, b: u8, field: u16 },
    /// R(B).field(K(C)) = R(A)
    SetField { a: u8, b: u8, field: u16 },

    // ── Array operations ───────────────────────────────────────────────
    /// R(A) = R(B)[R(C)]
    GetIndex { a: u8, b: u8, c: u8 },
    /// R(B)[R(C)] = R(A)
    SetIndex { a: u8, b: u8, c: u8 },
    /// R(A) = len(R(B))
    Len { a: u8, b: u8 },

    // ── Construction ───────────────────────────────────────────────────
    /// R(A) = Array(R(B)..R(B+C-1))
    MakeArray { a: u8, b: u8, count: u8 },
    /// R(A) = Tuple(R(B)..R(B+C-1))
    MakeTuple { a: u8, b: u8, count: u8 },
    /// R(A) = Struct(def_id, R(B)..R(B+C-1))
    MakeStruct { a: u8, def_id: u16, b: u8, count: u8 },

    // ── Control flow ───────────────────────────────────────────────────
    /// Jump to instruction Bx
    Jump { bx: u16 },
    /// if R(A) then jump to Bx
    JumpIf { a: u8, bx: u16 },
    /// if !R(A) then jump to Bx
    JumpIfNot { a: u8, bx: u16 },

    // ── Functions ──────────────────────────────────────────────────────
    /// R(A) = call R(B)(R(B+1)..R(B+C))
    Call { a: u8, b: u8, num_args: u8 },
    /// return R(A)
    Return { a: u8 },

    // ── Iteration ──────────────────────────────────────────────────────
    /// R(A) = iter_init(R(B))
    IterInit { a: u8, b: u8 },
    /// R(A), R(A+1) = iter_next(R(B)) → (value, has_next)
    IterNext { a: u8, b: u8 },

    // ── Query operations ───────────────────────────────────────────────
    /// R(A) = query_scan(table_id)
    QueryScan { a: u8, table_id: u16 },
    /// R(A) = query_filter(R(B), R(C))
    QueryFilter { a: u8, b: u8, c: u8 },
    /// R(A) = query_project(R(B), fields)
    QueryProject { a: u8, b: u8 },
    /// R(A) = query_join(R(B), R(C))
    QueryJoin { a: u8, b: u8, c: u8 },
    /// R(A) = query_aggregate(R(B), keys)
    QueryAggregate { a: u8, b: u8 },
    /// R(A) = query_sort(R(B), keys)
    QuerySort { a: u8, b: u8 },
    /// R(A) = query_limit(R(B), R(C), R(C+1))
    QueryLimit { a: u8, b: u8, c: u8 },

    // ── Aggregates ─────────────────────────────────────────────────────
    /// R(A) = sum(R(B))
    AggSum { a: u8, b: u8 },
    /// R(A) = count(R(B))
    AggCount { a: u8, b: u8 },
    /// R(A) = avg(R(B))
    AggAvg { a: u8, b: u8 },
    /// R(A) = min(R(B))
    AggMin { a: u8, b: u8 },
    /// R(A) = max(R(B))
    AggMax { a: u8, b: u8 },

    // ── Misc ───────────────────────────────────────────────────────────
    Nop,
    Halt,
}

impl RegInstruction {
    /// Encode this instruction as a 32-bit word.
    pub fn encode(&self) -> u32 {
        // TODO: implement actual encoding.
        // For now, return 0 as placeholder.
        0
    }

    /// Decode a 32-bit word into an instruction.
    pub fn decode(_word: u32) -> Self {
        // TODO: implement actual decoding.
        RegInstruction::Nop
    }
}
