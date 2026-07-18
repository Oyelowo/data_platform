//! Slotted page implementation for the in-place B+ tree.
//!
//! This module contains a small, audited amount of `unsafe` code for the
//! Optimistic Lock Coupling (OLC) latch.  The latch word is a separate
//! `AtomicU64` field on [`Page`] and is not part of the on-disk header, so the
//! in-memory latch never needs to be derived from a shared `&[u8]` reference.
//!
//! See `UNSAFE_MANIFEST.md` for the audit entry for this module.

use std::alloc::{self, Layout};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};

use crc32c::crc32c;

use crate::error::{Error, Result};
use crate::slot::{
    OwnedCell, SLOT_SIZE, Slot, ValueKind, cell_size_with_mvcc, parse_cell, write_cell_with_mvcc,
};

/// Magic for the slotted-page format ("BTR2" little-endian).
pub const PAGE_MAGIC: u32 = 0x32_52_54_42;
/// Current format version for the slotted-page layout.
pub const PAGE_FORMAT_VERSION: u16 = 1;

/// Byte size of the slotted-page header.
///
/// The header contains only durable fields.  The in-memory OLC latch word is
/// stored as a separate `AtomicU64` on [`Page`] and is **not** part of the
/// on-disk layout.
pub const HEADER_SIZE: usize = 56;

/// Reserved flags word (bit 0 = leaf, bit 1 = internal).
pub const PAGE_FLAG_LEAF: u16 = 0x0001;
pub const PAGE_FLAG_INTERNAL: u16 = 0x0002;

/// Unique identifier for a page.
pub type PageId = u64;

/// Sentinel meaning "no page".
pub const NULL_PAGE_ID: PageId = 0;

/// Fixed header stored at the start of every slotted page.
///
/// The in-memory `latch_word` field is **not** serialized: the on-disk layout
/// reserves it as transient padding (bytes 8..15) that must be zero.  This lets
/// the OLC latch live as a separate `AtomicU64` on [`Page`] without deriving a
/// typed reference from the shared byte buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub magic: u32,
    pub version: u16,
    pub flags: u16,
    /// Optimistic-lock word: bit 0 = exclusive lock, bits 1..63 = version.
    ///
    /// This field is in-memory only; it is not written to disk.
    pub latch_word: u64,
    pub page_id: PageId,
    pub slot_count: u16,
    /// Offset where the cell area begins (i.e. the lower bound of free space).
    pub cell_start: u16,
    /// LSN of the most recent WAL record applied to this page.
    pub page_lsn: u64,
    /// Previous leaf page in the sibling chain (`NULL_PAGE_ID` if none).
    pub prev_page_id: PageId,
    /// Next leaf page in the sibling chain (`NULL_PAGE_ID` if none).
    pub next_page_id: PageId,
    /// Leftmost child page id for internal nodes (`NULL_PAGE_ID` for leaves).
    pub leftmost_child: PageId,
    pub checksum: u32,
}

impl Default for Header {
    fn default() -> Self {
        Self {
            magic: PAGE_MAGIC,
            version: PAGE_FORMAT_VERSION,
            flags: 0,
            latch_word: 0,
            page_id: NULL_PAGE_ID,
            slot_count: 0,
            cell_start: 0,
            page_lsn: 0,
            prev_page_id: NULL_PAGE_ID,
            next_page_id: NULL_PAGE_ID,
            leftmost_child: NULL_PAGE_ID,
            checksum: 0,
        }
    }
}

impl Header {
    fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(Error::Corruption("page header truncated".into()));
        }
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != PAGE_MAGIC {
            return Err(Error::Corruption(format!(
                "page magic mismatch: expected {PAGE_MAGIC:#x}, got {magic:#x}"
            )));
        }
        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != PAGE_FORMAT_VERSION {
            return Err(Error::Corruption(format!(
                "unsupported page format version {version}"
            )));
        }
        Ok(Self {
            magic,
            version,
            flags: u16::from_le_bytes([data[6], data[7]]),
            latch_word: 0,
            page_id: u64::from_le_bytes([
                data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
            ]),
            slot_count: u16::from_le_bytes([data[16], data[17]]),
            cell_start: u16::from_le_bytes([data[18], data[19]]),
            page_lsn: u64::from_le_bytes([
                data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
            ]),
            prev_page_id: u64::from_le_bytes([
                data[28], data[29], data[30], data[31], data[32], data[33], data[34], data[35],
            ]),
            next_page_id: u64::from_le_bytes([
                data[36], data[37], data[38], data[39], data[40], data[41], data[42], data[43],
            ]),
            leftmost_child: u64::from_le_bytes([
                data[44], data[45], data[46], data[47], data[48], data[49], data[50], data[51],
            ]),
            checksum: u32::from_le_bytes([data[52], data[53], data[54], data[55]]),
        })
    }

    fn encode(&self, data: &mut [u8]) {
        data[0..4].copy_from_slice(&self.magic.to_le_bytes());
        data[4..6].copy_from_slice(&self.version.to_le_bytes());
        data[6..8].copy_from_slice(&self.flags.to_le_bytes());
        data[8..16].copy_from_slice(&self.page_id.to_le_bytes());
        data[16..18].copy_from_slice(&self.slot_count.to_le_bytes());
        data[18..20].copy_from_slice(&self.cell_start.to_le_bytes());
        data[20..28].copy_from_slice(&self.page_lsn.to_le_bytes());
        data[28..36].copy_from_slice(&self.prev_page_id.to_le_bytes());
        data[36..44].copy_from_slice(&self.next_page_id.to_le_bytes());
        data[44..52].copy_from_slice(&self.leftmost_child.to_le_bytes());
        data[52..56].copy_from_slice(&self.checksum.to_le_bytes());
    }
}

