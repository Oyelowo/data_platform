//! Expression equivalence / union-find for rewrites and CSE.

use std::collections::HashMap;

use crate::expr::QExprId;

/// A simple union-find over expression ids.
#[derive(Debug, Default)]
pub struct ExprEquiv {
    parent: HashMap<QExprId, QExprId>,
}

impl ExprEquiv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn union(&mut self, a: QExprId, b: QExprId) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }

    pub fn find(&mut self, id: QExprId) -> QExprId {
        let parent = *self.parent.get(&id).unwrap_or(&id);
        if parent == id {
            id
        } else {
            let root = self.find(parent);
            self.parent.insert(id, root);
            root
        }
    }
}
