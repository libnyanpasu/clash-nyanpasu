//! The chunk reader. It reads fragment 0, then only the fragments that hold the column blocks
//! a query needs. Once a block is validated, it is read through zerocopy's unaligned
//! little-endian types.
