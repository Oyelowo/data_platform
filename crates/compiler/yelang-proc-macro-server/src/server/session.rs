//! Per-client session state.

use slotmap::SlotMap;
use yelang_proc_macro_bridge::protocol::LibraryHandle;

use super::library::LoadedLibrary;

/// Session state for one compiler connection.
pub struct Session {
    libraries: SlotMap<LibraryHandle, LoadedLibrary>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            libraries: SlotMap::with_key(),
        }
    }

    pub fn insert_library(&mut self, library: LoadedLibrary) -> LibraryHandle {
        self.libraries.insert(library)
    }

    pub fn get_library(&self, handle: LibraryHandle) -> Option<&LoadedLibrary> {
        self.libraries.get(handle)
    }

    pub fn remove_library(&mut self, handle: LibraryHandle) -> Option<LoadedLibrary> {
        self.libraries.remove(handle)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