/// A heap-allocated byte buffer aligned to 8 bytes.
///
/// The header contains `u64` fields (page id, LSN, pointers) and the on-disk
/// layout must be portable.  `Vec<u8>` is only 1-byte aligned, so `AlignedBytes`
/// allocates with at least 8-byte alignment.  The OLC latch word itself is a
/// separate `AtomicU64` field on [`Page`] and is no longer part of the byte
/// buffer.
struct AlignedBytes {
    ptr: *mut u8,
    len: usize,
    layout: Layout,
}

impl AlignedBytes {
    fn new_zeroed(len: usize) -> Result<Self> {
        let layout = Layout::from_size_align(len, 8)
            .map_err(|e| Error::Corruption(format!("invalid page buffer layout: {e}")))?;
        let ptr = unsafe { alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            return Err(Error::Corruption("failed to allocate page buffer".into()));
        }
        Ok(Self { ptr, len, layout })
    }

    fn from_slice(src: &[u8]) -> Result<Self> {
        let this = Self::new_zeroed(src.len())?;
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), this.ptr, src.len());
        }
        Ok(this)
    }
}

impl Clone for AlignedBytes {
    fn clone(&self) -> Self {
        Self::clone_from_raw(self.ptr, self.len)
            .expect("allocation failed while cloning page buffer")
    }
}

impl AlignedBytes {
    /// Allocate a new buffer of length `len` and copy `src` bytes into it.
    ///
    /// This is a construction-time helper: it never creates a Rust reference to
    /// an active page buffer, so it is safe to call while another thread might
    /// be racing on the source bytes as long as the caller has exclusive access
    /// to the destination.
    fn clone_from_raw(src: *const u8, len: usize) -> Result<Self> {
        let cloned = Self::new_zeroed(len)?;
        // SAFETY: `src` is valid for `len` bytes and `cloned.ptr` is a freshly
        // allocated, non-overlapping buffer of the same length.
        unsafe {
            std::ptr::copy_nonoverlapping(src, cloned.ptr, len);
        }
        Ok(cloned)
    }
}

impl Drop for AlignedBytes {
    fn drop(&mut self) {
        unsafe {
            alloc::dealloc(self.ptr, self.layout);
        }
    }
}

// SAFETY: the pointer uniquely owns the allocation and is never aliased
// outside the `Page` that contains it.  Access to the buffer is serialised by
// the page OLC latch.
unsafe impl Send for AlignedBytes {}
unsafe impl Sync for AlignedBytes {}

/// A mutable, in-memory slotted page.
///
/// The page buffer is wrapped in an [`UnsafeCell`] so that the Optimistic Lock
/// Coupling write guard can mutate the page while readers hold only a shared
/// reference.  All mutation is serialised by the page's OLC latch.
pub struct Page {
    pub id: PageId,
    data: UnsafeCell<AlignedBytes>,
    /// Optimistic-lock word: bit 0 = exclusive lock, bits 1..63 = version.
    ///
    /// Stored separately from the byte buffer so Miri never sees a `&AtomicU64`
    /// derived from a shared `&[u8]` reference.  The latch word is still
    /// excluded from the page checksum and is copied into/out of the on-disk
    /// header only at serialisation boundaries.
    latch: AtomicU64,
}

// SAFETY: mutation is serialised by the OLC latch; the AlignedBytes payload is
// Sync when treated as immutable, so Page is Sync when accessed through the
// latch helpers.
unsafe impl Sync for Page {}

