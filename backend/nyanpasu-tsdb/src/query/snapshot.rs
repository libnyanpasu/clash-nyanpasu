//! Snapshots. A snapshot holds:
//!
//! - the storage snapshot, plus the catalog and head of one publication;
//! - a budget reservation sized to the bytes of that head and catalog at capture.
//!
//! The accessors here read the snapshot's catalog, so that a consumer can build filters from
//! label values and render the label ids of group keys and combinations. Ids are those of
//! this snapshot's catalog: after a release, a later one may give an id to another value
//! (§5.3). A result outlives its snapshot and keeps the snapshot's
//! [`catalog`](Snapshot::catalog) token: render its ids, and build filters from them, through
//! a snapshot whose token equals it.
//!
//! Retirement revokes queries at coarse work boundaries (§7.4, §7.5), including scans entirely
//! in the head. Frozen catalog accessors remain usable; they perform no backend work.

use crate::{LabelId, Snapshot, storage::Storage, write::ComboId};

impl<S: Storage> Snapshot<S> {
    /// The value of label `id` of `dimension`, as it was written. `None` for the overflow
    /// marker, 0, for an id the catalog does not hold, and for a label the schema does not
    /// have. An id of a result means this value only when the result's catalog token equals
    /// this snapshot's [`catalog`](Self::catalog).
    pub fn label(&self, dimension: LabelId, id: u32) -> Option<&[u8]> {
        self.has_label(dimension)
            .then(|| self.version().catalog.label(dimension, id))
            .flatten()
    }

    /// The id of `value` in `dimension`, if the catalog holds it. The id, and a filter built
    /// from it, name `value` only in a snapshot whose [`catalog`](Self::catalog) equals this
    /// one's.
    pub fn find_label(&self, dimension: LabelId, value: &[u8]) -> Option<u32> {
        self.has_label(dimension)
            .then(|| self.version().catalog.find_label(dimension, value))
            .flatten()
    }

    /// The label ids of combination `combo`, one per series dimension in
    /// [`SeriesSpec::dimensions`](crate::SeriesSpec::dimensions) order. A dimension the fold
    /// ladder replaced holds the overflow marker, 0, and combination 0, the whole overflow,
    /// holds 0 in every one (§5.2). `None` for an id the catalog does not hold. A result's
    /// `combo` means these labels only when the result's catalog token equals this snapshot's
    /// [`catalog`](Self::catalog); the label ids returned are this snapshot's.
    pub fn combination(&self, combo: ComboId) -> Option<impl ExactSizeIterator<Item = u32> + '_> {
        self.version().catalog.combo(combo.get())
    }

    fn has_label(&self, dimension: LabelId) -> bool {
        usize::from(dimension.0) < self.schema().labels.len()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        LabelId,
        db::tests::{LABELS, PROCESS, RULE, TARGET, open, point, reopen},
        query::series::tests::{at, write},
        storage::memory::MemoryDisk,
        write::ComboId,
    };

    #[test]
    fn labels_and_combinations_read_the_snapshots_catalog() {
        let disk = MemoryDisk::new();
        let mut tsdb = open(&disk);
        let combo = write(&mut tsdb, 1, &[at(point(LABELS, &[1, 2]), 0)])[0];
        let snapshot = tsdb.reader().capture().unwrap();

        let ids: Vec<u32> = snapshot.combination(combo).unwrap().collect();
        let values: Vec<&[u8]> = [RULE, PROCESS, TARGET]
            .into_iter()
            .zip(&ids)
            .map(|(dimension, &id)| snapshot.label(dimension, id).unwrap())
            .collect();
        assert_eq!(values, LABELS);
        for (dimension, (&value, &id)) in [RULE, PROCESS, TARGET]
            .into_iter()
            .zip(LABELS.iter().zip(&ids))
        {
            assert_eq!(snapshot.find_label(dimension, value), Some(id));
        }

        // The whole overflow, the overflow marker, and what the catalog does not hold.
        let overflow: Vec<u32> = snapshot.combination(ComboId::new(0)).unwrap().collect();
        assert_eq!(overflow, [0, 0, 0]);
        assert_eq!(snapshot.label(RULE, 0), None);
        assert!(
            snapshot
                .combination(ComboId::new(combo.get() + 1))
                .is_none()
        );
        assert_eq!(snapshot.find_label(RULE, b"curl"), None);
        assert_eq!(snapshot.label(RULE, ids[0] + 1), None);
        // A label the schema does not have is no value, not a panic.
        let missing = LabelId(99);
        assert_eq!(snapshot.label(missing, 1), None);
        assert_eq!(snapshot.find_label(missing, b"match"), None);
    }

    // A reopen loads the catalog at generation 0 again, under a new open's marker, so its token
    // differs although it holds the same ids (§5.3).
    #[test]
    fn a_reopen_changes_the_catalog_token() {
        let disk = MemoryDisk::new();
        let mut tsdb = open(&disk);
        write(&mut tsdb, 1, &[at(point(LABELS, &[1, 2]), 0)]);
        let before = tsdb.reader().capture().unwrap().catalog();
        assert_eq!(tsdb.reader().capture().unwrap().catalog(), before);

        let tsdb = reopen(tsdb, &disk);
        let after = tsdb.reader().capture().unwrap();
        assert_ne!(after.catalog(), before);
        assert!(after.find_label(RULE, b"match").is_some());
    }

    // Two stores opened apart both load their catalogs at generation 0 and give their first
    // values id 1, yet their tokens differ: a result of one does not resolve through the other.
    #[test]
    fn independent_stores_have_different_catalog_tokens() {
        const OTHER: &[&[u8]] = &[b"direct", b"wget", b"b.com"];
        let mut one = open(&MemoryDisk::new());
        let mut two = open(&MemoryDisk::new());
        write(&mut one, 1, &[at(point(LABELS, &[1, 2]), 0)]);
        write(&mut two, 1, &[at(point(OTHER, &[1, 2]), 0)]);
        let one = one.reader().capture().unwrap();
        let two = two.reader().capture().unwrap();

        assert_eq!(one.find_label(RULE, b"match"), Some(1));
        assert_eq!(two.find_label(RULE, b"direct"), Some(1));
        assert_eq!(
            one.version().catalog.generation(),
            two.version().catalog.generation()
        );
        assert_ne!(one.catalog(), two.catalog());
    }
}
