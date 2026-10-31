//! Pure derivation of per-connection and per-chain-member transfer rates from
//! two consecutive `clash_api::ConnectionsSnapshot` samples. No IO, no actor
//! state: time and the previous sample's counters are explicit parameters.
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrafficRate {
    pub download: u64,
    pub upload: u64,
}

/// Pushed on every connection sample; size is independent of connection count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionsSummary {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub memory: Option<u64>,
    pub connection_count: u32,
    /// Keyed by chain member name (group or node); summed over every
    /// connection whose `chains` contains that name.
    pub member_rates: IndexMap<String, TrafficRate>,
}

/// One connection plus its derived rates, for IPC consumers that need the
/// per-connection detail (only built when `with_details` is set).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnection<'a> {
    #[serde(flatten)]
    pub connection: &'a clash_api::Connection,
    pub download_speed: u64,
    pub upload_speed: u64,
}

/// Shared source data and rates in source order. Serialization borrows each
/// connection, so slow consumers retain an immutable snapshot without cloning it.
#[derive(Debug)]
pub struct ConnectionDetails {
    snapshot: Arc<clash_api::ConnectionsSnapshot>,
    rates: Vec<TrafficRate>,
}

impl ConnectionDetails {
    pub fn new(snapshot: Arc<clash_api::ConnectionsSnapshot>, rates: Vec<TrafficRate>) -> Self {
        debug_assert_eq!(
            snapshot.connections.as_ref().map_or(0, Vec::len),
            rates.len()
        );
        Self { snapshot, rates }
    }
}

impl Serialize for ConnectionDetails {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.snapshot
                .connections
                .iter()
                .flatten()
                .zip(&self.rates)
                .map(|(connection, rate)| ClashConnection {
                    connection,
                    download_speed: rate.download,
                    upload_speed: rate.upload,
                }),
        )
    }
}

/// Spare counter storage, owned by the stream actor and recycled after a commit.
#[derive(Default)]
pub struct ConnectionRateScratch {
    per_connection: HashMap<Uuid, (i64, i64)>,
}

impl ConnectionRateScratch {
    pub fn recycle(&mut self, counters: ConnectionCounters) {
        self.per_connection = counters.per_connection;
    }
}

/// The counters `ConnectionRates` needs from the previous sample to derive
/// rates for the next one. Deliberately not a full connection list.
#[derive(Debug, Clone)]
pub struct ConnectionCounters {
    pub at: Instant,
    pub download_total: u64,
    pub upload_total: u64,
    pub per_connection: HashMap<Uuid, (i64, i64)>,
}

pub struct DerivedConnections {
    pub summary: ClashConnectionsSummary,
    pub details: Option<Vec<TrafficRate>>,
    pub counters: ConnectionCounters,
}

pub struct ConnectionRates;

