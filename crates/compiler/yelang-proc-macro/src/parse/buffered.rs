//! Lookahead / fork support for speculative parsing.

use super::Cursor;

/// A cursor wrapper that supports cheap forking for speculative parsing.
///
/// `BufferedCursor` owns a primary [`Cursor`]. To try a parse without
/// committing to it, call [`fork`](Self::fork) to obtain a cheap clone,
/// attempt the parse on the fork, and then call
/// [`commit`](Self::commit) to replace the primary cursor with the fork
/// if the parse succeeded. Call [`reset`](Self::reset) to discard the
/// fork and restore the primary cursor to a previously saved snapshot.
#[derive(Debug, Clone)]
pub struct BufferedCursor<'a> {
    cursor: Cursor<'a>,
}

impl<'a> BufferedCursor<'a> {
    /// Create a buffered cursor from `cursor`.
    pub fn new(cursor: Cursor<'a>) -> Self {
        Self { cursor }
    }

    /// Return a cheap fork of the current primary cursor for speculative parsing.
    pub fn fork(&self) -> Cursor<'a> {
        self.cursor.fork()
    }

    /// Replace the primary cursor with `cursor`, typically a successfully advanced fork.
    pub fn commit(&mut self, cursor: Cursor<'a>) {
        self.cursor = cursor;
    }

    /// Reset the primary cursor to `cursor`, discarding any speculative advances.
    pub fn reset(&mut self, cursor: Cursor<'a>) {
        self.cursor = cursor;
    }

    /// Borrow the primary cursor.
    pub fn cursor(&self) -> &Cursor<'a> {
        &self.cursor
    }

    /// Borrow the primary cursor mutably.
    pub fn cursor_mut(&mut self) -> &mut Cursor<'a> {
        &mut self.cursor
    }

    /// Consume the wrapper and return the underlying cursor.
    pub fn into_cursor(self) -> Cursor<'a> {
        self.cursor
    }
}

impl<'a> From<Cursor<'a>> for BufferedCursor<'a> {
    fn from(cursor: Cursor<'a>) -> Self {
        Self::new(cursor)
    }
}
