//! Small in-memory TTL cache, so the dashboard, its refreshes and MCP tool
//! calls for the same place share one upstream request.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct TtlCache<K, V> {
    ttl: Duration,
    capacity: usize,
    entries: Mutex<HashMap<K, (Instant, V)>>,
}

impl<K: Eq + Hash + Clone, V: Clone> TtlCache<K, V> {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            ttl,
            capacity: capacity.max(1),
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let entries = self.entries.lock().expect("cache lock poisoned");
        entries
            .get(key)
            .filter(|(at, _)| at.elapsed() < self.ttl)
            .map(|(_, v)| v.clone())
    }

    pub fn insert(&self, key: K, value: V) {
        let mut entries = self.entries.lock().expect("cache lock poisoned");
        if entries.len() >= self.capacity && !entries.contains_key(&key) {
            entries.retain(|_, (at, _)| at.elapsed() < self.ttl);
            if entries.len() >= self.capacity {
                if let Some(oldest) = entries
                    .iter()
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(k, _)| k.clone())
                {
                    entries.remove(&oldest);
                }
            }
        }
        entries.insert(key, (Instant::now(), value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expires_entries() {
        let c = TtlCache::new(Duration::from_millis(20), 10);
        c.insert(1, "a");
        assert_eq!(c.get(&1), Some("a"));
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(c.get(&1), None);
    }

    #[test]
    fn evicts_oldest_at_capacity() {
        let c = TtlCache::new(Duration::from_secs(60), 2);
        c.insert(1, 1);
        std::thread::sleep(Duration::from_millis(2));
        c.insert(2, 2);
        c.insert(3, 3);
        assert_eq!(c.get(&1), None);
        assert_eq!(c.get(&2), Some(2));
        assert_eq!(c.get(&3), Some(3));
    }
}
