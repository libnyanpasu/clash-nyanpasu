//! The facade. `Tsdb` is held by the owner, which writes, maintains and runs the lifecycle.
//! The shared reader it hands out captures snapshots and runs queries from any thread.
