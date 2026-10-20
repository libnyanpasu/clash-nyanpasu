//! Event row buffers. A buffer becomes an L0 chunk at 4,096 rows, at 64 KB, or once its
//! oldest row is older than T (§4.2).