impl Clone for Page {
    fn clone(&self) -> Self {
        // Copy the page buffer through raw pointers so we never create a Rust
        // reference to the source buffer while another thread could be racing
        // on it.
        let len = self.page_size();
        let data = AlignedBytes::clone_from_raw(self.buf_ptr(), len)
            .expect("allocation failed while cloning page");
        // A cloned page starts unlocked.  The in-memory latch field is reset
        // below; encoding the header already writes zeros for the (removed)
        // on-disk latch-word region.
        let mut header = self.header().expect("page header is valid");
        header.latch_word = 0;
        let mut header_buf = [0u8; HEADER_SIZE];
        header.encode(&mut header_buf);
        unsafe {
            std::ptr::copy_nonoverlapping(header_buf.as_ptr(), data.ptr, HEADER_SIZE);
        }
        let mut page_bytes = vec![0u8; len];
        unsafe {
            std::ptr::copy_nonoverlapping(data.ptr, page_bytes.as_mut_ptr(), len);
        }
        let sum = Self::compute_checksum(&page_bytes);
        unsafe {
            std::ptr::copy_nonoverlapping(sum.to_le_bytes().as_ptr(), data.ptr.add(52), 4);
        }
        Self {
            id: self.id,
            data: UnsafeCell::new(data),
            latch: AtomicU64::new(0),
        }
    }
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Page")
            .field("id", &self.id)
            .field("data_len", &self.data().len())
            .finish()
    }
}

/// Outcome of a key lookup in a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocateResult {
    /// Key was found at the given slot index.
    Found(usize),
    /// Key was not found; this is the slot index where it should be inserted.
    InsertAt(usize),
}

impl Page {
    /// Create an empty page of the given size.
    pub fn new(id: PageId, page_size: usize) -> Result<Self> {
        if page_size < HEADER_SIZE + SLOT_SIZE {
            return Err(Error::InvalidArgument(format!(
                "page size {page_size} is too small for slotted header"
            )));
        }
        if page_size > u16::MAX as usize + 1 {
            return Err(Error::InvalidArgument(
                "page size must fit in a u16 offset".into(),
            ));
        }
        let data = AlignedBytes::new_zeroed(page_size)?;
        let header = Header {
            page_id: id,
            cell_start: page_size as u16,
            ..Header::default()
        };
        let mut header_buf = [0u8; HEADER_SIZE];
        header.encode(&mut header_buf);
        unsafe {
            std::ptr::copy_nonoverlapping(header_buf.as_ptr(), data.ptr, HEADER_SIZE);
        }
        let mut page_bytes = vec![0u8; page_size];
        unsafe {
            std::ptr::copy_nonoverlapping(data.ptr, page_bytes.as_mut_ptr(), page_size);
        }
        let sum = Self::compute_checksum(&page_bytes);
        unsafe {
            std::ptr::copy_nonoverlapping(sum.to_le_bytes().as_ptr(), data.ptr.add(52), 4);
        }
        Ok(Self {
            id,
            data: UnsafeCell::new(data),
            latch: AtomicU64::new(0),
        })
    }

    /// Parse and validate a page from raw bytes.
    pub fn from_bytes(src: Vec<u8>) -> Result<Self> {
        if src.len() < HEADER_SIZE {
            return Err(Error::Corruption("page data too short".into()));
        }
        if src.len() > u16::MAX as usize + 1 {
            return Err(Error::Corruption(
                "page size exceeds u16 offset limit".into(),
            ));
        }
        let header = Header::decode(&src)?;
        let id = header.page_id;
        let expected = Self::compute_checksum(&src);
        if expected != header.checksum {
            return Err(Error::Corruption(format!(
                "page {id} checksum mismatch: expected {expected:#x}, got {:#x}",
                header.checksum
            )));
        }
        let data = AlignedBytes::from_slice(&src)?;
        // The latch word is transient in-memory state; never restore a locked
        // page from disk.  It is not part of the on-disk header.
        Ok(Self {
            id,
            data: UnsafeCell::new(data),
            latch: AtomicU64::new(0),
        })
    }

    /// Raw pointer to the start of the page buffer.
    #[inline]
    fn buf_ptr(&self) -> *mut u8 {
        // SAFETY: `UnsafeCell::get` yields a raw pointer to the `AlignedBytes`.
        // We only read its `ptr` field; no reference to the `AlignedBytes`
        // container is created.
        unsafe { (*self.data.get()).ptr }
    }

    /// Size of the page buffer in bytes.
    #[inline]
    fn page_size(&self) -> usize {
        unsafe { (*self.data.get()).len }
    }

    /// Copy `len` bytes starting at `off` into a fresh `Vec`.
    ///
    /// # Safety
    ///
    /// The caller must ensure `off + len` does not exceed `page_size()` and that
    /// no concurrent writer is mutating the source region.
    unsafe fn read_bytes(&self, off: usize, len: usize) -> Vec<u8> {
        debug_assert!(off + len <= self.page_size());
        // SAFETY: the bounds are checked by the caller and the buffer pointer
        // is valid for the lifetime of the page.
        unsafe {
            let ptr = self.buf_ptr().add(off);
            let mut v = Vec::with_capacity(len);
            std::ptr::copy_nonoverlapping(ptr, v.as_mut_ptr(), len);
            v.set_len(len);
            v
        }
    }

