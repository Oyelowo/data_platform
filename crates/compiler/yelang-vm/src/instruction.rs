//! Bytecode instruction set for the Yelang VM.
//!
//! Stack-based instruction set. Each instruction operates on the value
//! stack and/or local variables. The instruction pointer (IP) advances
//! through a flat instruction array.
//!
//! Design follows WebAssembly's stack-based model with extensions for
//! Yelang's query operations and aggregate support.

use yelang_interner::Symbol;

use crate::traverse::TraverseSpec;
use crate::value::Value;

/// A window aggregate function computed over a partition frame.
///
/// Used by [`WindowFunc::Aggregate`] for windowed `SUM`/`COUNT`/etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAgg {
    /// Sum of the field over the frame.
    Sum,
    /// Number of rows in the frame.
    Count,
    /// Average of the field over the frame.
    Avg,
    /// Minimum of the field over the frame.
    Min,
    /// Maximum of the field over the frame.
    Max,
}

/// A window function computed over a partition.
///
/// Mirrors the SQL standard window functions. Ranking functions (`RowNumber`,
/// `Rank`, `DenseRank`) depend only on the partition's order; `Lag`/`Lead`
/// access neighbouring rows; `Aggregate` reduces the whole partition frame.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowFunc {
    /// `ROW_NUMBER()` — sequential 1-based integer per partition.
    RowNumber,
    /// `RANK()` — rank with gaps for ties (e.g. `1, 1, 3`).
    Rank,
    /// `DENSE_RANK()` — rank without gaps for ties (e.g. `1, 1, 2`).
    DenseRank,
    /// `LAG(field, offset)` — value of `field` from `offset` rows earlier in
    /// the partition order, or `Null` if there is no such row.
    Lag(Symbol, usize),
    /// `LEAD(field, offset)` — value of `field` from `offset` rows later in
    /// the partition order, or `Null` if there is no such row.
    Lead(Symbol, usize),
    /// A windowed aggregate over `field` (frame = the whole partition).
    Aggregate(WindowAgg, Symbol),
}

