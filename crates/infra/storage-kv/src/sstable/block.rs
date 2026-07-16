//! Block-based data block builder and iterator.

use bytes::{Buf, BufMut};

/// Builder for a data block with prefix compression restart points.
pub struct BlockBuilder {
    buf: Vec<u8>,
    restarts: Vec<u32>,
    last_key: Vec<u8>,
    restart_interval: usize,
    entry_count: usize,
    finished: bool,
}

impl BlockBuilder {
    pub fn new(restart_interval: usize) -> Self {
        Self {
            buf: Vec::new(),
            restarts: Vec::new(),
            last_key: Vec::new(),
            restart_interval,
            entry_count: 0,
            finished: false,
        }
    }

    /// Reset the builder to an empty state.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.restarts.clear();
        self.last_key.clear();
        self.entry_count = 0;
        self.finished = false;
    }

    /// Add a key/value pair. Keys must be added in strictly ascending order.
    pub fn add(&mut self, key: &[u8], value: &[u8]) {
        assert!(!self.finished);
        assert!(
            self.last_key.as_slice() < key,
            "keys must be added in ascending order"
        );

        let restart = self.entry_count.is_multiple_of(self.restart_interval);
        let shared = if restart {
            self.restarts.push(self.buf.len() as u32);
            0
        } else {
            shared_prefix_len(&self.last_key, key)
        };

        let non_shared = key.len() - shared;
        self.buf.put_u32_le(shared as u32);
        self.buf.put_u32_le(non_shared as u32);
        self.buf.put_u32_le(value.len() as u32);
        self.buf.extend_from_slice(&key[shared..]);
        self.buf.extend_from_slice(value);

        self.last_key.clear();
        self.last_key.extend_from_slice(key);
        self.entry_count += 1;
    }

    /// Finish the block and return the encoded bytes (excluding trailer).
    pub fn finish(&mut self) -> &[u8] {
        if !self.finished {
            for r in &self.restarts {
                self.buf.put_u32_le(*r);
            }
            self.buf.put_u32_le(self.restarts.len() as u32);
            self.finished = true;
        }
        &self.buf
    }

    pub fn current_size_estimate(&self) -> usize {
        self.buf.len() + self.restarts.len() * 4 + 4
    }

    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }
}

fn shared_prefix_len(a: &[u8], b: &[u8]) -> usize {
    let mut i = 0;
    while i < a.len() && i < b.len() && a[i] == b[i] {
        i += 1;
    }
    i
}

/// Iterator over entries in a data block.
pub struct BlockIterator<'a> {
    data: &'a [u8],
    restarts_offset: usize,
    num_restarts: u32,
    current: usize,
    current_key: Vec<u8>,
    current_value: &'a [u8],
    valid: bool,
}

impl<'a> BlockIterator<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let (num_restarts, restarts_offset) = read_num_restarts(data);
        Self {
            data,
            restarts_offset,
            num_restarts,
            current: data.len(), // invalid initially
            current_key: Vec::new(),
            current_value: &[],
            valid: false,
        }
    }

    /// Position the iterator at the first key >= target.
    pub fn seek(&mut self, target: &[u8]) {
        if self.num_restarts == 0 {
            self.valid = false;
            return;
        }
        let mut left: usize = 0;
        let mut right: usize = self.num_restarts as usize;
        while left < right {
            let mid = (left + right) / 2;
            let offset = self.restart_offset(mid);
            let key = self.key_at_restart(offset);
            if key.as_slice() < target {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        let restart_idx = if left == 0 { 0 } else { left - 1 };
        self.parse_from_restart(restart_idx);
        while self.valid && self.current_key.as_slice() < target {
            self.next();
        }
    }

    pub fn seek_to_first(&mut self) {
        if self.num_restarts == 0 {
            self.valid = false;
            return;
        }
        self.parse_from_restart(0);
    }

    fn restart_offset(&self, idx: usize) -> usize {
        let mut cursor = &self.data[self.restarts_offset + idx * 4..];
        cursor.get_u32_le() as usize
    }

    fn key_at_restart(&self, offset: usize) -> Vec<u8> {
        let mut cursor = &self.data[offset..];
        let _shared = cursor.get_u32_le();
        let non_shared = cursor.get_u32_le() as usize;
        let _value_len = cursor.get_u32_le();
        cursor[..non_shared].to_vec()
    }

    fn parse_from_restart(&mut self, idx: usize) {
        self.current = self.restart_offset(idx);
        self.current_key.clear();
        self.parse_entry();
    }

    fn parse_entry(&mut self) {
        if self.current >= self.restarts_offset {
            self.valid = false;
            return;
        }
        let mut cursor = &self.data[self.current..];
        let shared = cursor.get_u32_le() as usize;
        let non_shared = cursor.get_u32_le() as usize;
        let value_len = cursor.get_u32_le() as usize;
        self.current_key.truncate(shared);
        self.current_key.extend_from_slice(&cursor[..non_shared]);
        let value_start = self.current + 12 + non_shared;
        self.current_value = &self.data[value_start..value_start + value_len];
        self.current = value_start + value_len;
        self.valid = true;
    }

    pub fn key(&self) -> &[u8] {
        &self.current_key
    }

    pub fn value(&self) -> &[u8] {
        self.current_value
    }

    pub fn valid(&self) -> bool {
        self.valid
    }

    pub fn next(&mut self) {
        if !self.valid {
            return;
        }
        self.parse_entry();
    }
}

fn read_num_restarts(data: &[u8]) -> (u32, usize) {
    let mut cursor = &data[data.len() - 4..];
    let num = cursor.get_u32_le();
    let offset = data.len() - 4 - num as usize * 4;
    (num, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_roundtrip() {
        let mut builder = BlockBuilder::new(2);
        builder.add(b"apple", b"1");
        builder.add(b"application", b"2");
        builder.add(b"banana", b"3");
        let data = builder.finish().to_vec();

        let mut iter = BlockIterator::new(&data);
        iter.seek_to_first();
        assert_eq!(iter.key(), b"apple");
        assert_eq!(iter.value(), b"1");
        iter.next();
        assert_eq!(iter.key(), b"application");
        assert_eq!(iter.value(), b"2");
        iter.next();
        assert_eq!(iter.key(), b"banana");
        assert_eq!(iter.value(), b"3");
        iter.next();
        assert!(!iter.valid());
    }

    #[test]
    fn block_seek() {
        let mut builder = BlockBuilder::new(2);
        builder.add(b"a", b"1");
        builder.add(b"b", b"2");
        builder.add(b"d", b"3");
        let data = builder.finish().to_vec();

        let mut iter = BlockIterator::new(&data);
        iter.seek(b"c");
        assert!(iter.valid());
        assert_eq!(iter.key(), b"d");
    }
}
