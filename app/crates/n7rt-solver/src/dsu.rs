//! Union-find whose representative is always the smallest index of the set: components are then named
//! deterministically, and "smallest node of a component" is its root.

#[derive(Clone, Debug, Default)]
pub(crate) struct Dsu {
    parent: Vec<usize>,
}

impl Dsu {
    pub(crate) fn new(n: usize) -> Self {
        Dsu { parent: (0..n).collect() }
    }

    pub(crate) fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            let grand = self.parent[self.parent[x]];
            self.parent[x] = grand;
            x = grand;
        }
        x
    }

    /// Joins the sets of `a` and `b`. Returns false if they were already joined.
    pub(crate) fn union(&mut self, a: usize, b: usize) -> bool {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return false;
        }
        if ra < rb {
            self.parent[rb] = ra;
        } else {
            self.parent[ra] = rb;
        }
        true
    }

    /// Representative (smallest member) of every element.
    pub(crate) fn roots(&mut self) -> Vec<usize> {
        (0..self.parent.len()).map(|i| self.find(i)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Dsu;

    #[test]
    fn root_is_smallest_member() {
        let mut d = Dsu::new(6);
        d.union(5, 3);
        d.union(3, 4);
        d.union(2, 0);
        assert!(!d.union(4, 5));
        assert_eq!(d.roots(), vec![0, 1, 0, 3, 3, 3]);
        d.union(4, 2);
        assert_eq!(d.roots(), vec![0, 1, 0, 0, 0, 0]);
    }
}
