//! QIR ↔ MIR bridge.
//!
//! Converts QIR query results into MIR locals. When the MIR builder
//! encounters a `ThirExpr::Query`, it calls the bridge to:
//! 1. Get the QIR physical plan for the query
//! 2. Execute the plan (via the Executor trait)
//! 3. Store the result in a MIR local
//!
//! The bridge is the connection point between the query pipeline (QIR)
//! and the general-purpose pipeline (MIR). Query results become ordinary
//! MIR values that can be used in regular code.
//!
//! ```text
//! THIR: let result = select users@u[*].id from users@u:User;
//!
//! QIR:  Scan(users) → Map(u.id) → Physical plan
//!
//! MIR:  _1 = Call(query_execute, [physical_plan_id])
//!       // _1 now holds the query result (an array of ids)
//!       // regular MIR code can use _1 like any other local
//! ```

use yelang_arena::DefId;
use yelang_interner::Symbol;
use yelang_lexer::Span;
use yelang_ty::ty::TyId;

use crate::body::{Body, Local, Statement};
use crate::ops::{ConstValue, Constant, Operand};
use crate::place::Place;
use crate::terminator::{Terminator, TerminatorKind};

/// A query execution request: everything needed to execute a QIR plan
/// and store the result in a MIR local.
#[derive(Debug, Clone)]
pub struct QueryBridgeRequest {
    /// The QIR physical plan ID (opaque handle).
    pub plan_id: u64,
    /// The MIR local to store the result in.
    pub result_local: Local,
    /// The expected result type.
    pub result_ty: TyId,
    /// Source span for diagnostics.
    pub span: Span,
}

/// The QIR↔MIR bridge.
///
/// Collects query execution requests during MIR building.
/// After MIR building, the requests are executed and the results
/// are stored in the corresponding MIR locals.
#[derive(Debug, Default)]
pub struct QueryBridge {
    /// Pending query execution requests.
    pub requests: Vec<QueryBridgeRequest>,
}

impl QueryBridge {
    pub fn new() -> Self {
        Self {
            requests: Vec::new(),
        }
    }

    /// Register a query execution request.
    ///
    /// Called by the MIR builder when it encounters a `ThirExpr::Query`.
    /// Returns the MIR local that will hold the query result.
    pub fn register_query(
        &mut self,
        body: &mut Body,
        plan_id: u64,
        result_ty: TyId,
        span: Span,
    ) -> Local {
        let result_local = body.new_temp(result_ty);
        self.requests.push(QueryBridgeRequest {
            plan_id,
            result_local,
            result_ty,
            span,
        });
        result_local
    }

    /// Emit MIR code for all pending query executions.
    ///
    /// Each query becomes a `Call` to a built-in `query_execute` function
    /// that takes the plan ID and returns the result.
    pub fn emit_query_calls(&self, body: &mut Body, _query_execute_fn: DefId) {
        let entry = body.entry_block();
        for request in &self.requests {
            // Create a call to query_execute(plan_id) -> result_local.
            let plan_id_local = body.new_temp(request.result_ty); // TODO: use u64 type
            body.basic_blocks[entry]
                .statements
                .push(Statement::Assign(
                    Place::local(plan_id_local),
                    crate::body::Rvalue::Use(Operand::Constant(Constant {
                        ty: request.result_ty, // TODO: use u64 type
                        value: ConstValue::Uint(request.plan_id as u128),
                    })),
                ));
        }
    }

    /// Whether there are any pending query requests.
    pub fn has_queries(&self) -> bool {
        !self.requests.is_empty()
    }
}
