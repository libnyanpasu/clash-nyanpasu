use crate::model::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub fn digest<T: serde::Serialize>(value: &T) -> TrafficResult<String> {
    let bytes = serde_json::to_vec(value).map_err(|e| StoreError::InvalidData(e.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
/// Hash the domain changes independently of the resulting position's digest.
pub fn observation_digest(batch: &ObservationCommit) -> TrafficResult<String> {
    let mut canonical = batch.clone();
    canonical.digest.clear();
    canonical.session.position = canonical.expected_previous.clone();
    digest(&canonical)
}

pub fn counters(upload: i64, download: i64) -> TrafficResult<Bytes> {
    Ok(Bytes {
        upload: UInt(
            upload
                .try_into()
                .map_err(|_| StoreError::InvalidData("negative upload counter".into()))?,
        ),
        download: UInt(
            download
                .try_into()
                .map_err(|_| StoreError::InvalidData("negative download counter".into()))?,
        ),
    })
}
pub fn add_quality(quality: &mut Vec<Quality>, flag: Quality) {
    if !quality.contains(&flag) {
        quality.push(flag)
    }
}
fn delta(current: &Bytes, previous: Option<&Bytes>) -> (Bytes, bool) {
    match previous {
        None => (current.clone(), false),
        Some(p) => {
            let reset = current.upload < p.upload || current.download < p.download;
            (
                Bytes {
                    upload: UInt(if current.upload < p.upload {
                        current.upload.0
                    } else {
                        current.upload.0 - p.upload.0
                    }),
                    download: UInt(if current.download < p.download {
                        current.download.0
                    } else {
                        current.download.0 - p.download.0
                    }),
                },
                reset,
            )
        }
    }
}
fn text(metadata: &BTreeMap<String, serde_json::Value>, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
pub fn normalized_rule_kind(kind: &str) -> String {
    kind.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
pub fn dimensions(sample: &ConnectionSample, context: Option<&ConfigContext>) -> Dimensions {
    let process = text(&sample.metadata, "processPath")
        .or_else(|| text(&sample.metadata, "process"))
        .unwrap_or_else(|| "unknown".into())
        .replace('\\', "/");
    let ambiguous = context.is_some_and(|c| {
        c.rules
            .iter()
            .filter(|(r, p)| {
                normalized_rule_kind(r) == normalized_rule_kind(&sample.rule)
                    && p == &sample.rule_payload
            })
            .count()
            > 1
    });
    Dimensions {
        process,
        source: text(&sample.metadata, "sourceIP").unwrap_or_else(|| "unknown".into()),
        target: text(&sample.metadata, "host")
            .or_else(|| text(&sample.metadata, "destinationIP"))
            .unwrap_or_else(|| "unknown".into()),
        protocol: text(&sample.metadata, "network").unwrap_or_else(|| "unknown".into()),
        rule: RuleKey {
            kind: sample.rule.clone(),
            payload: sample.rule_payload.clone(),
            context: context.map(|c| c.revision.clone()),
            ambiguous,
        },
        path: sample.chains.clone(),
        exit: sample
            .chains
            .first()
            .cloned()
            .unwrap_or_else(|| "unknown".into()),
    }
}
#[derive(Clone)]
pub struct AccountingResult {
    pub commit: ObservationCommit,
    pub active: BTreeMap<String, ConnectionRecord>,
    pub rates: BTreeMap<String, Option<Rate>>,
    pub current_rate: Option<Rate>,
    pub interval_valid: bool,
}
/// Previous contains active records plus only those persisted closed IDs present in this frame.
pub fn account(
    session: &SessionRecord,
    previous: &BTreeMap<String, ConnectionRecord>,
    observation: &Observation,
    context: Option<&ConfigContext>,
    continuous: bool,
) -> TrafficResult<AccountingResult> {
    let global = counters(observation.upload_total, observation.download_total)?;
    let mut seen = BTreeSet::new();
    for sample in &observation.connections {
        counters(sample.upload, sample.download)?;
        if !seen.insert(&sample.id) {
            return Err(StoreError::InvalidData("duplicate connection ID".into()));
        }
    }
    let mut next = session.clone();
    let sequence = UInt(
        session
            .position
            .sequence
            .0
            .checked_add(1)
            .ok_or(StoreError::InvalidData("sequence overflow".into()))?,
    );
    let elapsed = session
        .last_monotonic_ns
        .and_then(|t| observation.monotonic_ns.0.checked_sub(t.0))
        .filter(|&n| n > 0);
    let contiguous =
        continuous && session.source_generation == observation.generation && elapsed.is_some();
    let (global_delta, global_reset) = delta(&global, session.global_counters.as_ref());
    if global_reset {
        add_quality(&mut next.quality, Quality::CounterReset)
    }
    let current_rate = if contiguous && !global_reset {
        elapsed.map(|n| Rate {
            upload: global_delta.upload.0 as f64 / (n as f64 / 1e9),
            download: global_delta.download.0 as f64 / (n as f64 / 1e9),
        })
    } else {
        None
    };
    next.core_reported_bytes = next.core_reported_bytes.checked_add(&global_delta)?;
    next.global_counters = Some(global);
    next.last_monotonic_ns = Some(observation.monotonic_ns);
    next.source_generation = observation.generation;
    next.first_sample_at = next.first_sample_at.or(Some(observation.wall_time));
    next.last_sample_at = Some(observation.wall_time);
    next.freshness = Freshness::Fresh;
    let minute = if contiguous
        && session
            .last_sample_at
            .is_some_and(|t| t <= observation.wall_time)
    {
        Some(UInt(observation.wall_time.0 / 60000))
    } else {
        None
    };
    let mut active = BTreeMap::new();
    let mut connections = Vec::new();
    let mut facts = Vec::new();
    let mut rates = BTreeMap::new();
    for sample in &observation.connections {
        let bytes = counters(sample.upload, sample.download)?;
        let old = previous.get(&sample.id);
        let mut dims = dimensions(sample, context);
        if let Some(old) = old
            && old.dimensions.rule.kind == dims.rule.kind
            && old.dimensions.rule.payload == dims.rule.payload
        {
            dims.rule = old.dimensions.rule.clone();
        }
        let (increment, reset) = delta(&bytes, old.map(|r| &r.counters));
        let reappeared = old.is_some_and(|r| !matches!(r.status, ConnectionStatus::Active));
        let mut record = old.cloned().unwrap_or_else(|| ConnectionRecord {
            session_id: session.id.clone(),
            id: sample.id.clone(),
            first_observed_sequence: sequence,
            first_observed_at: observation.wall_time,
            last_observed_at: observation.wall_time,
            sample: sample.clone(),
            counters: Bytes::default(),
            accounted_bytes: Bytes::default(),
            status: ConnectionStatus::Active,
            dimensions: dims.clone(),
            segment: UInt(0),
            quality: Vec::new(),
        });
        if old.is_none() {
            next.observed_connections = UInt(
                next.observed_connections
                    .0
                    .checked_add(1)
                    .ok_or(StoreError::InvalidData("connection count overflow".into()))?,
            )
        }
        if reset {
            add_quality(&mut record.quality, Quality::CounterReset);
            add_quality(&mut next.quality, Quality::CounterReset)
        }
        if reappeared {
            add_quality(&mut record.quality, Quality::Reappeared)
        }
        if dims.rule.ambiguous {
            add_quality(&mut record.quality, Quality::AmbiguousRule);
            add_quality(&mut next.quality, Quality::AmbiguousRule)
        }
        if old.is_some() && (record.dimensions != dims || reset) {
            record.segment = UInt(
                record
                    .segment
                    .0
                    .checked_add(1)
                    .ok_or(StoreError::InvalidData("segment overflow".into()))?,
            )
        }
        let rate = if contiguous && !reset && !reappeared && old.is_some() {
            elapsed.map(|n| Rate {
                upload: increment.upload.0 as f64 / (n as f64 / 1e9),
                download: increment.download.0 as f64 / (n as f64 / 1e9),
            })
        } else {
            None
        };
        rates.insert(sample.id.clone(), rate);
        let allocated = if old.is_some() && !reset && !reappeared {
            minute
        } else {
            None
        };
        if increment != Bytes::default()
            || old.is_none()
            || old.is_some_and(|r| r.dimensions != dims)
        {
            facts.push(AttributionFact {
                session_id: session.id.clone(),
                connection_id: sample.id.clone(),
                sequence,
                segment: record.segment,
                dimensions: dims.clone(),
                bytes: increment.clone(),
                minute: allocated,
                interval_from: session.last_sample_at,
                interval_until: observation.wall_time,
            });
            if allocated.is_none() {
                next.time_unallocated = next.time_unallocated.checked_add(&increment)?;
                add_quality(&mut next.quality, Quality::TimeUnallocated)
            }
        }
        next.attributed_bytes = next.attributed_bytes.checked_add(&increment)?;
        record.accounted_bytes = record.accounted_bytes.checked_add(&increment)?;
        record.counters = bytes;
        record.sample = sample.clone();
        record.dimensions = dims;
        record.last_observed_at = observation.wall_time;
        record.status = ConnectionStatus::Active;
        active.insert(record.id.clone(), record.clone());
        connections.push(record);
    }
    for (id, old) in previous {
        if matches!(old.status, ConnectionStatus::Active) && !active.contains_key(id) {
            let mut closed = old.clone();
            closed.status = ConnectionStatus::Closed {
                detected_at: observation.wall_time,
                reason: CloseReason::MissingFromSnapshot,
                final_counters_exact: false,
            };
            connections.push(closed)
        }
    }
    let mut commit = ObservationCommit {
        session: next,
        expected_previous: session.position.clone(),
        sequence,
        digest: String::new(),
        connections,
        facts,
    };
    commit.digest = observation_digest(&commit)?;
    commit.session.position = CommittedPosition {
        sequence,
        digest: commit.digest.clone(),
    };
    Ok(AccountingResult {
        commit,
        active,
        rates,
        current_rate,
        interval_valid: contiguous,
    })
}
pub fn matches_dimensions(d: &Dimensions, f: &ConnectionFilter) -> bool {
    f.rule.as_ref().is_none_or(|x| {
        x.kind == d.rule.kind
            && x.payload == d.rule.payload
            && x.context
                .as_ref()
                .is_none_or(|c| Some(c) == d.rule.context.as_ref())
    }) && f.process.as_ref().is_none_or(|x| x == &d.process)
        && f.source.as_ref().is_none_or(|x| x == &d.source)
        && f.target.as_ref().is_none_or(|x| x == &d.target)
        && f.exit.as_ref().is_none_or(|x| x == &d.exit)
        && f.path.as_ref().is_none_or(|x| x == &d.path)
        && f.protocol.as_ref().is_none_or(|x| x == &d.protocol)
        && f.group_chain
            .as_ref()
            .is_none_or(|x| *x == d.path.iter().skip(1).rev().cloned().collect::<Vec<_>>())
}
pub fn group_key(d: &Dimensions, group: &GroupBy) -> String {
    match group {
        GroupBy::Process => d.process.clone(),
        GroupBy::Source => d.source.clone(),
        GroupBy::Target => d.target.clone(),
        GroupBy::Protocol => d.protocol.clone(),
        GroupBy::Exit => d.exit.clone(),
        GroupBy::Rule => serde_json::to_string(&d.rule).expect("rule serializes"),
        GroupBy::Path => serde_json::to_string(&d.path).expect("path serializes"),
    }
}

/// Only simple rules have a payload we can identify without interpreting nested syntax.
/// Omitted composites remain unresolved evidence, never inferred rule indices.
pub fn config_context_rules(rules: &[String]) -> Vec<(String, String)> {
    rules
        .iter()
        .filter_map(|rule| {
            let parts: Vec<_> = rule.split(',').map(str::trim).collect();
            let kind = *parts.first()?;
            let normalized = normalized_rule_kind(kind);
            if normalized == "match" && parts.len() == 2 {
                return Some((kind.to_owned(), String::new()));
            }
            if parts.len() < 3 || parts.iter().any(|p| p.contains(['(', ')'])) {
                return None;
            }
            if [
                "domain",
                "domainsuffix",
                "domainkeyword",
                "domainregex",
                "geosite",
                "geoip",
                "ipcidr",
                "ipcidr6",
                "srcipcidr",
                "srcipcidr6",
                "srcport",
                "dstport",
                "port",
                "processname",
                "processpath",
                "processnameregex",
                "processpathregex",
                "ruleset",
                "network",
                "inname",
                "intype",
                "inuser",
                "uid",
                "dscp",
            ]
            .contains(&normalized.as_str())
            {
                Some((kind.to_owned(), parts[1].to_owned()))
            } else {
                None
            }
        })
        .collect()
}
