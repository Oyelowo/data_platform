//! Time-range cursor implementation.

use crate::format::{Sample, Timestamp};

/// Iterator over samples within a time range.
#[derive(Debug)]
pub struct RangeCursor<I> {
    inner: I,
    start: Timestamp,
    end: Timestamp,
}

impl<I> RangeCursor<I> {
    /// Create a range cursor filtering `inner` to `[start, end)`.
    pub fn new(inner: I, start: Timestamp, end: Timestamp) -> Self {
        Self { inner, start, end }
    }
}

impl<I> Iterator for RangeCursor<I>
where
    I: Iterator<Item = crate::Result<Sample>>,
{
    type Item = crate::Result<Sample>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next() {
                Some(Ok(sample)) => {
                    if sample.timestamp >= self.start && sample.timestamp < self.end {
                        return Some(Ok(sample));
                    }
                    if sample.timestamp >= self.end {
                        return None;
                    }
                }
                other => return other,
            }
        }
    }
}
