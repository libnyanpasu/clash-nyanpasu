//! A label dictionary: values in a byte heap with offsets, looked up through an
//! open-addressing table of `u32` indices. Id 0 is the overflow marker and never a user
//! value (§5.2).
