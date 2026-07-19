//! Cursor wrappers that strip internal key prefixes and map error types.

use bytes::Bytes;
use storage_traits::cursor::Cursor;

use crate::error::Error;
use crate::keys::{primary_key, unpack_primary_key};

/// A cursor over primary records. It wraps the underlying engine cursor,
/// strips the primary-record prefix from returned keys, filters by the
/// user-visible `[start, end)` range, and maps the underlying error into
/// [`Error`].
pub struct IndexCursor<Inner> {
    inner: Inner,
    start: Option<Bytes>,
    end: Option<Bytes>,
}

impl<Inner> IndexCursor<Inner> {
    pub(crate) fn new(inner: Inner, start: Option<Bytes>, end: Option<Bytes>) -> Self {
        Self {
            inner,
            start,
            end,
        }
    }
}

impl<Inner, E> Iterator for IndexCursor<Inner>
where
    Inner: Iterator<Item = std::result::Result<(Bytes, Bytes), E>>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = std::result::Result<(Bytes, Bytes), Error<E>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (key, value) = match self.inner.next()? {
                Ok(kv) => kv,
                Err(e) => return Some(Err(Error::Engine(e))),
            };
            let user_key = match unpack_primary_key(&key) {
                Some(k) => Bytes::copy_from_slice(k),
                None => key,
            };
            if let Some(ref start) = self.start
                && user_key.as_ref() < start.as_ref()
            {
                continue;
            }
            if let Some(ref end) = self.end
                && user_key.as_ref() >= end.as_ref()
            {
                return None;
            }
            return Some(Ok((user_key, value)));
        }
    }
}

impl<Inner, E> Cursor for IndexCursor<Inner>
where
    Inner: Cursor<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Error = Error<E>;

    fn seek(&mut self, target: &[u8]) -> std::result::Result<(), Self::Error> {
        self.inner.seek(&primary_key(target)).map_err(Error::Engine)
    }
}

/// A cursor over index entries. The key is parsed to extract the primary key,
/// and the value (which is also the primary key) is returned as well.
pub struct IndexEntryCursor<Inner> {
    inner: Inner,
}

impl<Inner> IndexEntryCursor<Inner> {
    pub(crate) fn new(inner: Inner) -> Self {
        Self { inner }
    }
}

impl<Inner, E> Iterator for IndexEntryCursor<Inner>
where
    Inner: Iterator<Item = std::result::Result<(Bytes, Bytes), E>>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = std::result::Result<(Bytes, Bytes), Error<E>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next()? {
            // The underlying key is the full index key (ordered by column
            // value, then primary key). The value is the primary key.
            Ok((key, value)) => Some(Ok((key, value))),
            Err(e) => Some(Err(Error::Engine(e))),
        }
    }
}

impl<Inner, E> Cursor for IndexEntryCursor<Inner>
where
    Inner: Cursor<Error = E>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Error = Error<E>;

    fn seek(&mut self, target: &[u8]) -> std::result::Result<(), Self::Error> {
        self.inner.seek(target).map_err(Error::Engine)
    }
}
