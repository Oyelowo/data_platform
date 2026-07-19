//! R*-tree spatial index wrapping `rstar`.

use bytes::{Buf, BufMut};
use rstar::{AABB, PointDistance, RTree, RTreeObject};

use crate::feature::{Feature, Geometry};
use crate::store::FeatureAddress;

/// An R-tree entry.
#[derive(Debug, Clone)]
pub struct IndexedFeature {
    /// Envelope of the feature in degrees.
    pub envelope: AABB<[f64; 2]>,
    /// Feature id.
    pub id: Vec<u8>,
    /// On-disk address.
    pub address: FeatureAddress,
}

impl PartialEq for IndexedFeature {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for IndexedFeature {}

impl RTreeObject for IndexedFeature {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.envelope
    }
}

impl PointDistance for IndexedFeature {
    fn distance_2(&self, point: &[f64; 2]) -> f64 {
        let lower = self.envelope.lower();
        let upper = self.envelope.upper();
        let mut d2 = 0.0;
        for i in 0..2 {
            let v = if point[i] < lower[i] {
                lower[i] - point[i]
            } else if point[i] > upper[i] {
                point[i] - upper[i]
            } else {
                0.0
            };
            d2 += v * v;
        }
        d2
    }
}

/// Spatial index backed by `rstar`.
#[derive(Clone, Default)]
pub struct SpatialIndex {
    tree: RTree<IndexedFeature>,
}

impl SpatialIndex {
    /// Create an empty spatial index.
    pub fn new() -> Self {
        Self {
            tree: RTree::new(),
        }
    }

    /// Insert a feature into the index.
    pub fn insert(&mut self, feature: &Feature, address: FeatureAddress) {
        let Some(envelope) = geometry_envelope(&feature.geometry) else {
            // Empty geometries are not indexed.
            return;
        };
        self.tree.insert(IndexedFeature {
            envelope,
            id: feature.id.clone(),
            address,
        });
    }

    /// Remove a feature from the index by id.
    pub fn delete(&mut self, id: &[u8]) {
        let dummy = IndexedFeature {
            envelope: AABB::from_corners([0.0, 0.0], [0.0, 0.0]),
            id: id.to_vec(),
            address: FeatureAddress {
                file_id: 0,
                offset: 0,
                len: 0,
            },
        };
        let _ = self.tree.remove(&dummy);
    }

    /// Return all indexed features whose envelopes intersect `bbox`.
    pub fn intersecting_bbox(
        &self,
        min_lon: f64,
        min_lat: f64,
        max_lon: f64,
        max_lat: f64,
    ) -> Vec<&IndexedFeature> {
        let envelope = AABB::from_corners([min_lon, min_lat], [max_lon, max_lat]);
        self.tree.locate_in_envelope_intersecting(&envelope).collect()
    }

    /// Return the `k` nearest envelopes to `point`.
    pub fn nearest(&self, point: &geo::Point<f64>, k: usize) -> Vec<&IndexedFeature> {
        if k == 0 {
            return Vec::new();
        }
        self.tree
            .nearest_neighbor_iter(&[point.0.x, point.0.y])
            .take(k)
            .collect()
    }

    /// Return the number of entries in the index.
    pub fn len(&self) -> usize {
        self.tree.size()
    }

    /// Return whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bulk-load the index from entries.
    pub fn bulk_load(entries: Vec<IndexedFeature>) -> Self {
        Self {
            tree: RTree::bulk_load(entries),
        }
    }

    /// Serialize the index to bytes.
    ///
    /// Layout: `[magic u32][version u32][count u32][entry...][crc u32]`.
    pub fn encode(&self) -> crate::Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(12 + self.tree.size() * 48);
        buf.put_u32_le(crate::format::MAGIC);
        buf.put_u32_le(crate::format::VERSION);
        buf.put_u32_le(self.tree.size() as u32);
        for entry in self.tree.iter() {
            encode_entry(entry, &mut buf);
        }
        let crc = storage_format::crc32c(&buf);
        buf.put_u32_le(crc);
        Ok(buf)
    }

    /// Deserialize the index from bytes.
    pub fn decode(data: &[u8]) -> crate::Result<Self> {
        if data.len() < 12 {
            return Err(crate::Error::corruption("index file too short"));
        }
        let mut cursor = data;
        let magic = cursor.get_u32_le();
        let version = cursor.get_u32_le();
        if magic != crate::format::MAGIC {
            return Err(crate::Error::corruption(format!(
                "bad index magic: {magic:#x}"
            )));
        }
        if version != crate::format::VERSION {
            return Err(crate::Error::corruption(format!(
                "unsupported index version: {version}"
            )));
        }
        let body_end = data.len() - 4;
        let stored_crc = storage_format::read_u32_le(&data[body_end..]);
        let computed_crc = storage_format::crc32c(&data[..body_end]);
        if stored_crc != computed_crc {
            return Err(crate::Error::corruption("index checksum mismatch"));
        }

        let count = cursor.get_u32_le() as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(decode_entry(&mut cursor)?);
        }
        Ok(Self::bulk_load(entries))
    }
}

fn encode_entry(entry: &IndexedFeature, buf: &mut Vec<u8>) {
    let lower = entry.envelope.lower();
    let upper = entry.envelope.upper();
    buf.put_u32_le(entry.id.len() as u32);
    buf.extend_from_slice(&entry.id);
    buf.put_u32_le(entry.address.file_id);
    buf.put_u64_le(entry.address.offset);
    buf.put_u32_le(entry.address.len);
    for v in lower {
        buf.put_f64_le(v);
    }
    for v in upper {
        buf.put_f64_le(v);
    }
}

fn decode_entry(cursor: &mut &[u8]) -> crate::Result<IndexedFeature> {
    if cursor.len() < 4 {
        return Err(crate::Error::corruption("truncated index entry id length"));
    }
    let id_len = cursor.get_u32_le() as usize;
    if cursor.len() < id_len + 28 {
        return Err(crate::Error::corruption("truncated index entry"));
    }
    let id = cursor[..id_len].to_vec();
    cursor.advance(id_len);
    let file_id = cursor.get_u32_le();
    let offset = cursor.get_u64_le();
    let len = cursor.get_u32_le();
    let lower = [cursor.get_f64_le(), cursor.get_f64_le()];
    let upper = [cursor.get_f64_le(), cursor.get_f64_le()];
    Ok(IndexedFeature {
        envelope: AABB::from_corners(lower, upper),
        id,
        address: FeatureAddress { file_id, offset, len },
    })
}

fn geometry_envelope(geometry: &Geometry) -> Option<AABB<[f64; 2]>> {
    let (min_lon, min_lat, max_lon, max_lat) = geometry.envelope()?;
    Some(AABB::from_corners([min_lon, min_lat], [max_lon, max_lat]))
}
