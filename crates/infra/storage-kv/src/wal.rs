//! Integration with `storage-wal`.

use bytes::{Buf, BufMut, Bytes};

use crate::SequenceNumber;
use crate::column_family::ColumnFamilyId;

/// Record type stored in the WAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordType {
    Put = 1,
    Delete = 2,
    DeleteRange = 3,
}

/// A WAL record for the KV engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalRecord {
    pub ty: WalRecordType,
    pub cf_id: ColumnFamilyId,
    pub key: Bytes,
    /// For `DeleteRange` this stores the exclusive end key.
    pub value: Option<Bytes>,
    pub sequence: SequenceNumber,
}

impl WalRecord {
    #[allow(dead_code)]
    pub fn new_put(key: &[u8], value: &[u8], sequence: SequenceNumber) -> Self {
        Self::new_put_cf(0, key, value, sequence)
    }

    pub fn new_put_cf(
        cf_id: ColumnFamilyId,
        key: &[u8],
        value: &[u8],
        sequence: SequenceNumber,
    ) -> Self {
        Self {
            ty: WalRecordType::Put,
            cf_id,
            key: Bytes::copy_from_slice(key),
            value: Some(Bytes::copy_from_slice(value)),
            sequence,
        }
    }

    #[allow(dead_code)]
    pub fn new_delete(key: &[u8], sequence: SequenceNumber) -> Self {
        Self::new_delete_cf(0, key, sequence)
    }

    pub fn new_delete_cf(cf_id: ColumnFamilyId, key: &[u8], sequence: SequenceNumber) -> Self {
        Self {
            ty: WalRecordType::Delete,
            cf_id,
            key: Bytes::copy_from_slice(key),
            value: None,
            sequence,
        }
    }

    #[allow(dead_code)]
    pub fn new_delete_range(start: &[u8], end: &[u8], sequence: SequenceNumber) -> Self {
        Self::new_delete_range_cf(0, start, end, sequence)
    }

    pub fn new_delete_range_cf(
        cf_id: ColumnFamilyId,
        start: &[u8],
        end: &[u8],
        sequence: SequenceNumber,
    ) -> Self {
        Self {
            ty: WalRecordType::DeleteRange,
            cf_id,
            key: Bytes::copy_from_slice(start),
            value: Some(Bytes::copy_from_slice(end)),
            sequence,
        }
    }

    pub fn encode(&self, buf: &mut Vec<u8>) {
        buf.put_u8(self.ty as u8);
        buf.put_u64_le(self.sequence);
        buf.put_u32_le(self.cf_id);
        buf.put_u32_le(self.key.len() as u32);
        buf.extend_from_slice(&self.key);
        if let Some(ref v) = self.value {
            buf.put_u32_le(v.len() as u32);
            buf.extend_from_slice(v);
        } else {
            buf.put_u32_le(0);
        }
    }

    pub fn decode(buf: &[u8]) -> Option<(Self, usize)> {
        if buf.len() < 17 {
            return None;
        }
        let mut cursor = buf;
        let ty = match cursor.get_u8() {
            1 => WalRecordType::Put,
            2 => WalRecordType::Delete,
            3 => WalRecordType::DeleteRange,
            _ => return None,
        };
        let sequence = cursor.get_u64_le();
        let cf_id = cursor.get_u32_le();
        let key_len = cursor.get_u32_le() as usize;
        if cursor.len() < key_len + 4 {
            return None;
        }
        let key = Bytes::copy_from_slice(&cursor[..key_len]);
        cursor.advance(key_len);
        let value_len = cursor.get_u32_le() as usize;
        if cursor.len() < value_len {
            return None;
        }
        let value = if value_len == 0 {
            None
        } else {
            Some(Bytes::copy_from_slice(&cursor[..value_len]))
        };
        cursor.advance(value_len);
        let consumed = buf.len() - cursor.len();
        Some((
            Self {
                ty,
                cf_id,
                key,
                value,
                sequence,
            },
            consumed,
        ))
    }

    /// Convert to a storage-wal RecordType.
    #[allow(dead_code)]
    pub fn to_wal_type(&self) -> storage_wal::RecordType {
        match self.ty {
            WalRecordType::Put => storage_wal::RecordType::Put,
            WalRecordType::Delete => storage_wal::RecordType::Delete,
            WalRecordType::DeleteRange => storage_wal::RecordType::Delete,
        }
    }
}
