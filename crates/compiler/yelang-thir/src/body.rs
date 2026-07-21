//! THIR bodies.
//!
//! A `ThirBody` is a self-contained unit with parameters and a value
//! expression. `ThirBodies` owns all bodies produced during HIR → THIR
//! lowering.

use slotmap::SlotMap;

use crate::ids::{ThirBodyId, ThirExprId, ThirPatId};

#[derive(Debug, Clone)]
pub struct ThirBody {
    pub params: Vec<ThirPatId>,
    pub value: ThirExprId,
}

#[derive(Debug, Clone, Default)]
pub struct ThirBodies {
    pub bodies: SlotMap<ThirBodyId, ThirBody>,
}

impl ThirBodies {
    pub fn alloc(&mut self, params: Vec<ThirPatId>, value: ThirExprId) -> ThirBodyId {
        self.bodies.insert(ThirBody { params, value })
    }
}
