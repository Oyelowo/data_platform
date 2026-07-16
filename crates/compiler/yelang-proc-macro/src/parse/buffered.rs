//! Lookahead / fork support.

use super::Cursor;

/// A cursor that can be forked for speculative parsing.
#[derive(Debug, Clone)]
pub struct Buffered {
    cursor: Cursor,
}

impl Buffered {
    pub fn new(cursor: Cursor) -> Self {
        Self { cursor }
    }

    pub fn fork(&self) -> Cursor {
        self.cursor.clone()
    }

    pub fn reset(&mut self, cursor: Cursor) {
        self.cursor = cursor;
    }
}
