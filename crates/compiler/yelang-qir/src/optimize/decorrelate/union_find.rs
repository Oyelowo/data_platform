//! Union-Find (disjoint set) over column symbols.

use yelang_arena::FxHashMap;
use yelang_interner::Symbol;

/// Union-Find (disjoint set) over column symbols.
///
/// Tracks equivalence classes of columns derived from join predicates
/// (e.g. `a.x = b.y` makes `x` and `y` equivalent). Used during
/// decorrelation to decide whether an outer reference can be substituted
/// with an already-bound inner column (avoiding a domain join).
#[derive(Debug, Default)]
pub struct UnionFind {
    parent: FxHashMap<Symbol, Symbol>,
}

impl UnionFind {
    pub fn new() -> Self {
        Self {
            parent: FxHashMap::default(),
        }
    }

    /// Find the representative of the equivalence class containing `x`.
    /// Applies path compression.
    pub fn find(&mut self, x: Symbol) -> Symbol {
        let parent = self.parent.get(&x).copied();
        match parent {
            None => x,
            Some(p) if p == x => x,
            Some(p) => {
                let root = self.find(p);
                self.parent.insert(x, root);
                root
            }
        }
    }

    /// Merge the equivalence classes of `a` and `b`.
    pub fn union(&mut self, a: Symbol, b: Symbol) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }

    /// Check whether two symbols are in the same equivalence class.
    pub fn equivalent(&mut self, a: Symbol, b: Symbol) -> bool {
        self.find(a) == self.find(b)
    }

    /// Merge all equivalences from another union-find into this one.
    pub fn merge(&mut self, other: &UnionFind) {
        for (&k, &v) in other.parent.iter() {
            self.union(k, v);
        }
    }
}
