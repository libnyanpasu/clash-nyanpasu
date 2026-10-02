//! Converts the raw Clash connections feed into accounting frames.
use std::time::Duration;

use clash_api::{ConfigEnum, Connection, ConnectionNetwork};
use nyanpasu_traffic::{Bytes, Dimensions, Frame, RuleKey, Sample};

use super::geo::{CountryLookup, locate_destination, locate_source};
use crate::core::clash::ws::ClashConnectionsFrame;

const UNKNOWN: &str = "unknown";

pub(crate) fn frame_from_snapshot(
    frame: &ClashConnectionsFrame,
    wall_ms: i64,
    mono: Duration,
    index: Option<&dyn CountryLookup>,
) -> Frame {
    let snapshot = &frame.snapshot;
    Frame {
        instance_id: frame.instance_id.clone(),
        wall_ms,
        mono,
        totals: counters(snapshot.upload_total, snapshot.download_total),
        connections: snapshot
            .connections
            .iter()
            .flatten()
            .map(|connection| sample(connection, index))
            .collect(),
    }
}

fn sample(connection: &Connection, index: Option<&dyn CountryLookup>) -> Sample {
    Sample {
        id: connection.id.to_string(),
        started_at: connection.start.timestamp_millis(),
        counters: counters(connection.upload, connection.download),
        dimensions: dimensions(connection, index),
    }
}

/// The core never reports negative counters; clamp rather than trust it.
fn counters(upload: i64, download: i64) -> Bytes {
    Bytes {
        upload: u64::try_from(upload).unwrap_or(0),
        download: u64::try_from(download).unwrap_or(0),
    }
}

fn dimensions(connection: &Connection, index: Option<&dyn CountryLookup>) -> Dimensions {
    let known = connection.metadata.as_ref().map(|meta| &meta.known);
    let process = known
        .and_then(|m| text(m.process_path.as_ref()))
        .or_else(|| known.and_then(|m| text(m.process.as_ref())));
    let target = known
        .and_then(|m| text(m.host.as_ref()))
        .or_else(|| known.and_then(|m| text(m.destination_ip.as_ref())));
    let inbound = known
        .and_then(|m| text(m.inbound_user.as_ref()))
        .or_else(|| known.and_then(|m| text(m.inbound_name.as_ref())));
    let (destination_region, destination_basis) = known.map_or_else(
        || (UNKNOWN.to_owned(), None),
        |m| locate_destination(m, connection.chains.first().map(String::as_str), index),
    );
    Dimensions {
        process: process.unwrap_or(UNKNOWN).replace('\\', "/"),
        source: known
            .and_then(|m| text(m.source_ip.as_ref()))
            .unwrap_or(UNKNOWN)
            .to_owned(),
        target: target.unwrap_or(UNKNOWN).to_owned(),
        protocol: known
            .and_then(|m| m.network.as_ref())
            .map(network)
            .filter(|value| !value.is_empty())
            .unwrap_or(UNKNOWN)
            .to_owned(),
        rule: RuleKey {
            kind: connection.rule.clone(),
            payload: connection.rule_payload.clone(),
        },
        chains: connection.chains.clone(),
        inbound: inbound.unwrap_or(UNKNOWN).to_owned(),
        // Labelled by the accounting session when it first sees the connection.
        profile: None,
        // TODO(traffic-geo): a loopback or private source is this machine, so
        // locate it at the address `NyanpasuClient::probe_direct_egress` reports,
        // and leave it unknown on `TunEnabled` or no address. Wiring it waits
        // on when to probe again: the app has no network-change signal yet.
        source_region: known.map_or_else(|| UNKNOWN.to_owned(), |m| locate_source(m, index)),
        destination_region,
        destination_basis,
    }
}

fn text(value: Option<&String>) -> Option<&str> {
    value.map(String::as_str).filter(|value| !value.is_empty())
}

fn network(network: &ConfigEnum<ConnectionNetwork>) -> &str {
    match network {
        ConfigEnum::Known(ConnectionNetwork::Tcp) => "tcp",
        ConfigEnum::Known(ConnectionNetwork::Udp) => "udp",
        ConfigEnum::Known(ConnectionNetwork::All) => "all",
        ConfigEnum::Known(ConnectionNetwork::Invalid) => "invalid",
        ConfigEnum::Unknown(value) => value,
    }
}
