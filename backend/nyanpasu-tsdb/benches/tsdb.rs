//! Deterministic crate-only measurements; stdout is validated CSV, stderr is progress.

use nyanpasu_tsdb as tsdb;
#[cfg(feature = "redb")]
use std::fs;
use std::path::PathBuf;
use support::{
    fixture::{Fixture, SEED, Scale, Shape},
    output::{self, HEADER},
    run::{self, Engine},
};
use tsdb::storage::{Durability, memory::MemoryDisk};
mod support;

/// CLI values are explicit and the default workload stays at scale S.
struct Args {
    /// Input cardinality; M and L are explicit opt-ins.
    scale: Scale,
    /// Local deterministic generator seed.
    seed: u64,
    /// Optional selection of one named synthetic shape.
    shape: Option<String>,
    /// Read repetitions per operation; opt in to sufficient samples for tail measurements.
    samples: Option<usize>,
    /// CSV destination; stdout when omitted.
    output: Option<PathBuf>,
    /// Explicit sanitized compatible redb source, copied before opening.
    copy: Option<PathBuf>,
    /// Physical redb space experiment, with its own CSV schema.
    space: bool,
}
impl Args {
    /// Cargo also supplies --bench; every other unknown option is an error.
    fn parse() -> Self {
        let mut result = Self {
            scale: Scale::S,
            seed: SEED,
            shape: None,
            samples: None,
            output: None,
            copy: None,
            space: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--bench" => (),
                "--space" => result.space = true,
                "--smoke" => result.scale = Scale::Smoke,
                "--scale" => {
                    result.scale = match args.next().as_deref() {
                        Some("S") => Scale::S,
                        Some("M") => Scale::M,
                        Some("L") => Scale::L,
                        _ => panic!("--scale S|M|L"),
                    }
                }
                "--seed" => {
                    result.seed = args
                        .next()
                        .expect("seed")
                        .parse()
                        .expect("decimal u64 seed")
                }
                "--shape" => result.shape = Some(args.next().expect("shape")),
                "--samples" => {
                    result.samples = Some(
                        args.next()
                            .expect("sample count")
                            .parse()
                            .expect("positive usize"),
                    )
                }
                "--output" => result.output = Some(args.next().expect("output").into()),
                "--path" => result.copy = Some(args.next().expect("sanitized copy").into()),
                _ => panic!("unknown argument {arg}"),
            }
        }
        assert!(
            result.samples.is_none_or(|n| n > 0),
            "--samples must be positive"
        );
        result
    }
}
/// Generation, engine warmup, timed work and verification are separate phases.
fn main() {
    let elapsed = std::time::Instant::now();
    let args = Args::parse();
    // Fail before minutes of work; `write_new` still refuses the file if it appears meanwhile.
    if let Some(path) = &args.output {
        assert!(!path.exists(), "--output {} already exists", path.display());
    }
    output::self_check();
    output::self_check_write_new();
    if args.space {
        assert!(args.copy.is_none() && args.shape.is_none() && args.samples.is_none());
        assert!(
            matches!(args.scale, Scale::S | Scale::Smoke),
            "--space has a fixed matrix"
        );
        #[cfg(not(feature = "redb"))]
        panic!("--space requires --features redb");
        #[cfg(feature = "redb")]
        {
            let csv = support::space::run(args.seed, matches!(args.scale, Scale::Smoke));
            if let Some(path) = args.output {
                output::write_new(&path, &csv).unwrap();
            } else {
                print!("{csv}");
            }
            return;
        }
    }
    #[cfg(feature = "redb")]
    if matches!(args.scale, Scale::Smoke) {
        run::check_copy(args.seed);
        // A small real-file experiment validates the separate space CSV without mixing headers.
        std::hint::black_box(support::space::run(args.seed, true));
    }
    if let Some(name) = &args.shape {
        assert!(
            Shape::all(args.scale)
                .iter()
                .any(|shape| shape.name == name),
            "unknown shape {name}"
        );
    }
    if let Some(path) = &args.copy {
        #[cfg(not(feature = "redb"))]
        panic!("--path requires --features redb: {}", path.display());
        #[cfg(feature = "redb")]
        {
            let shape = Shape::all(args.scale)
                .into_iter()
                .find(|s| args.shape.as_deref() == Some(s.name))
                .expect("--path requires an explicit compatible --shape");
            let csv = format!(
                "{HEADER}{}",
                run::sanitized(
                    path,
                    args.scale,
                    shape,
                    args.seed,
                    args.samples.unwrap_or(16)
                )
            );
            output::validate(&csv);
            if let Some(path) = args.output {
                output::write_new(&path, &csv).unwrap();
            } else {
                print!("{csv}");
            }
            return;
        }
    }
    #[cfg(feature = "redb")]
    support::disk::Counting::self_check();
    let mut csv = HEADER.to_owned();
    for shape in Shape::all(args.scale)
        .into_iter()
        .filter(|s| args.shape.as_ref().is_none_or(|name| name == s.name))
    {
        eprintln!(
            "generate synthetic approximation {} {:?} seed={}",
            shape.name, args.scale, args.seed
        );
        let fixture = Fixture::generate(shape, args.seed);
        for durability in [Durability::Consistent, Durability::Durable] {
            let warm = MemoryDisk::new();
            run::warmup(|| warm.open(), args.seed);
            let disk = MemoryDisk::new();
            csv += &run::day(
                &fixture,
                args.scale,
                args.seed,
                durability,
                args.samples
                    .unwrap_or(if matches!(args.scale, Scale::Smoke) {
                        4
                    } else {
                        16
                    }),
                Engine {
                    name: "memory",
                    reopen: || disk.open(),
                    observe: || (0, 0, 0, 0),
                },
            );
            #[cfg(feature = "redb")]
            {
                use std::sync::Arc;
                use support::disk::{Counting, Counts};
                use tsdb::storage::redb::RedbStorage;
                let temp = support::tempdir();
                let warm_path = temp.path().join("warm.redb");
                let warm_counts = Arc::new(Counts::default());
                run::warmup(
                    || {
                        RedbStorage::open_with_backend(
                            Counting::open(&warm_path, warm_counts.clone()),
                            shape.profile().backend_cache,
                        )
                    },
                    args.seed,
                );
                let path = temp.path().join("day.redb");
                let counts = Arc::new(Counts::default());
                csv += &run::day(
                    &fixture,
                    args.scale,
                    args.seed,
                    durability,
                    args.samples
                        .unwrap_or(if matches!(args.scale, Scale::Smoke) {
                            4
                        } else {
                            16
                        }),
                    Engine {
                        name: "redb",
                        reopen: || {
                            RedbStorage::open_with_backend(
                                Counting::open(&path, counts.clone()),
                                shape.profile().backend_cache,
                            )
                        },
                        observe: || {
                            let (bytes, syncs) = counts.read();
                            (
                                fs::metadata(&path).unwrap().len(),
                                bytes,
                                syncs,
                                counts.closes(),
                            )
                        },
                    },
                );
            }
        }
    }
    for events in [false, true] {
        let disk = MemoryDisk::new();
        csv += &run::maintenance(
            events,
            args.seed,
            Engine {
                name: "memory",
                reopen: || disk.open(),
                observe: || (0, 0, 0, 0),
            },
        );
        #[cfg(feature = "redb")]
        {
            use std::sync::Arc;
            use support::disk::{Counting, Counts};
            let temp = support::tempdir();
            let path = temp.path().join("maintenance.redb");
            let counts = Arc::new(Counts::default());
            csv += &run::maintenance(
                events,
                args.seed,
                Engine {
                    name: "redb",
                    reopen: || {
                        tsdb::storage::redb::RedbStorage::open_with_backend(
                            Counting::open(&path, counts.clone()),
                            tsdb::Profile::desktop().backend_cache,
                        )
                    },
                    observe: || {
                        let (bytes, syncs) = counts.read();
                        (
                            fs::metadata(&path).unwrap().len(),
                            bytes,
                            syncs,
                            counts.closes(),
                        )
                    },
                },
            );
        }
    }
    output::validate(&csv);
    eprintln!(
        "completed validated benchmark in {:.3}s",
        elapsed.elapsed().as_secs_f64()
    );
    if let Some(path) = args.output {
        output::write_new(&path, &csv).unwrap();
    } else {
        print!("{csv}");
    }
}
