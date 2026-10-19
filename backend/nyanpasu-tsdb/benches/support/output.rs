//! CSV serialization and percentile rules, checked before writing any result.

use super::fixture::{Scale, Shape};
use std::{collections::BTreeMap, time::Instant};

/// One complete operation sample; vector allocation happens after the timer stops.
#[derive(Debug)]
pub struct Sample {
    /// Stable public operation or returned maintenance kind.
    pub operation: &'static str,
    /// Elapsed nanoseconds of the API call only.
    pub ns: u128,
    /// Input rows, requested scan rows, or one operation.
    pub units: u64,
}
/// Measurements live in the harness, never in a store account.
#[derive(Default)]
pub struct Samples(pub Vec<Sample>);
impl Samples {
    /// Times only the supplied API call; caller prepares its inputs first.
    pub fn time<T>(&mut self, operation: &'static str, units: u64, run: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let result = std::hint::black_box(run());
        let ns = start.elapsed().as_nanos();
        self.0.push(Sample {
            operation,
            ns,
            units,
        });
        result
    }
    /// Classifies a just-completed one-round call by its public result.
    pub fn name_last(&mut self, name: &'static str) {
        self.0.last_mut().unwrap().operation = name;
    }
    /// Summary rows, with p99 withheld below 1000 samples and n always present.
    pub fn csv(&self, context: &Context<'_>) -> String {
        let mut groups: BTreeMap<&str, Vec<&Sample>> = BTreeMap::new();
        for sample in &self.0 {
            groups.entry(sample.operation).or_default().push(sample);
        }
        let mut out = String::new();
        for (name, samples) in groups {
            let mut times: Vec<_> = samples.iter().map(|s| s.ns).collect();
            times.sort_unstable();
            let n = times.len();
            let total: u128 = times.iter().sum();
            let units: u64 = samples.iter().map(|s| s.units).sum();
            let p99 = if n >= 1000 {
                times[(99 * n).div_ceil(100) - 1].to_string()
            } else {
                String::new()
            };
            let raw = samples
                .iter()
                .map(|sample| sample.ns.to_string())
                .collect::<Vec<_>>()
                .join(";");
            out += &format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{:.3},{},{},{},{},{},{},{},{},{},{}\n",
                context.source,
                context.seed,
                context.scale.name(),
                context.shape.name,
                context.engine,
                context.durability,
                name,
                n,
                times[n.div_ceil(2) - 1],
                p99,
                total,
                units,
                if total == 0 {
                    0.0
                } else {
                    units as f64 * 1e9 / total as f64
                },
                context.logical_bytes,
                context.file_bytes,
                context.written_bytes,
                context.syncs,
                context.journal_bytes,
                context.runs,
                context.crowding,
                context.catalog_bytes,
                context.replay_bytes,
                raw
            );
        }
        out
    }
}
/// Dataset summary after maintenance and actual close; physical counters cover the whole day.
pub struct Context<'a> {
    /// Synthetic approximation or explicitly supplied private copy.
    pub source: &'static str,
    /// Explicit generator seed.
    pub seed: u64,
    /// Declared input scale.
    pub scale: Scale,
    /// Synthetic workload parameters.
    pub shape: Shape,
    /// Memory or redb.
    pub engine: &'a str,
    /// Requested write mode, independent of automatic promotion.
    pub durability: &'a str,
    /// Final visible bytes summed from public status streams.
    pub logical_bytes: u64,
    /// Final redb file length; zero means inapplicable on memory.
    pub file_bytes: u64,
    /// Successful redb write bytes over the complete simulated day.
    pub written_bytes: u64,
    /// Successful redb sync calls over the complete simulated day.
    pub syncs: u64,
    /// Final public journal size, including stored keys.
    pub journal_bytes: u64,
    /// Final sorted runs summed from public status.
    pub runs: u64,
    /// Final crowding marks summed from public status.
    pub crowding: u64,
    /// Final catalog bytes from public status.
    pub catalog_bytes: u64,
    /// Public journal bytes just before the timed replay open, not physical read bytes.
    pub replay_bytes: u64,
}
/// Every row covers one day, including open, maintenance, checkpoint and close I/O.
pub const HEADER: &str = "source,seed,scale,shape,engine,durability,operation,n,p50_ns,p99_ns,\
     total_ns,units,units_per_second,logical_bytes,file_bytes,day_write_bytes,day_syncs,\
     journal_bytes,runs,crowding_marks,catalog_bytes,replay_bytes,samples_ns\n";
/// Smoke validates both the column schema and the small-n percentile rule.
pub fn validate(csv: &str) {
    let columns = HEADER.trim().split(',').count();
    let mut rows = 0;
    for line in csv.lines().skip(1) {
        let cells: Vec<_> = line.split(',').collect();
        assert_eq!(cells.len(), columns, "{line}");
        assert!(matches!(
            cells[0],
            "synthetic-approximation" | "owner-supplied-copy"
        ));
        let n = cells[7].parse::<usize>().unwrap();
        assert!(n > 0);
        assert_eq!(cells[9].is_empty(), n < 1000);
        for i in [1, 8, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21] {
            cells[i].parse::<u128>().unwrap();
        }
        assert!(cells[12].parse::<f64>().unwrap().is_finite());
        let mut raw: Vec<u128> = cells[22].split(';').map(|ns| ns.parse().unwrap()).collect();
        assert_eq!(raw.len(), n);
        assert_eq!(raw.iter().sum::<u128>(), cells[10].parse::<u128>().unwrap());
        raw.sort_unstable();
        assert_eq!(raw[n.div_ceil(2) - 1], cells[8].parse::<u128>().unwrap());
        if n >= 1000 {
            assert_eq!(
                raw[(99 * n).div_ceil(100) - 1],
                cells[9].parse::<u128>().unwrap()
            );
        }
        rows += 1;
    }
    assert!(rows > 0);
}

/// A known distribution verifies percentile arithmetic independently of Instant and the store.
pub fn self_check() {
    let context = Context {
        source: "synthetic-approximation",
        seed: 1,
        scale: Scale::Smoke,
        shape: Shape::tiny(),
        engine: "memory",
        durability: "Durable",
        logical_bytes: 0,
        file_bytes: 0,
        written_bytes: 0,
        syncs: 0,
        journal_bytes: 0,
        runs: 0,
        crowding: 0,
        catalog_bytes: 0,
        replay_bytes: 0,
    };
    let samples = Samples(
        (1..=1000)
            .map(|ns| Sample {
                operation: "self-check",
                ns,
                units: 1,
            })
            .collect(),
    );
    let row = samples.csv(&context);
    let cells: Vec<_> = row.trim().split(',').collect();
    assert_eq!(&cells[7..10], &["1000", "500", "990"]);
    validate(&format!("{HEADER}{row}"));
}

/// Never replace an existing file, including a hard link or symlink to a copy-mode source.
pub fn write_new(path: &std::path::Path, csv: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(csv.as_bytes())
}

/// Refusal preserves existing contents, including another name for the same file.
pub fn self_check_write_new() {
    let temp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let path = temp.path().join("result.csv");
    write_new(&path, "original").unwrap();
    assert_eq!(
        write_new(&path, "replacement").unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    let alias = temp.path().join("alias.csv");
    std::fs::hard_link(&path, &alias).unwrap();
    assert_eq!(
        write_new(&alias, "replacement").unwrap_err().kind(),
        std::io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
}