/// A bytecode instruction.
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    // ── Stack operations ───────────────────────────────────────────────
    /// Push a constant value onto the stack.
    PushConst(Value),
    /// Pop the top value and discard it.
    Pop,
    /// Duplicate the top value.
    Dup,
    /// Swap the top two values.
    Swap,

    // ── Arithmetic ─────────────────────────────────────────────────────
    /// `a + b` (pops 2, pushes 1)
    Add,
    /// `a - b`
    Sub,
    /// `a * b`
    Mul,
    /// `a / b`
    Div,
    /// `a % b`
    Rem,
    /// `-a` (negate)
    Neg,
    /// `!a` (logical/bitwise not)
    Not,

    // ── Comparison ─────────────────────────────────────────────────────
    /// `a == b` → bool
    Eq,
    /// `a != b` → bool
    Ne,
    /// `a < b` → bool
    Lt,
    /// `a <= b` → bool
    Le,
    /// `a > b` → bool
    Gt,
    /// `a >= b` → bool
    Ge,

    // ── Bitwise ────────────────────────────────────────────────────────
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    // ── Local variables ────────────────────────────────────────────────
    /// Push local[slot] onto the stack.
    LoadLocal(u32),
    /// Pop the top value into local[slot].
    StoreLocal(u32),

    // ── Field access ───────────────────────────────────────────────────
    /// Pop a struct, push struct.field.
    LoadField(Symbol),
    /// Pop a value and a struct, set struct.field = value.
    StoreField(Symbol),

    // ── Array operations ───────────────────────────────────────────────
    /// Pop index and array, push array[index].
    Index,
    /// Pop value, index, and array, set array[index] = value.
    StoreIndex,
    /// Pop array, push array.len().
    Len,

    // ── Construction ───────────────────────────────────────────────────
    /// Pop N values, push Array([v1, v2, ..., vN]).
    MakeArray(u32),
    /// Pop N values, push Tuple(v1, v2, ..., vN).
    MakeTuple(u32),
    /// Pop N (name, value) pairs, push Struct(def_id, fields).
    MakeStruct(u64, u32),
    /// Pop N values, push EnumVariant(def_id, variant_idx, values).
    MakeEnumVariant(u64, usize, u32),

    // ── Option / Result ────────────────────────────────────────────────
    /// Pop value, push Some(value).
    MakeSome,
    /// Push None.
    MakeNone,
    /// Pop value, push Ok(value).
    MakeOk,
    /// Pop value, push Err(value).
    MakeErr,

    // ── Control flow ───────────────────────────────────────────────────
    /// Unconditional jump to instruction index.
    Jump(u32),
    /// Pop condition, jump if true.
    JumpIf(u32),
    /// Pop condition, jump if false.
    JumpIfNot(u32),

    // ── Functions ──────────────────────────────────────────────────────
    /// Call function with N arguments. Pops N args + function value.
    /// Pushes the return value.
    Call(u32),
    /// Return from the current function. Pops the return value.
    Return,

    // ── Iteration ──────────────────────────────────────────────────────
    /// Pop an iterable, push an Iterator value.
    IterInit,
    /// Pop an Iterator, push (next_value, true) or (null, false).
    IterNext,

    // ── Query operations (QIR execution) ───────────────────────────────
    /// Scan a table: push QueryResult.
    /// Operand: table identifier.
    QueryScan(u64),
    /// Filter: pop QueryResult + predicate closure, push filtered QueryResult.
    QueryFilter,
    /// Project: pop QueryResult + field list, push projected QueryResult.
    QueryProject(Vec<Symbol>),
    /// Join: pop two QueryResults + join predicate, push joined QueryResult.
    QueryJoin,
    /// Aggregate: pop QueryResult + group keys + agg functions, push aggregated result.
    QueryAggregate(Vec<Symbol>),
    /// Sort: pop QueryResult + sort keys, push sorted QueryResult.
    QuerySort(Vec<(Symbol, bool)>),
    /// Limit: pop QueryResult + skip + fetch, push limited QueryResult.
    QueryLimit,
    /// Traverse (links): pop QueryResult, follow links per the spec, push a
    /// QueryResult with a nested array column of matched target rows.
    QueryTraverse(TraverseSpec),

    // ── Window operations ──────────────────────────────────────────────
    /// Window: pop a QueryResult, compute a window function over partitions,
    /// push a QueryResult with an added `output` column.
    ///
    /// Rows are grouped into partitions by `partition_by` field values, ordered
    /// within each partition by the `order_by` keys (`(field, ascending)`), and
    /// the window `func` is evaluated for each row. The input row order is
    /// preserved in the output.
    Window {
        /// Fields that define each partition.
        partition_by: Vec<Symbol>,
        /// Ordering within a partition: `(field, ascending)`.
        order_by: Vec<(Symbol, bool)>,
        /// The window function to compute.
        func: WindowFunc,
        /// Output column name for the computed value.
        output: Symbol,
    },

    // ── Aggregate operations ───────────────────────────────────────────
    /// Pop a QueryResult, push the sum of all elements.
    AggSum,
    /// Pop a QueryResult, push the count.
    AggCount,
    /// Pop a QueryResult, push the average.
    AggAvg,
    /// Pop a QueryResult, push the minimum.
    AggMin,
    /// Pop a QueryResult, push the maximum.
    AggMax,

    // ── Misc ───────────────────────────────────────────────────────────
    /// No operation.
    Nop,
    /// Halt execution.
    Halt,
}

/// A compiled function: a sequence of instructions + metadata.
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    /// The function name (for diagnostics).
    pub name: Option<Symbol>,
    /// The bytecode instructions.
    pub instructions: Vec<Instruction>,
    /// Number of local variable slots.
    pub num_locals: u32,
    /// Number of argument slots.
    pub num_args: u32,
}

/// A compiled program: a collection of functions + a constant pool.
#[derive(Debug, Clone, Default)]
pub struct CompiledProgram {
    /// All compiled functions, indexed by function ID.
    pub functions: Vec<CompiledFunction>,
    /// Constant pool (shared across functions).
    pub constants: Vec<Value>,
    /// The entry point function ID.
    pub entry: Option<u64>,
}

impl CompiledProgram {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a function and return its ID.
    pub fn add_function(&mut self, func: CompiledFunction) -> u64 {
        let id = self.functions.len() as u64;
        self.functions.push(func);
        id
    }

    /// Get a function by ID.
    pub fn get_function(&self, id: u64) -> Option<&CompiledFunction> {
        self.functions.get(id as usize)
    }
}
