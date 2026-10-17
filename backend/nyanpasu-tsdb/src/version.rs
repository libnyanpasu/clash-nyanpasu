//! The version registry (§7.1): a read-write lock over the current version and at most one
//! pending version. Readers capture a snapshot under the read lock: open the storage
//! snapshot, read its publication Q, and pin the catalog and head of Q. The writer never
//! holds the lock across a commit, so an fsync never blocks a query.
