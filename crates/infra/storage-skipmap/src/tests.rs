//! Tests for the lock-free skip-map.

#[cfg(test)]
mod unit {
    use super::super::SkipMap;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn empty_map() {
        let map: SkipMap<i32, i32> = SkipMap::new();
        assert!(map.is_empty());
        assert_eq!(map.get(&1), None);
        assert!(!map.contains_key(&1));
    }

    #[test]
    fn insert_and_get() {
        let map = SkipMap::new();
        assert_eq!(map.insert(1, 10), None);
        assert_eq!(map.get(&1), Some(10));
        assert!(map.contains_key(&1));
    }

    #[test]
    fn overwrite_returns_old() {
        let map = SkipMap::new();
        map.insert("a", 1);
        assert_eq!(map.insert("a", 2), Some(1));
        assert_eq!(map.get(&"a"), Some(2));
    }

    #[test]
    fn remove() {
        let map = SkipMap::new();
        map.insert(1, 10);
        assert_eq!(map.remove(&1), Some(10));
        assert_eq!(map.get(&1), None);
        assert_eq!(map.remove(&1), None);
    }

    #[test]
    fn range_sorted() {
        let map = SkipMap::new();
        map.insert(3, 30);
        map.insert(1, 10);
        map.insert(2, 20);

        let entries: Vec<_> = map.range(Some(&1), Some(&3));
        assert_eq!(entries, vec![(1, 10), (2, 20)]);
    }

    #[test]
    fn iter_sorted() {
        let map = SkipMap::new();
        map.insert(3, 30);
        map.insert(1, 10);
        map.insert(2, 20);

        let entries: Vec<_> = map.iter();
        assert_eq!(entries, vec![(1, 10), (2, 20), (3, 30)]);
    }

    #[test]
    fn matches_btree_map_sequential() {
        let map = SkipMap::new();
        let mut model = BTreeMap::new();

        for i in 0..1000 {
            map.insert(i, i * 2);
            model.insert(i, i * 2);
        }

        for i in (0..1000).step_by(3) {
            map.remove(&i);
            model.remove(&i);
        }

        for i in 0..1000 {
            assert_eq!(map.get(&i), model.get(&i).copied());
        }

        assert_eq!(
            map.iter(),
            model.iter().map(|(&k, &v)| (k, v)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn concurrent_inserts() {
        let map = Arc::new(SkipMap::new());
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let map = Arc::clone(&map);
                thread::spawn(move || {
                    for i in 0..1000 {
                        map.insert((t * 1000 + i) as i64, i);
                    }
                })
            })
            .collect();

        for handle in threads {
            handle.join().unwrap();
        }

        assert_eq!(map.len(), 8000);
        for i in 0..8000 {
            assert!(map.contains_key(&(i as i64)));
        }
    }

    #[test]
    fn concurrent_read_write() {
        let map = Arc::new(SkipMap::new());

        let writer = {
            let map = Arc::clone(&map);
            thread::spawn(move || {
                for i in 0..5000 {
                    map.insert(i, i);
                }
            })
        };

        let reader = {
            let map = Arc::clone(&map);
            thread::spawn(move || {
                for _ in 0..10000 {
                    let _ = map.get(&2500);
                }
            })
        };

        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn concurrent_insert_remove() {
        let map = Arc::new(SkipMap::new());

        let inserter = {
            let map = Arc::clone(&map);
            thread::spawn(move || {
                for i in 0..2000 {
                    map.insert(i, i);
                }
            })
        };

        let remover = {
            let map = Arc::clone(&map);
            thread::spawn(move || {
                for i in 0..2000 {
                    map.remove(&i);
                }
            })
        };

        inserter.join().unwrap();
        remover.join().unwrap();

        // The map may or may not be empty depending on interleaving,
        // but all operations should complete without crashing.
        let _ = map.len();
    }
}
