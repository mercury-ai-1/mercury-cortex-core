//! LRU cache for file metadata entries.
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::engine::index::runtime_index::FileEntry;

const MAX_CACHE_ENTRIES: usize = 1000;

/// LRU cache for file metadata entries.
#[derive(Clone, Debug)]
pub(crate) struct FileMetadataCache {
    inner: Arc<RwLock<LruCache>>,
}

#[derive(Debug)]
struct LruCache {
    map: HashMap<String, FileEntry>,
    order: VecDeque<String>,
    capacity: usize,
}

impl LruCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn get(&mut self, key: &str) -> Option<FileEntry> {
        if self.map.contains_key(key) {
            self.move_to_front(key);
            self.map.get(key).cloned()
        } else {
            None
        }
    }

    fn put(&mut self, key: String, value: FileEntry) {
        if self.map.contains_key(&key) {
            self.map.insert(key.clone(), value);
            self.move_to_front(&key);
            return;
        }
        if self.map.len() >= self.capacity
            && let Some(evicted) = self.order.pop_back()
        {
            self.map.remove(&evicted);
        }

        self.map.insert(key.clone(), value);
        self.order.push_front(key);
    }

    fn move_to_front(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            let k = self.order.remove(pos).unwrap();
            self.order.push_front(k);
        }
    }
}

impl FileMetadataCache {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(LruCache::new(MAX_CACHE_ENTRIES))),
        }
    }

    pub async fn get(&self, key: &str) -> Option<FileEntry> {
        self.inner.write().await.get(key)
    }

    pub async fn put(&self, key: String, value: FileEntry) {
        self.inner.write().await.put(key, value);
    }
}

impl Default for FileMetadataCache {
    fn default() -> Self {
        Self::new()
    }
}