    /// Copy `src` into the page buffer starting at `off`.
    ///
    /// # Safety
    ///
    /// The caller must ensure `off + src.len()` does not exceed `page_size()`
    /// and that the caller holds exclusive access to the page (e.g. the OLC
    /// write latch).
    unsafe fn write_bytes(&self, off: usize, src: &[u8]) {
        debug_assert!(off + src.len() <= self.page_size());
        // SAFETY: the bounds are checked by the caller and the buffer pointer
        // is valid for the lifetime of the page.
        unsafe {
            let ptr = self.buf_ptr().add(off);
            std::ptr::copy_nonoverlapping(src.as_ptr(), ptr, src.len());
        }
    }

    /// Fill `len` bytes starting at `off` with `val`.
    ///
    /// # Safety
    ///
    /// The caller must ensure `off + len` does not exceed `page_size()` and
    /// holds exclusive access to the page.
    unsafe fn fill_bytes(&self, off: usize, len: usize, val: u8) {
        debug_assert!(off + len <= self.page_size());
        // SAFETY: the bounds are checked by the caller and the buffer pointer
        // is valid for the lifetime of the page.
        unsafe {
            let ptr = self.buf_ptr().add(off);
            std::ptr::write_bytes(ptr, val, len);
        }
    }

    /// Copy `len` bytes from buffer offset `src` to offset `dst`.
    ///
    /// The regions may overlap.
    ///
    /// # Safety
    ///
    /// The caller must ensure both regions lie inside the page and holds
    /// exclusive access to the page.
    unsafe fn copy_within(&self, src: usize, dst: usize, len: usize) {
        debug_assert!(src + len <= self.page_size());
        debug_assert!(dst + len <= self.page_size());
        // SAFETY: the bounds are checked by the caller and the buffer pointer
        // is valid for the lifetime of the page.
        unsafe {
            let ptr = self.buf_ptr();
            std::ptr::copy(ptr.add(src), ptr.add(dst), len);
        }
    }

    /// Return an owned copy of the raw page bytes.
    pub fn data(&self) -> Vec<u8> {
        unsafe { self.read_bytes(0, self.page_size()) }
    }

    /// Consume the page and return the raw bytes.
    pub fn into_vec(self) -> Vec<u8> {
        self.data()
    }

    /// Return the parsed header.
    pub fn header(&self) -> Result<Header> {
        let bytes = unsafe { self.read_bytes(0, HEADER_SIZE) };
        let mut header = Header::decode(&bytes)?;
        header.latch_word = self.latch.load(Ordering::Acquire);
        Ok(header)
    }

    /// Write a new header and recompute the checksum.
    pub fn set_header(&self, header: &Header) {
        let mut buf = [0u8; HEADER_SIZE];
        header.encode(&mut buf);
        unsafe {
            self.write_bytes(0, &buf);
        }
        let page_bytes = unsafe { self.read_bytes(0, self.page_size()) };
        let sum = Self::compute_checksum(&page_bytes);
        unsafe {
            self.write_bytes(52, &sum.to_le_bytes());
        }
    }

    // ------------------------------------------------------------------
    // Optimistic Lock Coupling primitives
    // ------------------------------------------------------------------

    /// Read the raw latch word (lock bit + version).
    pub fn latch_word(&self) -> u64 {
        self.latch.load(Ordering::Acquire)
    }

    /// True if the page is currently exclusively locked.
    pub fn is_locked(&self) -> bool {
        self.latch_word() & 1 != 0
    }

    /// Return the current version if the page is not locked.
    pub fn optimistic_version(&self) -> Option<u64> {
        let word = self.latch_word();
        if word & 1 != 0 { None } else { Some(word) }
    }

    /// Try to acquire the exclusive lock.  On success returns the version that
    /// must be passed to `unlock`.
    pub fn try_lock(&self) -> Option<u64> {
        let mut current = self.latch.load(Ordering::Relaxed);
        loop {
            if current & 1 != 0 {
                return None;
            }
            match self.latch.compare_exchange_weak(
                current,
                current | 1,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(current),
                Err(actual) => current = actual,
            }
        }
    }

    /// Release the exclusive lock and bump the version.
    ///
    /// # Safety
    ///
    /// The caller must hold the exclusive lock returned by `try_lock`.
    pub unsafe fn unlock(&self) {
        // Versions are always even.  Adding one clears the lock bit and moves
        // to the next even version (e.g. locked word 1 -> unlocked word 2).
        self.latch.fetch_add(1, Ordering::Release);
    }

