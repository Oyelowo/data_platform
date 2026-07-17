//! In-memory index mapping object ID to on-disk location.

use dashmap::DashMap;

use crate::format::RecordHeader;

/// On-disk location of an object payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobLocation {
    /// Volume file number.
    pub volume_number: u64,
    /// Byte offset of the record header in the volume.
    pub offset: u64,
    /// Length of the payload in bytes.
    pub payload_len: u64,
    /// CRC32C of the payload.
    pub payload_crc: u32,
}

impl BlobLocation {
    /// Build a location from a volume number and a freshly-written record.
    pub fn from_record(volume_number: u64, offset: u64, header: &RecordHeader) -> Self {
        Self {
            volume_number,
            offset,
            payload_len: header.payload_len,
            payload_crc: header.payload_crc,
        }
    }
}

/// Thread-safe in-memory index.
#[derive(Debug, Clone)]
pub struct Index {
    map: DashMap<Vec<u8>, BlobLocation>,
}

impl Default for Index {
    fn default() -> Self {
        Self::new()
    }
}

impl Index {
    /// Create an empty index.
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Look up an object by ID.
    pub fn get(&self, id: &[u8]) -> Option<BlobLocation> {
        self.map.get(id).map(|e| *e.value())
    }

    /// Insert or update the location for an ID.
    pub fn put(&self, id: Vec<u8>, location: BlobLocation) {
        self.map.insert(id, location);
    }

    /// Remove an ID from the index (delete).
    pub fn delete(&self, id: &[u8]) {
        self.map.remove(id);
    }

    /// Return a snapshot of all live (id, location) pairs.
    pub fn snapshot(&self) -> Vec<(Vec<u8>, BlobLocation)> {
        self.map
            .iter()
            .map(|e| (e.key().clone(), *e.value()))
            .collect()
    }

    /// Atomically replace the location for `id` if it currently equals
    /// `expected`.  Returns `Some(new)` on success, `None` otherwise.
    pub fn compare_and_swap(
        &self,
        id: &[u8],
        expected: BlobLocation,
        new: BlobLocation,
    ) -> Option<BlobLocation> {
        use dashmap::mapref::entry::Entry;
        match self.map.entry(id.to_vec()) {
            Entry::Occupied(mut e) => {
                if *e.get() == expected {
                    e.insert(new);
                    Some(new)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Number of indexed objects.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}
