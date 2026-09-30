//! Isolated evaluation harness. Actor implementation remains owned by the app.

#[cfg(feature = "turso")]
use nyanpasu_traffic_turso as _;

#[cfg(test)]
#[allow(dead_code)] // The workload uses only a subset of the app's typed protocol.
#[path = "../../../backend/tauri/src/core/traffic/actor.rs"]
mod actor;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../backend/tauri/src/core/traffic/client.rs"]
mod client;

#[cfg(test)]
use actor::TrafficActorArgs;
#[cfg(test)]
use client::TrafficClient;

#[cfg(test)]
mod benchmark;