    /// Create an optimistic read guard.
    pub fn optimistic(&self) -> Option<OptimisticGuard<'_>> {
        self.optimistic_version().map(|version| OptimisticGuard {
            page: self,
            version,
        })
    }

    /// Try to acquire an exclusive write guard.
    pub fn try_write(&self) -> Option<WriteGuard<'_>> {
        self.try_lock().map(|version| WriteGuard {
            page: self,
            version,
        })
    }

    fn slot_offset(idx: usize) -> usize {
        HEADER_SIZE + idx * SLOT_SIZE
    }

    pub(crate) fn read_slot(&self, idx: usize) -> Result<Slot> {
        let off = Self::slot_offset(idx);
        if off + SLOT_SIZE > self.page_size() {
            return Err(Error::Corruption("slot index out of bounds".into()));
        }
        let bytes = unsafe { self.read_bytes(off, SLOT_SIZE) };
        Ok(Slot::decode(&bytes))
    }

    pub(crate) fn write_slot(&self, idx: usize, slot: Slot) {
        let off = Self::slot_offset(idx);
        unsafe {
            self.write_bytes(off, &slot.encode());
        }
    }

    /// Number of slots (including deleted ones) currently in the directory.
    pub fn slot_count(&self) -> Result<usize> {
        Ok(self.header()?.slot_count as usize)
    }

    /// Offset where the cell area begins.
    pub fn cell_start(&self) -> Result<usize> {
        Ok(self.header()?.cell_start as usize)
    }

    /// Amount of contiguous free space between the slot directory and the cells.
    pub fn free_space(&self) -> Result<usize> {
        let header = self.header()?;
        let used_by_slots = header.slot_count as usize * SLOT_SIZE;
        let free_start = HEADER_SIZE + used_by_slots;
        let cell_start = header.cell_start as usize;
        if cell_start < free_start {
            return Err(Error::Corruption(
                "slot directory overlaps cell area".into(),
            ));
        }
        Ok(cell_start - free_start)
    }

    /// Insert a key/value cell into the page, returning the slot index.
    ///
    /// Returns `Error::PageFull` if the cell does not fit.
    pub fn insert(&self, key: &[u8], value: &ValueKind<'_>) -> Result<usize> {
        self.insert_with_mvcc(key, value, None)
    }

    /// Insert a key/value cell that may carry MVCC metadata.
    ///
    /// Returns `Error::PageFull` if the cell does not fit.
    pub fn insert_with_mvcc(
        &self,
        key: &[u8],
        value: &ValueKind<'_>,
        mvcc: Option<&crate::version::MvccHeader>,
    ) -> Result<usize> {
        let needed = cell_size_with_mvcc(key.len(), value, mvcc);
        if needed + SLOT_SIZE > self.free_space()? {
            return Err(Error::PageFull);
        }

        let idx = match self.locate(key)? {
            LocateResult::Found(idx) => {
                // Duplicate key: replace the existing cell.
                self.replace_at_with_mvcc(idx, key, value, mvcc)?;
                return Ok(idx);
            }
            LocateResult::InsertAt(idx) => idx,
        };

        let mut header = self.header()?;
        let new_cell_start = header.cell_start - needed as u16;
        let cell_off = new_cell_start as usize;
        let mut cell_buf = vec![0u8; needed];
        write_cell_with_mvcc(&mut cell_buf, key, value, mvcc)?;
        unsafe {
            self.write_bytes(cell_off, &cell_buf);
        }

        // Shift slot directory to make room for the new slot.
        let slot_off = Self::slot_offset(idx);
        let shift_len = (header.slot_count as usize - idx) * SLOT_SIZE;
        if shift_len > 0 {
            let src = slot_off;
            let dst = slot_off + SLOT_SIZE;
            unsafe {
                self.copy_within(src, dst, shift_len);
            }
        }
        self.write_slot(
            idx,
            Slot {
                offset: new_cell_start,
                len: needed as u16,
            },
        );

        header.slot_count += 1;
        header.cell_start = new_cell_start;
        self.set_header(&header);
        Ok(idx)
    }

    /// Replace the cell at `idx` with a new key/value that may carry MVCC metadata.
    fn replace_at_with_mvcc(
        &self,
        idx: usize,
        key: &[u8],
        value: &ValueKind<'_>,
        mvcc: Option<&crate::version::MvccHeader>,
    ) -> Result<()> {
        // For correctness over simplicity, we do not attempt in-place updates.
        // Delete the old slot, compact, then insert.
        self.write_slot(idx, Slot::deleted());
        self.compact()?;
        self.insert_with_mvcc(key, value, mvcc)?;
        Ok(())
    }

    /// Delete the cell for `key`. Returns true if a live cell was deleted.
    ///
    /// The page is compacted immediately so that subsequent binary searches
    /// never have to reason about deleted slots.
    pub fn delete(&self, key: &[u8]) -> Result<bool> {
        match self.locate(key)? {
            LocateResult::Found(idx) => {
                self.write_slot(idx, Slot::deleted());
                self.compact()?;
                Ok(true)
            }
            LocateResult::InsertAt(_) => Ok(false),
        }
    }

    /// Look up a key and return an owned copy of the cell if found.
    pub fn get(&self, key: &[u8]) -> Result<Option<OwnedCell>> {
        match self.locate(key)? {
            LocateResult::Found(idx) => self.get_by_slot(idx).map(Some),
            LocateResult::InsertAt(_) => Ok(None),
        }
    }

    /// Return an owned copy of the cell at the given slot index.
    pub fn get_by_slot(&self, idx: usize) -> Result<OwnedCell> {
        let slot = self.read_slot(idx)?;
        if slot.is_deleted() {
            return Err(Error::Corruption("read of deleted slot".into()));
        }
        let off = slot.offset as usize;
        let len = slot.len as usize;
        if off + len > self.page_size() {
            return Err(Error::Corruption("slot points past page end".into()));
        }
        let bytes = unsafe { self.read_bytes(off, len) };
        let cell = parse_cell(&bytes)?;
        Ok(OwnedCell {
            key: cell.key.to_vec(),
            value: cell.value.into_owned(),
            mvcc: cell.mvcc,
        })
    }

    /// Binary-search for `key`.
    pub fn locate(&self, key: &[u8]) -> Result<LocateResult> {
        let header = self.header()?;
        let mut low = 0usize;
        let mut high = header.slot_count as usize;
        while low < high {
            let mid = (low + high) / 2;
            let slot = self.read_slot(mid)?;
            if slot.is_deleted() {
                // Deleted slots do not affect ordering.  Treat as < key so the
                // binary search still converges; the caller must compact holes.
                low = mid + 1;
                continue;
            }
            let cell = self.get_by_slot(mid)?;
            match cell.key.as_slice().cmp(key) {
                std::cmp::Ordering::Equal => return Ok(LocateResult::Found(mid)),
                std::cmp::Ordering::Less => low = mid + 1,
                std::cmp::Ordering::Greater => high = mid,
            }
        }
        Ok(LocateResult::InsertAt(low))
    }

    /// Compact the page: remove deleted slots and repack cells at the end.
    pub fn compact(&self) -> Result<()> {
        let header = self.header()?;
        let mut live: Vec<(usize, Vec<u8>)> = Vec::new();
        for idx in 0..header.slot_count as usize {
            let slot = self.read_slot(idx)?;
            if !slot.is_deleted() {
                let off = slot.offset as usize;
                let len = slot.len as usize;
                live.push((idx, unsafe { self.read_bytes(off, len) }));
            }
        }

        let page_size = self.page_size();
        let mut new_header = Header {
            slot_count: live.len() as u16,
            cell_start: page_size as u16,
            ..header
        };

        // Clear slot directory area (not strictly required, but makes torn
        // pages easier to detect).
        let slot_dir_end = HEADER_SIZE + new_header.slot_count as usize * SLOT_SIZE;
        unsafe {
            self.fill_bytes(HEADER_SIZE, slot_dir_end - HEADER_SIZE, 0);
        }

        for (new_idx, (_old_idx, cell_bytes)) in live.iter().enumerate().rev() {
            let len = cell_bytes.len();
            new_header.cell_start -= len as u16;
            let off = new_header.cell_start as usize;
            unsafe {
                self.write_bytes(off, cell_bytes);
            }
            self.write_slot(
                new_idx,
                Slot {
                    offset: new_header.cell_start,
                    len: len as u16,
                },
            );
        }

        self.set_header(&new_header);
        Ok(())
    }

    /// Maximum key length among live cells in the page.
    pub fn max_key_len(&self) -> Result<usize> {
        let count = self.slot_count()?;
        let mut max = 0usize;
        for idx in 0..count {
            if self.read_slot(idx)?.is_deleted() {
                continue;
            }
            let cell = self.get_by_slot(idx)?;
            if cell.key.len() > max {
                max = cell.key.len();
            }
        }
        Ok(max)
    }

    /// Number of live (non-deleted) slots.
    pub fn live_count(&self) -> Result<usize> {
        let count = self.slot_count()?;
        let mut live = 0;
        for idx in 0..count {
            if !self.read_slot(idx)?.is_deleted() {
                live += 1;
            }
        }
        Ok(live)
    }

    /// True if the page has the leaf flag set.
    pub fn is_leaf(&self) -> bool {
        self.header().is_ok_and(|h| (h.flags & PAGE_FLAG_LEAF) != 0)
    }

    /// True if the page has the internal flag set.
    pub fn is_internal(&self) -> bool {
        self.header()
            .is_ok_and(|h| (h.flags & PAGE_FLAG_INTERNAL) != 0)
    }

    /// Set the leaf flag and clear the internal flag.
    pub fn set_leaf(&self) {
        let mut header = self.header().expect("page header is valid");
        header.flags = PAGE_FLAG_LEAF;
        self.set_header(&header);
    }

    /// Set the internal flag and clear the leaf flag.
    pub fn set_internal(&self) {
        let mut header = self.header().expect("page header is valid");
        header.flags = PAGE_FLAG_INTERNAL;
        self.set_header(&header);
    }

    /// Previous sibling page id.
    pub fn prev_page_id(&self) -> Result<PageId> {
        Ok(self.header()?.prev_page_id)
    }

    /// Set the previous sibling page id.
    pub fn set_prev_page_id(&self, id: PageId) {
        let mut header = self.header().expect("page header is valid");
        header.prev_page_id = id;
        self.set_header(&header);
    }

    /// Next sibling page id.
    pub fn next_page_id(&self) -> Result<PageId> {
        Ok(self.header()?.next_page_id)
    }

    /// Set the next sibling page id.
    pub fn set_next_page_id(&self, id: PageId) {
        let mut header = self.header().expect("page header is valid");
        header.next_page_id = id;
        self.set_header(&header);
    }

    /// Leftmost child page id (internal nodes only).
    pub fn leftmost_child(&self) -> Result<PageId> {
        Ok(self.header()?.leftmost_child)
    }

    /// Set the leftmost child page id.
    pub fn set_leftmost_child(&self, id: PageId) {
        let mut header = self.header().expect("page header is valid");
        header.leftmost_child = id;
        self.set_header(&header);
    }

    /// Iterate over live cells in key order, returning owned copies.
    pub fn iter(&self) -> Result<impl Iterator<Item = OwnedCell> + '_> {
        let count = self.slot_count()?;
        Ok((0..count).filter_map(move |idx| {
            if self.read_slot(idx).ok()?.is_deleted() {
                return None;
            }
            self.get_by_slot(idx).ok()
        }))
    }

    fn compute_checksum(data: &[u8]) -> u32 {
        // Exclude only the checksum field (bytes 52..56) from the checksum.  The
        // in-memory OLC latch word is no longer part of the on-disk layout, so
        // there is no transient region to skip.
        let mut body = Vec::with_capacity(data.len() - 4);
        body.extend_from_slice(&data[..52]);
        body.extend_from_slice(&data[56..]);
        crc32c(&body)
    }
}

