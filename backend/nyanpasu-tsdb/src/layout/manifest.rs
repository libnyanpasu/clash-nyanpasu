//! The chunk manifest, listing for each tier or stream, and each extent, its chunks. For
//! each chunk it records the kind, the sorted run, the key range, and the counts of rows and
//! bytes. An extent's version increases on every change (§7.3).
