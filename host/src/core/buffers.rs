//! Multiple buffers owned by the editing core.
//!
//! The first window lane only needs the model to exist; protocol behaviour
//! still goes through the focused buffer exactly as before.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::buffer::Buffer;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct BufferId(pub u64);

pub struct BufferEntry {
    pub id: BufferId,
    pub buffer: Buffer,
    pub listed: bool,
    pub scratch_name: Option<String>,
}

impl BufferEntry {
    fn path(&self) -> Option<&Path> {
        self.buffer.path()
    }
}

pub struct BufferStore {
    next: u64,
    current: BufferId,
    alternate: Option<BufferId>,
    order: Vec<BufferId>,
    entries: BTreeMap<BufferId, BufferEntry>,
}

impl BufferStore {
    pub fn new(buffer: Buffer) -> Self {
        let id = BufferId(1);
        let mut entries = BTreeMap::new();
        entries.insert(
            id,
            BufferEntry {
                id,
                buffer,
                listed: true,
                scratch_name: None,
            },
        );
        Self {
            next: 2,
            current: id,
            alternate: None,
            order: vec![id],
            entries,
        }
    }

    pub fn current_id(&self) -> BufferId {
        self.current
    }

    pub fn alternate_id(&self) -> Option<BufferId> {
        self.alternate
    }

    pub fn current(&self) -> &Buffer {
        &self.entries[&self.current].buffer
    }

    pub fn current_mut(&mut self) -> &mut Buffer {
        &mut self.entries.get_mut(&self.current).unwrap().buffer
    }

    pub fn get(&self, id: BufferId) -> Option<&Buffer> {
        self.entries.get(&id).map(|entry| &entry.buffer)
    }

    pub fn get_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.entries.get_mut(&id).map(|entry| &mut entry.buffer)
    }

    pub fn entry(&self, id: BufferId) -> Option<&BufferEntry> {
        self.entries.get(&id)
    }

    pub fn list(&self) -> Vec<BufferId> {
        self.order
            .iter()
            .copied()
            .filter(|id| self.entries.get(id).is_some_and(|entry| entry.listed))
            .collect()
    }

    pub fn by_index(&self, index: usize) -> Option<BufferId> {
        self.list().get(index).copied()
    }

    pub fn set_current(&mut self, id: BufferId) -> bool {
        if !self.entries.contains_key(&id) {
            return false;
        }
        if self.current != id {
            self.alternate = Some(self.current);
            self.current = id;
        }
        true
    }

    pub fn next(&mut self) -> Option<BufferId> {
        let listed = self.list();
        let at = listed.iter().position(|id| *id == self.current)?;
        let id = listed[(at + 1) % listed.len()];
        self.set_current(id);
        Some(id)
    }

    pub fn prev(&mut self) -> Option<BufferId> {
        let listed = self.list();
        let at = listed.iter().position(|id| *id == self.current)?;
        let id = listed[(at + listed.len() - 1) % listed.len()];
        self.set_current(id);
        Some(id)
    }

    pub fn open_or_reuse(&mut self, path: &Path) -> std::io::Result<BufferId> {
        if let Some(id) = self.id_for_path(path) {
            self.set_current(id);
            return Ok(id);
        }
        let buffer = Buffer::open(path)?;
        Ok(self.insert(buffer, true, None))
    }

    pub fn scratch(&mut self, name: impl Into<String>) -> BufferId {
        self.insert(Buffer::empty(), false, Some(name.into()))
    }

    pub fn empty(&mut self) -> BufferId {
        self.insert(Buffer::empty(), true, None)
    }

    pub fn delete(&mut self, id: BufferId) -> bool {
        if self.entries.len() == 1 || !self.entries.contains_key(&id) {
            return false;
        }
        self.entries.remove(&id);
        self.order.retain(|candidate| *candidate != id);
        if self.alternate == Some(id) {
            self.alternate = None;
        }
        if self.current == id {
            self.current = self.order[0];
        }
        true
    }

    fn insert(&mut self, buffer: Buffer, listed: bool, scratch_name: Option<String>) -> BufferId {
        let id = BufferId(self.next);
        self.next += 1;
        self.order.push(id);
        self.entries.insert(
            id,
            BufferEntry {
                id,
                buffer,
                listed,
                scratch_name,
            },
        );
        self.set_current(id);
        id
    }

    fn id_for_path(&self, path: &Path) -> Option<BufferId> {
        let target = normalize_path(path);
        self.order.iter().copied().find(|id| {
            self.entries
                .get(id)
                .and_then(BufferEntry::path)
                .map(normalize_path)
                == Some(target.clone())
        })
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_the_same_path_reuses_the_buffer() {
        let dir =
            std::env::temp_dir().join(format!("nvimglsl-buffer-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("one.txt");
        std::fs::write(&path, "one\n").unwrap();

        let mut store = BufferStore::new(Buffer::empty());
        let first = store.open_or_reuse(&path).unwrap();
        let second = store.open_or_reuse(&path).unwrap();
        assert_eq!(first, second);
        assert_eq!(store.list(), vec![BufferId(1), first]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn alternate_tracks_the_previous_buffer() {
        let mut store = BufferStore::new(Buffer::from_text("one\n"));
        let scratch = store.scratch("tree");
        assert_eq!(store.current_id(), scratch);
        assert_eq!(store.alternate_id(), Some(BufferId(1)));
        assert!(store.entry(scratch).unwrap().scratch_name.is_some());
        assert!(store.list().contains(&BufferId(1)));
        assert!(!store.list().contains(&scratch));
    }

    #[test]
    fn next_prev_and_delete_follow_listed_order() {
        let mut store = BufferStore::new(Buffer::from_text("one\n"));
        let two = store.insert(Buffer::from_text("two\n"), true, None);
        let three = store.insert(Buffer::from_text("three\n"), true, None);
        assert_eq!(store.by_index(1), Some(two));
        assert_eq!(store.next(), Some(BufferId(1)));
        assert_eq!(store.prev(), Some(three));
        assert!(store.delete(two));
        assert_eq!(store.list(), vec![BufferId(1), three]);
    }
}