/// An optimistic read guard for a page.
///
/// The guard captures a latch version.  Use [`OptimisticGuard::read`] to run a
/// read-only closure on the page; the closure's result is returned only if the
/// version remained stable for the entire call.
pub struct OptimisticGuard<'a> {
    page: &'a Page,
    version: u64,
}

impl<'a> OptimisticGuard<'a> {
    /// Run `f` on the page and return its result only if the latch version did
    /// not change while `f` executed and the page was not locked.
    pub fn read<R>(&self, f: impl FnOnce(&Page) -> R) -> Option<R> {
        if self.page.latch_word() != self.version {
            return None;
        }
        let result = f(self.page);
        if self.page.latch_word() == self.version {
            Some(result)
        } else {
            None
        }
    }

    /// True if the captured version is still current.
    pub fn validate(&self) -> bool {
        self.page.latch_word() == self.version
    }

    /// Return the captured latch version.
    pub fn version(&self) -> u64 {
        self.version
    }
}

/// An exclusive write guard for a page.
///
/// The guard holds the page's exclusive OLC lock.  Mutating access is available
/// through [`WriteGuard::page`]: all `Page` mutation methods are `&self` and
/// rely on the exclusive latch for serialisation.  The lock is released and the
/// version is bumped when the guard is dropped.
pub struct WriteGuard<'a> {
    page: &'a Page,
    version: u64,
}