impl ConnectionRates {
    /// Derives a summary (and, if `with_details`, per-connection detail) from
    /// `current` and the counters kept from the previous sample. Returns
    /// `None` when the core reports negative totals, matching the actor's
    /// existing behavior of dropping such samples.
    pub fn derive(
        scratch: &mut ConnectionRateScratch,
        previous: Option<&ConnectionCounters>,
        current: &clash_api::ConnectionsSnapshot,
        at: Instant,
        with_details: bool,
    ) -> Option<DerivedConnections> {
        let download_total = u64::try_from(current.download_total).ok()?;
        let upload_total = u64::try_from(current.upload_total).ok()?;

        let seconds = previous.map(|prev| {
            at.checked_duration_since(prev.at)
                .unwrap_or_default()
                .as_secs_f64()
        });
        let (download_speed, upload_speed) = match (previous, seconds) {
            (Some(prev), Some(seconds)) if seconds > 0.0 => (
                (download_total.saturating_sub(prev.download_total) as f64 / seconds) as u64,
                (upload_total.saturating_sub(prev.upload_total) as f64 / seconds) as u64,
            ),
            _ => (0, 0),
        };

        let connections = current.connections.as_deref().unwrap_or(&[]);
        let mut member_rates: IndexMap<String, TrafficRate> = IndexMap::new();
        let mut per_connection = std::mem::take(&mut scratch.per_connection);
        per_connection.clear();
        per_connection.reserve(connections.len());
        let mut seen = HashSet::new();
        let mut details = with_details.then(|| Vec::with_capacity(connections.len()));

        for connection in connections {
            let prev_counters = previous.and_then(|prev| prev.per_connection.get(&connection.id));
            let (conn_download_speed, conn_upload_speed) = match (prev_counters, seconds) {
                (Some(&(prev_download, prev_upload)), Some(seconds)) if seconds > 0.0 => (
                    rate(prev_download, connection.download, seconds),
                    rate(prev_upload, connection.upload, seconds),
                ),
                _ => (0, 0),
            };
            per_connection.insert(connection.id, (connection.download, connection.upload));

            seen.clear();
            for member in &connection.chains {
                if seen.insert(member.as_str()) {
                    if let Some(entry) = member_rates.get_mut(member.as_str()) {
                        entry.download += conn_download_speed;
                        entry.upload += conn_upload_speed;
                    } else {
                        member_rates.insert(
                            member.clone(),
                            TrafficRate {
                                download: conn_download_speed,
                                upload: conn_upload_speed,
                            },
                        );
                    }
                }
            }

            if let Some(details) = details.as_mut() {
                details.push(TrafficRate {
                    download: conn_download_speed,
                    upload: conn_upload_speed,
                });
            }
        }

        let connection_count = u32::try_from(connections.len()).unwrap_or(u32::MAX);

        Some(DerivedConnections {
            summary: ClashConnectionsSummary {
                download_total,
                upload_total,
                download_speed,
                upload_speed,
                memory: current.memory,
                connection_count,
                member_rates,
            },
            details,
            counters: ConnectionCounters {
                at,
                download_total,
                upload_total,
                per_connection,
            },
        })
    }
}

