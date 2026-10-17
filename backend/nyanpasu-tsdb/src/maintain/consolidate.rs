//! Consolidation:
//!
//! - an extent's L0 chunks become compressed chunks;
//! - the delta chunks of a consolidated extent merge into one delta chunk;
//! - sorted runs merge once there are more than R of them (§4.3).