impl<'a> WriteGuard<'a> {
    /// Access to the page.  The guard holds the exclusive OLC latch, so
    /// mutating methods may be called through the returned reference.
    pub fn page(&self) -> &Page {
        self.page
    }

    /// The latch version observed when the lock was acquired.  This is the
    /// version that will be visible to optimistic readers after the guard is
    /// dropped.
    pub fn version(&self) -> u64 {
        self.version
    }
}

impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: the guard holds the exclusive latch.
        unsafe {
            self.page.unlock();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slot::OwnedValue;

    #[test]
    fn empty_page_roundtrip() {
        let page = Page::new(7, 4096).unwrap();
        assert_eq!(page.id, 7);
        let bytes = page.into_vec();
        let page = Page::from_bytes(bytes).unwrap();
        assert_eq!(page.id, 7);
        assert_eq!(page.slot_count().unwrap(), 0);
        assert_eq!(page.cell_start().unwrap(), 4096);
    }

    #[test]
    fn insert_and_get() {
        let page = Page::new(1, 512).unwrap();
        page.insert(b"a", &ValueKind::Inline(b"1")).unwrap();
        page.insert(b"b", &ValueKind::Inline(b"2")).unwrap();

        let cell = page.get(b"a").unwrap().unwrap();
        let view = cell.as_cell();
        assert_eq!(view.key, b"a");
        assert_eq!(view.value, ValueKind::Inline(b"1"));

        let cell = page.get(b"b").unwrap().unwrap();
        assert_eq!(cell.value, OwnedValue::Inline(b"2".to_vec()));
        assert!(page.get(b"c").unwrap().is_none());
    }

    #[test]
    fn insert_maintains_order() {
        let page = Page::new(1, 1024).unwrap();
        page.insert(b"z", &ValueKind::Inline(b"last")).unwrap();
        page.insert(b"a", &ValueKind::Inline(b"first")).unwrap();
        page.insert(b"m", &ValueKind::Inline(b"mid")).unwrap();

        let keys: Vec<_> = page.iter().unwrap().map(|c| c.key.to_vec()).collect();
        assert_eq!(keys, vec![b"a".to_vec(), b"m".to_vec(), b"z".to_vec()]);
    }

    #[test]
    fn duplicate_put_overwrites() {
        let page = Page::new(1, 512).unwrap();
        page.insert(b"k", &ValueKind::Inline(b"v1")).unwrap();
        page.insert(b"k", &ValueKind::Inline(b"v2")).unwrap();
        let cell = page.get(b"k").unwrap().unwrap();
        assert_eq!(cell.value, OwnedValue::Inline(b"v2".to_vec()));
    }

    #[test]
    fn delete_and_compact() {
        let page = Page::new(1, 1024).unwrap();
        page.insert(b"a", &ValueKind::Inline(b"1")).unwrap();
        page.insert(b"b", &ValueKind::Inline(b"2")).unwrap();
        page.insert(b"c", &ValueKind::Inline(b"3")).unwrap();

        assert!(page.delete(b"b").unwrap());
        assert!(!page.delete(b"x").unwrap());
        assert_eq!(page.slot_count().unwrap(), 2); // auto-compacted

        page.compact().unwrap();
        assert_eq!(page.slot_count().unwrap(), 2);
        assert!(page.get(b"b").unwrap().is_none());
        assert_eq!(
            page.get(b"a").unwrap().unwrap().value,
            OwnedValue::Inline(b"1".to_vec())
        );
        assert_eq!(
            page.get(b"c").unwrap().unwrap().value,
            OwnedValue::Inline(b"3".to_vec())
        );
    }

    #[test]
    fn page_full_detected() {
        let page = Page::new(1, 512).unwrap();
        // A 500-byte inline value plus overhead does not fit in a 512-byte page.
        let big_value = vec![0u8; 500];
        let result = page.insert(b"k", &ValueKind::Inline(&big_value));
        assert!(matches!(result, Err(Error::PageFull)));
    }

    #[test]
    fn checksum_failure_detected() {
        let page = Page::new(1, 512).unwrap();
        let mut bytes = page.into_vec();
        bytes[60] ^= 0xFF;
        assert!(Page::from_bytes(bytes).is_err());
    }

    #[test]
    fn value_log_reference_roundtrip() {
        let page = Page::new(1, 512).unwrap();
        page.insert(
            b"big",
            &ValueKind::ValueLog {
                offset: 12345,
                len: 999,
            },
        )
        .unwrap();
        let cell = page.get(b"big").unwrap().unwrap();
        assert_eq!(
            cell.value,
            OwnedValue::ValueLog {
                offset: 12345,
                len: 999
            }
        );
    }

    #[test]
    fn optimistic_read_sees_stable_version() {
        let page = Page::new(1, 512).unwrap();
        page.insert(b"k", &ValueKind::Inline(b"v")).unwrap();
        let guard = page.optimistic().unwrap();
        let value = guard
            .read(|p| p.get(b"k").unwrap().map(|c| c.value))
            .unwrap();
        assert_eq!(value, Some(OwnedValue::Inline(b"v".to_vec())));
    }

    #[test]
    fn optimistic_read_retries_after_write() {
        let page = Page::new(1, 512).unwrap();
        page.insert(b"k", &ValueKind::Inline(b"v1")).unwrap();
        let guard = page.optimistic().unwrap();

        let w = page.try_write().unwrap();
        w.page().insert(b"k", &ValueKind::Inline(b"v2")).unwrap();
        drop(w);

        assert!(!guard.validate());
        // Retry with a fresh optimistic guard until we observe a stable version.
        let value = loop {
            let guard = page.optimistic().unwrap();
            if let Some(v) = guard.read(|p| p.get(b"k").unwrap().map(|c| c.value)) {
                break v;
            }
        };
        assert_eq!(value, Some(OwnedValue::Inline(b"v2".to_vec())));
    }

    #[test]
    fn write_guard_bumps_version_on_drop() {
        let page = Page::new(1, 512).unwrap();
        let v0 = page.optimistic_version().unwrap();
        {
            let _w = page.try_write().unwrap();
        }
        let v1 = page.optimistic_version().unwrap();
        assert_eq!(v1, v0 + 2);
    }
}
