//! Execution operators.

use std::ffi::c_void;

use yelang_hir::ids::DefId;
use yelang_interner::Symbol;

use crate::exec::value::ArrowSchema;
use crate::ids::ExecId;
use crate::pir::operator::{AggMode, ExchangeKind, JoinKind};

/// A bound aggregate function pointer set.
#[derive(Debug)]
pub struct BoundAggregate {
    pub agg_def: DefId,
    pub input_column: usize,
    pub init_fn: extern "C" fn() -> *mut c_void,
    pub step_fn: extern "C" fn(*mut c_void, *const u8),
    pub merge_fn: extern "C" fn(*mut c_void, *const c_void),
    pub finish_fn: extern "C" fn(*const c_void, *mut u8),
    pub acc_layout: ArrowSchema,
    pub out_layout: ArrowSchema,
}

/// A bound scalar kernel.
#[derive(Debug)]
pub struct BoundKernel {
    pub def: DefId,
    pub fn_ptr: extern "C" fn(*const *const u8, *mut u8, usize),
}

/// An execution operator.
#[derive(Debug)]
pub enum ExecOp {
    Scan(ScanKernel),
    Filter(FilterKernel),
    Project(ProjectKernel),
    HashJoin(HashJoinExec),
    MergeJoin(MergeJoinExec),
    NestedLoopJoin(NestedLoopJoinExec),
    HashAggregate(HashAggregateExec),
    Sort(SortExec),
    TopK(TopKExec),
    Slice(SliceExec),
    Exchange(ExchangeExec),
    Union(Vec<ExecId>),
    UnionAll(Vec<ExecId>),
    Distinct(DistinctExec),
    EdgeExpand(EdgeExpandExec),
    Construct(ConstructExec),
    AttachField(AttachFieldExec),
    Window(WindowExec),
    Expr(ExprExec),
}

#[derive(Debug)]
pub struct ScanKernel {
    pub source: crate::logical::operator::ScanSource,
    pub predicate: Option<crate::ids::QExprId>,
    pub projection: crate::demand::DemandSet,
}

#[derive(Debug)]
pub struct FilterKernel {
    pub input: ExecId,
    pub predicate: crate::ids::QExprId,
    pub kernel: BoundKernel,
}

#[derive(Debug)]
pub struct ProjectKernel {
    pub input: ExecId,
    pub projection: crate::ids::QExprId,
    pub kernels: Vec<BoundKernel>,
}

#[derive(Debug)]
pub struct HashJoinExec {
    pub build: ExecId,
    pub probe: ExecId,
    pub build_key: crate::ids::QExprId,
    pub probe_key: crate::ids::QExprId,
    pub kind: JoinKind,
}

#[derive(Debug)]
pub struct MergeJoinExec {
    pub left: ExecId,
    pub right: ExecId,
    pub left_keys: Vec<crate::expr::OrderKey>,
    pub right_keys: Vec<crate::expr::OrderKey>,
    pub kind: JoinKind,
}

#[derive(Debug)]
pub struct NestedLoopJoinExec {
    pub outer: ExecId,
    pub inner: ExecId,
    pub predicate: crate::ids::QExprId,
    pub kind: JoinKind,
}

#[derive(Debug)]
pub struct HashAggregateExec {
    pub input: ExecId,
    pub group_key_indices: Vec<usize>,
    pub aggregates: Vec<BoundAggregate>,
    pub mode: AggMode,
}

#[derive(Debug)]
pub struct SortExec {
    pub input: ExecId,
    pub keys: Vec<crate::expr::OrderKey>,
}

#[derive(Debug)]
pub struct TopKExec {
    pub input: ExecId,
    pub keys: Vec<crate::expr::OrderKey>,
    pub k: usize,
}

#[derive(Debug)]
pub struct SliceExec {
    pub input: ExecId,
    pub offset: usize,
    pub limit: Option<usize>,
}

#[derive(Debug)]
pub struct ExchangeExec {
    pub input: ExecId,
    pub kind: ExchangeKind,
    pub endpoint: ExchangeEndpoint,
}

#[derive(Debug, Clone, Copy)]
pub enum ExchangeEndpoint {
    Local,
    Remote(u32),
}

#[derive(Debug)]
pub struct DistinctExec {
    pub input: ExecId,
    pub key_indices: Vec<usize>,
}

#[derive(Debug)]
pub struct EdgeExpandExec {
    pub input: ExecId,
    pub edge: DefId,
    pub direction: crate::logical::operator::EdgeDirection,
    pub predicate: Option<crate::ids::QExprId>,
}

#[derive(Debug)]
pub struct ConstructExec {
    pub kind: crate::logical::operator::ConstructKind,
    pub field_inputs: Vec<(Symbol, ExecId)>,
}

#[derive(Debug)]
pub struct AttachFieldExec {
    pub input: ExecId,
    pub field: Symbol,
    pub value_plan: ExecId,
}

#[derive(Debug)]
pub struct WindowExec {
    pub input: ExecId,
    pub func: crate::expr::WindowFunc,
    pub partition_indices: Vec<usize>,
    pub order_keys: Vec<crate::expr::OrderKey>,
}

#[derive(Debug)]
pub struct ExprExec {
    pub expr: crate::ids::QExprId,
}