/// Bytes/second between two counter readings, clamped to 0 when the core's
/// counter went backwards (e.g. it reset).
fn rate(previous: i64, current: i64, seconds: f64) -> u64 {
    let delta = current.saturating_sub(previous);
    if delta <= 0 {
        return 0;
    }
    (delta as u64 as f64 / seconds) as u64
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    fn derive(
        previous: Option<&ConnectionCounters>,
        current: &clash_api::ConnectionsSnapshot,
        at: Instant,
        details: bool,
    ) -> Option<DerivedConnections> {
        ConnectionRates::derive(
            &mut ConnectionRateScratch::default(),
            previous,
            current,
            at,
            details,
        )
    }

    fn conn(id: &str, download: i64, upload: i64, chains: &[&str]) -> clash_api::Connection {
        serde_json::from_value(json!({
            "id": id,
            "metadata": null,
            "upload": upload,
            "download": download,
            "start": "2024-01-01T00:00:00Z",
            "chains": chains,
            "rule": "MATCH",
            "rulePayload": "",
        }))
        .unwrap()
    }

    fn snapshot(
        download_total: i64,
        upload_total: i64,
        connections: Vec<clash_api::Connection>,
    ) -> clash_api::ConnectionsSnapshot {
        clash_api::ConnectionsSnapshot {
            download_total,
            upload_total,
            connections: if connections.is_empty() {
                None
            } else {
                Some(connections)
            },
            memory: Some(1024),
        }
    }

    const A: &str = "11111111-1111-1111-1111-111111111111";
    const B: &str = "22222222-2222-2222-2222-222222222222";

    #[test]
    fn first_sample_has_zero_rates() {
        let current = snapshot(1000, 2000, vec![conn(A, 100, 200, &["Proxy"])]);
        let derived = derive(None, &current, Instant::now(), false).unwrap();
        assert_eq!(derived.summary.download_speed, 0);
        assert_eq!(derived.summary.upload_speed, 0);
        assert_eq!(derived.summary.download_total, 1000);
        assert_eq!(derived.summary.upload_total, 2000);
        assert_eq!(derived.summary.connection_count, 1);
        assert_eq!(
            derived.summary.member_rates["Proxy"],
            TrafficRate {
                download: 0,
                upload: 0
            }
        );
        assert!(derived.details.is_none());
    }

    #[test]
    fn rates_use_elapsed_time_between_samples() {
        let first = snapshot(0, 0, vec![conn(A, 0, 0, &["Proxy"])]);
        let t0 = Instant::now();
        let derived0 = derive(None, &first, t0, false).unwrap();

        let second = snapshot(1000, 2000, vec![conn(A, 1000, 2000, &["Proxy"])]);
        let t1 = t0 + Duration::from_secs(2);
        let derived1 = derive(Some(&derived0.counters), &second, t1, false).unwrap();

        assert_eq!(derived1.summary.download_speed, 500);
        assert_eq!(derived1.summary.upload_speed, 1000);
        assert_eq!(derived1.summary.member_rates["Proxy"].download, 500);
        assert_eq!(derived1.summary.member_rates["Proxy"].upload, 1000);
    }

    #[test]
    fn zero_elapsed_reports_zero_rates() {
        let first = snapshot(0, 0, vec![conn(A, 0, 0, &[])]);
        let t0 = Instant::now();
        let derived0 = derive(None, &first, t0, false).unwrap();

        let second = snapshot(1000, 1000, vec![conn(A, 1000, 1000, &[])]);
        let derived1 = derive(Some(&derived0.counters), &second, t0, false).unwrap();

        assert_eq!(derived1.summary.download_speed, 0);
        assert_eq!(derived1.summary.upload_speed, 0);
    }

    #[test]
    fn new_connection_has_zero_rate_and_closed_connection_is_dropped() {
        let first = snapshot(0, 0, vec![conn(A, 0, 0, &["Proxy"])]);
        let t0 = Instant::now();
        let derived0 = derive(None, &first, t0, false).unwrap();

        // A closes, B opens.
        let second = snapshot(500, 500, vec![conn(B, 500, 500, &["Proxy"])]);
        let t1 = t0 + Duration::from_secs(1);
        let derived1 = derive(Some(&derived0.counters), &second, t1, true).unwrap();

        assert_eq!(derived1.summary.connection_count, 1);
        let details = derived1.details.unwrap();
        assert_eq!(details.len(), 1);
        assert_eq!(second.connections.as_ref().unwrap()[0].id.to_string(), B);
        assert_eq!(details[0].download, 0);
        assert_eq!(details[0].upload, 0);
    }

    #[test]
    fn counter_going_backward_is_clamped_to_zero() {
        let first = snapshot(1000, 1000, vec![conn(A, 1000, 1000, &[])]);
        let t0 = Instant::now();
        let derived0 = derive(None, &first, t0, false).unwrap();

        // The core reset this connection's download counter.
        let second = snapshot(1000, 2000, vec![conn(A, 200, 2000, &[])]);
        let t1 = t0 + Duration::from_secs(1);
        let derived1 = derive(Some(&derived0.counters), &second, t1, true).unwrap();

        let details = derived1.details.unwrap();
        assert_eq!(details[0].download, 0);
        assert_eq!(details[0].upload, 1000);
    }

    #[test]
    fn duplicate_chain_names_in_one_connection_count_once() {
        let first = snapshot(0, 0, vec![conn(A, 0, 0, &["Proxy", "Proxy"])]);
        let t0 = Instant::now();
        let derived0 = derive(None, &first, t0, false).unwrap();

        let second = snapshot(1000, 0, vec![conn(A, 1000, 0, &["Proxy", "Proxy"])]);
        let t1 = t0 + Duration::from_secs(1);
        let derived1 = derive(Some(&derived0.counters), &second, t1, false).unwrap();

        assert_eq!(derived1.summary.member_rates["Proxy"].download, 1000);
    }

    #[test]
    fn member_rates_sum_across_connections() {
        let first = snapshot(
            0,
            0,
            vec![
                conn(A, 0, 0, &["Proxy"]),
                conn(B, 0, 0, &["Proxy", "Direct"]),
            ],
        );
        let t0 = Instant::now();
        let derived0 = derive(None, &first, t0, false).unwrap();

        let second = snapshot(
            2000,
            0,
            vec![
                conn(A, 1000, 0, &["Proxy"]),
                conn(B, 1000, 0, &["Proxy", "Direct"]),
            ],
        );
        let t1 = t0 + Duration::from_secs(1);
        let derived1 = derive(Some(&derived0.counters), &second, t1, false).unwrap();

        assert_eq!(derived1.summary.member_rates["Proxy"].download, 2000);
        assert_eq!(derived1.summary.member_rates["Direct"].download, 1000);
    }

    #[test]
    fn shared_details_preserve_order_rates_and_extensions() {
        let at = Instant::now();
        let previous = snapshot(0, 0, vec![conn(A, 0, 0, &[]), conn(B, 0, 0, &[])]);
        let counters = derive(None, &previous, at, false).unwrap().counters;
        let mut current = snapshot(40, 60, vec![conn(B, 30, 40, &[]), conn(A, 10, 20, &[])]);
        let connection = &mut current.connections.as_mut().unwrap()[0];
        connection
            .extra
            .insert("future".into(), json!({"nested":[1,null,"text"]}));
        connection.metadata =
            Some(serde_json::from_value(json!({"sourcePort":"443","future":false})).unwrap());
        let current = Arc::new(current);
        let derived = derive(Some(&counters), &current, at + Duration::from_secs(1), true).unwrap();
        let rates = derived.details.unwrap();
        let mut expected = serde_json::to_value(current.connections.as_ref().unwrap()).unwrap();
        expected[0]["downloadSpeed"] = json!(30);
        expected[0]["uploadSpeed"] = json!(40);
        expected[1]["downloadSpeed"] = json!(10);
        expected[1]["uploadSpeed"] = json!(20);
        let details = ConnectionDetails::new(Arc::clone(&current), rates);
        let weak = Arc::downgrade(&current);
        drop(current);
        assert_eq!(serde_json::to_value(&details).unwrap(), expected);
        drop(details);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn recycled_counters_keep_previous_baselines_and_duplicate_id_behavior() {
        let mut scratch = ConnectionRateScratch::default();
        let mut previous = None;
        let at = Instant::now();
        for i in 0..5 {
            let current = snapshot(
                100 * i,
                0,
                vec![conn(A, 100 * i, 0, &[]), conn(A, 100 * i + 10, 0, &[])],
            );
            let derived = ConnectionRates::derive(
                &mut scratch,
                previous.as_ref(),
                &current,
                at + Duration::from_secs(i as u64),
                true,
            )
            .unwrap();
            let rates = derived.details.unwrap();
            assert_eq!(rates[0].download, if i == 0 { 0 } else { 90 });
            assert_eq!(rates[1].download, if i == 0 { 0 } else { 100 });
            if let Some(old) = previous.replace(derived.counters) {
                scratch.recycle(old);
            }
        }
        let counters = previous.unwrap();
        assert!(
            ConnectionRates::derive(
                &mut scratch,
                Some(&counters),
                &snapshot(-1, 0, vec![]),
                at,
                false
            )
            .is_none()
        );
        let empty = ConnectionRates::derive(
            &mut scratch,
            Some(&counters),
            &snapshot(500, 0, vec![]),
            at + Duration::from_secs(5),
            false,
        )
        .unwrap();
        assert_eq!(empty.summary.connection_count, 0);
        assert!(empty.counters.per_connection.is_empty());
    }

    #[test]
    fn no_connections_reports_zero_count() {
        let current = clash_api::ConnectionsSnapshot {
            download_total: 0,
            upload_total: 0,
            connections: None,
            memory: None,
        };
        let derived = derive(None, &current, Instant::now(), true).unwrap();
        assert_eq!(derived.summary.connection_count, 0);
        assert!(derived.details.unwrap().is_empty());
    }

    #[test]
    fn negative_totals_return_none() {
        let current = snapshot(-1, 0, vec![]);
        assert!(derive(None, &current, Instant::now(), false).is_none());
    }
}
