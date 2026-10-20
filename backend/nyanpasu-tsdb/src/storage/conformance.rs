//! The conformance suite every backend passes (§3.5). It is written once against the port
//! and run for each backend: a backend's tests implement [`Harness`] and expand
//! [`conformance_tests!`], which makes one test of each check.

use std::{
    cell::Cell,
    ops::{Bound, ControlFlow},
    sync::mpsc,
    thread,
};

use super::{
    ChunkId, CommitFailure, Direction, Durability, FragmentId, KeyRange, MAX_FRAGMENT_LEN, Ns,
    Result, Storage, StorageError, StorageRead, StorageWrite,
};
use crate::ErrorKind;

/// The classes of operations a fault can be injected into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpClass {
    /// `get`, `scan` and `fragment`, on a snapshot or a batch.
    Read,
    /// `put`, `delete`, `put_fragment` and `delete_chunk`.
    Write,
    /// `commit`. An I/O failure inside a commit leaves its outcome `Unknown`.
    Commit,
}

/// How an injected commit ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommitOutcome {
    /// [`CommitFailure::NotApplied`], like redb's `TransactionPoisoned`: the batch rolls
    /// back and the instance stays usable.
    NotApplied,
    /// [`CommitFailure::Unknown`], with the batch applied at its durability or left out.
    /// The instance fails, as redb refuses further writes after such a commit error.
    Unknown { applied: bool },
}

/// A backend under test. Every capability is required: the suite skips nothing, so a
/// backend that cannot provide one does not pass.
pub(crate) trait Harness {
    type Storage: Storage;

    /// A harness around a fresh, empty storage that shares nothing with other harnesses.
    fn fresh() -> Self;

    fn storage(&self) -> &Self::Storage;

    /// Closes the storage cleanly and opens the same data again.
    fn reopen(&mut self);

    /// Closes the storage the way a crash would, without anything a clean close writes,
    /// and opens the same data again.
    fn crash_and_reopen(&mut self);

    /// Fails the next operation of `class` with an I/O failure, which fails the instance.
    fn fail_next(&self, class: OpClass);

    /// Makes the next commit end with `outcome`.
    fn inject_commit(&self, outcome: CommitOutcome);

    /// Damages what is stored under `key`, so that reading it reports
    /// [`StorageError::Corrupted`], as a failed backend checksum would.
    fn corrupt_entry(&self, ns: Ns, key: &[u8]);

    /// Damages a stored fragment, like [`Harness::corrupt_entry`].
    fn corrupt_fragment(&self, id: FragmentId);
}

/// Expands to one test of each check, run against `$harness`.
macro_rules! conformance_tests {
    ($harness:ty) => {
        #[test]
        fn conformance_atomicity() {
            $crate::storage::conformance::atomicity::<$harness>();
        }

        #[test]
        fn conformance_read_your_writes() {
            $crate::storage::conformance::read_your_writes::<$harness>();
        }

        #[test]
        fn conformance_stable_snapshots() {
            $crate::storage::conformance::stable_snapshots::<$harness>();
        }

        #[test]
        fn conformance_ordering_and_ranges() {
            $crate::storage::conformance::ordering_and_ranges::<$harness>();
        }

        #[test]
        fn conformance_early_termination() {
            $crate::storage::conformance::early_termination::<$harness>();
        }

        #[test]
        fn conformance_crash_consistency() {
            $crate::storage::conformance::crash_consistency::<$harness>();
        }

        #[test]
        fn conformance_commit_outcomes() {
            $crate::storage::conformance::commit_outcomes::<$harness>();
        }

        #[test]
        fn conformance_latched_failure() {
            $crate::storage::conformance::latched_failure::<$harness>();
        }

        #[test]
        fn conformance_fragments() {
            $crate::storage::conformance::fragments::<$harness>();
        }

        #[test]
        fn conformance_fragment_limits() {
            $crate::storage::conformance::fragment_limits::<$harness>();
        }

        #[test]
        fn conformance_durability_levels() {
            $crate::storage::conformance::durability_levels::<$harness>();
        }

        #[test]
        fn conformance_failure_handling() {
            $crate::storage::conformance::failure_handling::<$harness>();
        }

        #[test]
        fn conformance_closed_under_reader() {
            $crate::storage::conformance::closed_under_reader::<$harness>();
        }
    };
}

pub(crate) use conformance_tests;

const A: Ns = Ns(1);
const B: Ns = Ns(2);
/// Never written.
const UNUSED: Ns = Ns(200);

type Entry = (Vec<u8>, Vec<u8>);

fn entry(key: &[u8], value: &[u8]) -> Entry {
    (key.to_vec(), value.to_vec())
}

fn frag(chunk: u64, index: u32) -> FragmentId {
    FragmentId {
        chunk: ChunkId(chunk),
        index,
    }
}

/// Bytes of this fragment's own, so that reading the wrong fragment shows.
fn payload(id: FragmentId) -> Vec<u8> {
    [id.chunk.0.to_be_bytes().as_slice(), &id.index.to_be_bytes()].concat()
}

fn get(read: &impl StorageRead, ns: Ns, key: &[u8]) -> Option<Vec<u8>> {
    read.get(ns, key, |value| value.map(<[u8]>::to_vec))
        .expect("get")
}

fn scan(read: &impl StorageRead, ns: Ns, range: KeyRange<'_>, direction: Direction) -> Vec<Entry> {
    let mut entries = Vec::new();
    let broke = read
        .scan(ns, range, direction, |key, value| {
            entries.push(entry(key, value));
            ControlFlow::<()>::Continue(())
        })
        .expect("scan");
    assert_eq!(broke, None, "a scan that never breaks returns None");
    entries
}

fn scan_all(read: &impl StorageRead, ns: Ns) -> Vec<Entry> {
    scan(read, ns, KeyRange::All, Direction::Forward)
}

fn fragment(read: &impl StorageRead, id: FragmentId) -> Option<Vec<u8>> {
    read.fragment(id, MAX_FRAGMENT_LEN, |bytes| bytes.map(<[u8]>::to_vec))
        .expect("read a fragment")
}

fn snapshot<S: Storage>(storage: &S) -> S::Snapshot {
    storage.snapshot().expect("snapshot")
}

fn commit<'s, S: Storage>(
    storage: &'s S,
    durability: Durability,
    write: impl FnOnce(&mut S::Batch<'s>),
) {
    let mut batch = storage.batch().expect("open a batch");
    write(&mut batch);
    batch.commit(durability).expect("commit");
}

fn put_one<S: Storage>(storage: &S, durability: Durability, key: &[u8], value: &[u8]) {
    commit(storage, durability, |batch| {
        batch.put(A, key, value).unwrap()
    });
}

/// The error of an operation that has to fail.
fn failure<T>(result: Result<T>) -> StorageError {
    match result {
        Ok(_) => panic!("the operation succeeded"),
        Err(error) => error,
    }
}

/// The entries of namespaces `A` and `B`, and the probed fragments that exist.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Dump {
    entries: Vec<(Ns, Entry)>,
    fragments: Vec<(FragmentId, Vec<u8>)>,
}

fn dump(read: &impl StorageRead, probes: &[FragmentId]) -> Dump {
    let entries = [A, B]
        .into_iter()
        .flat_map(|ns| scan_all(read, ns).into_iter().map(move |entry| (ns, entry)))
        .collect();
    let fragments = probes
        .iter()
        .filter_map(|&id| Some((id, fragment(read, id)?)))
        .collect();
    Dump { entries, fragments }
}

/// Writes generation `n` everywhere at once: the same key in both namespaces, and
/// fragment 0 of chunk `n`, deleting the chunk of generation `n - 1`.
fn write_generation(batch: &mut impl StorageWrite, n: u64) {
    stage_generation(batch, n).unwrap();
}

/// The same atomicity fixture with propagated errors for exhaustive disk schedules.
pub(crate) fn stage_generation(batch: &mut impl StorageWrite, n: u64) -> Result<(), StorageError> {
    batch.put(A, b"generation", &n.to_be_bytes())?;
    batch.put(B, b"generation", &n.to_be_bytes())?;
    batch.put_fragment(frag(n, 0), &payload(frag(n, 0)))?;
    if n > 0 {
        batch.delete_chunk(ChunkId(n - 1))?;
    }
    Ok(())
}

/// The generation a view holds, after checking that it holds all of that generation's
/// commit and nothing of another.
fn read_generation(read: &impl StorageRead) -> u64 {
    let bytes = get(read, A, b"generation").expect("a generation");
    let n = u64::from_be_bytes(bytes.try_into().expect("eight bytes"));
    let in_b = get(read, B, b"generation");
    assert_eq!(
        in_b,
        Some(n.to_be_bytes().to_vec()),
        "B is at another generation than {n}"
    );
    let chunk = fragment(read, frag(n, 0));
    assert_eq!(
        chunk,
        Some(payload(frag(n, 0))),
        "generation {n} lacks its chunk"
    );
    if n > 0 {
        let previous = fragment(read, frag(n - 1, 0));
        assert_eq!(previous, None, "generation {n} kept the previous chunk");
    }
    n
}

/// A commit applies across namespaces and fragments together; an aborted batch leaves
/// nothing behind.
pub(crate) fn atomicity<H: Harness>() {
    let mut harness = H::fresh();
    let probes = [frag(7, 0), frag(7, 1), frag(8, 0)];
    let storage = harness.storage();
    let before = snapshot(storage);
    commit(storage, Durability::Consistent, |batch| {
        batch.put(A, b"a", b"1").unwrap();
        batch.put(B, b"b", b"2").unwrap();
        batch
            .put_fragment(frag(7, 0), &payload(frag(7, 0)))
            .unwrap();
        batch
            .put_fragment(frag(7, 1), &payload(frag(7, 1)))
            .unwrap();
    });
    let committed = Dump {
        entries: vec![(A, entry(b"a", b"1")), (B, entry(b"b", b"2"))],
        fragments: vec![
            (frag(7, 0), payload(frag(7, 0))),
            (frag(7, 1), payload(frag(7, 1))),
        ],
    };
    let nothing = Dump {
        entries: Vec::new(),
        fragments: Vec::new(),
    };
    assert_eq!(dump(&before, &probes), nothing);
    assert_eq!(dump(&snapshot(storage), &probes), committed);

    let mut batch = storage.batch().unwrap();
    batch.put(A, b"a", b"changed").unwrap();
    batch.put(A, b"new", b"x").unwrap();
    batch.delete(B, b"b").unwrap();
    batch.delete_chunk(ChunkId(7)).unwrap();
    batch
        .put_fragment(frag(8, 0), &payload(frag(8, 0)))
        .unwrap();
    let aborted = Dump {
        entries: vec![(A, entry(b"a", b"changed")), (A, entry(b"new", b"x"))],
        fragments: vec![(frag(8, 0), payload(frag(8, 0)))],
    };
    assert_eq!(dump(&batch, &probes), aborted);
    drop(batch);

    assert_eq!(
        dump(&snapshot(storage), &probes),
        committed,
        "after an abort"
    );
    let batch = storage.batch().expect("a batch after an abort");
    assert_eq!(dump(&batch, &probes), committed, "a batch after an abort");
    drop(batch);
    harness.reopen();
    let reopened = dump(&snapshot(harness.storage()), &probes);
    assert_eq!(reopened, committed, "after an abort and a reopen");
}

/// A batch reads its own puts, overwrites and deletes, merged with the state it started
/// from in scans in both directions, and its own fragment writes and chunk deletions.
pub(crate) fn read_your_writes<H: Harness>() {
    let harness = H::fresh();
    let storage = harness.storage();
    commit(storage, Durability::Durable, |batch| {
        for (key, value) in [
            (b"k1", b"b1"),
            (b"k3", b"b3"),
            (b"k4", b"b4"),
            (b"k5", b"b5"),
        ] {
            batch.put(A, key, value).unwrap();
        }
        batch
            .put_fragment(frag(1, 0), &payload(frag(1, 0)))
            .unwrap();
        batch
            .put_fragment(frag(1, 1), &payload(frag(1, 1)))
            .unwrap();
    });

    let mut batch = storage.batch().unwrap();
    assert_eq!(get(&batch, A, b"k1"), Some(b"b1".to_vec()));
    batch.delete(A, b"k1").unwrap();
    batch.put(A, b"k1", b"o1").unwrap();
    batch.put(A, b"k2", b"o2").unwrap();
    batch.put(A, b"k3", b"o3").unwrap();
    batch.delete(A, b"k5").unwrap();
    batch.put(A, b"k6", b"").unwrap();
    batch.put(A, b"k7", b"gone").unwrap();
    batch.delete(A, b"k7").unwrap();
    batch.delete(A, b"k9").unwrap();
    batch.put(B, b"only", b"o").unwrap();

    let gets: [(&[u8], Option<&[u8]>); 8] = [
        (b"k1", Some(b"o1")),
        (b"k2", Some(b"o2")),
        (b"k3", Some(b"o3")),
        (b"k4", Some(b"b4")),
        (b"k5", None),
        (b"k6", Some(b"")),
        (b"k7", None),
        (b"k9", None),
    ];
    for (key, value) in gets {
        assert_eq!(get(&batch, A, key), value.map(<[u8]>::to_vec), "{key:?}");
    }
    assert_eq!(get(&batch, B, b"only"), Some(b"o".to_vec()));

    let forward = vec![
        entry(b"k1", b"o1"),
        entry(b"k2", b"o2"),
        entry(b"k3", b"o3"),
        entry(b"k4", b"b4"),
        entry(b"k6", b""),
    ];
    let backward: Vec<_> = forward.iter().rev().cloned().collect();
    assert_eq!(scan(&batch, A, KeyRange::All, Direction::Forward), forward);
    assert_eq!(
        scan(&batch, A, KeyRange::All, Direction::Backward),
        backward
    );
    let middle = KeyRange::Between(Bound::Excluded(b"k1"), Bound::Included(b"k5"));
    assert_eq!(scan(&batch, A, middle, Direction::Forward), forward[1..4]);
    assert_eq!(scan(&batch, A, middle, Direction::Backward), backward[1..4]);
    assert_eq!(scan_all(&batch, B), vec![entry(b"only", b"o")]);

    batch.put_fragment(frag(2, 0), b"two").unwrap();
    batch.put_fragment(frag(3, 0), b"three").unwrap();
    // Drops a fragment this batch wrote, then the stored ones of chunk 1.
    batch.delete_chunk(ChunkId(3)).unwrap();
    batch.delete_chunk(ChunkId(1)).unwrap();
    batch.put_fragment(frag(1, 1), b"anew").unwrap();
    let fragments: [(FragmentId, Option<&[u8]>); 4] = [
        (frag(1, 0), None),
        (frag(1, 1), Some(b"anew")),
        (frag(2, 0), Some(b"two")),
        (frag(3, 0), None),
    ];
    for (id, bytes) in fragments {
        assert_eq!(fragment(&batch, id), bytes.map(<[u8]>::to_vec), "{id:?}");
    }

    let probes = fragments.map(|(id, _)| id);
    let seen = dump(&batch, &probes);
    batch.commit(Durability::Consistent).unwrap();
    let committed = dump(&snapshot(storage), &probes);
    assert_eq!(committed, seen, "the commit applies what the batch read");
}

/// A snapshot keeps its view across namespaces and fragments while later batches commit,
/// on any thread, and is never torn by a concurrent commit.
pub(crate) fn stable_snapshots<H: Harness>() {
    let harness = H::fresh();
    let storage = harness.storage();
    commit(storage, Durability::Consistent, |batch| {
        write_generation(batch, 0)
    });

    let held = snapshot(storage);
    commit(storage, Durability::Consistent, |batch| {
        write_generation(batch, 1)
    });
    commit(storage, Durability::Durable, |batch| {
        write_generation(batch, 2)
    });
    assert_eq!(read_generation(&held), 0);
    let generation_key = entry(b"generation", &0_u64.to_be_bytes());
    assert_eq!(scan_all(&held, A), vec![generation_key]);
    let moved = thread::spawn(move || read_generation(&held));
    assert_eq!(moved.join().expect("read on another thread"), 0);

    // Captured and read on another thread while a batch is staged, then committed.
    thread::scope(|scope| {
        let (staged_tx, staged) = mpsc::channel::<()>();
        let (captured_tx, captured) = mpsc::channel::<()>();
        let (committed_tx, committed) = mpsc::channel::<()>();
        scope.spawn(move || {
            staged.recv().expect("the writer staged a batch");
            let view = snapshot(storage);
            assert_eq!(
                read_generation(&view),
                2,
                "a snapshot saw an uncommitted batch"
            );
            captured_tx.send(()).unwrap();
            committed.recv().expect("the writer committed");
            assert_eq!(
                read_generation(&view),
                2,
                "a snapshot followed a later commit"
            );
            assert_eq!(read_generation(&snapshot(storage)), 3);
        });
        let mut batch = storage.batch().unwrap();
        write_generation(&mut batch, 3);
        staged_tx.send(()).unwrap();
        captured.recv().expect("the reader captured a snapshot");
        batch.commit(Durability::Consistent).unwrap();
        committed_tx.send(()).unwrap();
    });

    // Snapshots captured while commits run are each one whole generation. The reader
    // stops at the last one, or fails once the writer is gone without reaching it.
    const LAST: u64 = 100;
    thread::scope(|scope| {
        let (writer_tx, writer) = mpsc::channel::<()>();
        scope.spawn(move || {
            loop {
                let writer_gone =
                    matches!(writer.try_recv(), Err(mpsc::TryRecvError::Disconnected));
                let n = read_generation(&snapshot(storage));
                if n == LAST {
                    break;
                }
                assert!(!writer_gone, "the writer stopped at generation {n}");
            }
        });
        for n in 4..=LAST {
            commit(storage, Durability::Consistent, |batch| {
                write_generation(batch, n)
            });
        }
        drop(writer_tx);
    });
}

/// Keys in byte order: the empty key first, each key before its extensions.
const ORDERED: [&[u8]; 10] = [
    b"",
    &[0x00],
    &[0x00, 0x00],
    &[0x01],
    &[0x01, 0x00],
    &[0x01, 0xFF],
    &[0x01, 0xFF, 0xFF],
    &[0x02],
    &[0xFF],
    &[0xFF, 0xFF],
];

fn ordered_value(index: usize) -> Vec<u8> {
    vec![b'v', index as u8]
}

/// Byte order, every kind of bound, prefixes (including empty and `0xFF`-ending ones),
/// both directions and namespace isolation, on a batch whose writes interleave with
/// stored keys and on the snapshot after its commit.
pub(crate) fn ordering_and_ranges<H: Harness>() {
    let harness = H::fresh();
    let storage = harness.storage();
    let scrambled = [7, 2, 9, 0, 5, 3, 8, 1, 6, 4];
    commit(storage, Durability::Durable, |batch| {
        for index in scrambled.into_iter().filter(|index| index % 2 == 0) {
            batch.put(A, ORDERED[index], &ordered_value(index)).unwrap();
        }
        batch.put(B, &[0x03], b"b3").unwrap();
    });
    let mut batch = storage.batch().unwrap();
    for index in scrambled.into_iter().filter(|index| index % 2 == 1) {
        batch.put(A, ORDERED[index], &ordered_value(index)).unwrap();
    }
    batch.put(B, &[0x01], b"b1").unwrap();
    check_ranges(&batch);
    batch.commit(Durability::Durable).unwrap();
    check_ranges(&snapshot(storage));
}

fn check_ranges(read: &impl StorageRead) {
    use Bound::{Excluded, Included, Unbounded};
    use Direction::{Backward, Forward};
    use KeyRange::{All, Between, Prefix};

    let every: Vec<usize> = (0..ORDERED.len()).collect();
    let every_backward: Vec<usize> = every.iter().rev().copied().collect();
    let cases: [(KeyRange<'_>, Direction, &[usize]); 32] = [
        (All, Forward, &every),
        (All, Backward, &every_backward),
        (Prefix(&[]), Forward, &every),
        (Prefix(&[]), Backward, &every_backward),
        (Prefix(&[0x00]), Forward, &[1, 2]),
        (Prefix(&[0x01]), Forward, &[3, 4, 5, 6]),
        (Prefix(&[0x01]), Backward, &[6, 5, 4, 3]),
        (Prefix(&[0x01, 0xFF]), Forward, &[5, 6]),
        (Prefix(&[0x01, 0xFF]), Backward, &[6, 5]),
        (Prefix(&[0xFF]), Forward, &[8, 9]),
        (Prefix(&[0xFF]), Backward, &[9, 8]),
        (Prefix(&[0xFF, 0xFF]), Forward, &[9]),
        (Prefix(&[0x03]), Forward, &[]),
        (Prefix(&[0x01, 0x00, 0x00]), Forward, &[]),
        (
            Between(Included(&[0x01]), Excluded(&[0x02])),
            Forward,
            &[3, 4, 5, 6],
        ),
        (
            Between(Excluded(&[0x01]), Included(&[0x02])),
            Forward,
            &[4, 5, 6, 7],
        ),
        (
            Between(Excluded(&[0x01]), Included(&[0x02])),
            Backward,
            &[7, 6, 5, 4],
        ),
        (
            Between(Unbounded, Excluded(&[0x00, 0x00])),
            Forward,
            &[0, 1],
        ),
        (
            Between(Unbounded, Included(&[0x00, 0x00])),
            Backward,
            &[2, 1, 0],
        ),
        (Between(Included(&[0xFF]), Unbounded), Forward, &[8, 9]),
        (Between(Excluded(&[0xFF]), Unbounded), Backward, &[9]),
        (Between(Unbounded, Unbounded), Forward, &every),
        (Between(Unbounded, Unbounded), Backward, &every_backward),
        (Between(Included(&[0x02]), Included(&[0x02])), Forward, &[7]),
        (
            Between(Included(&[0x02]), Included(&[0x02])),
            Backward,
            &[7],
        ),
        (Between(Included(&[0x02]), Excluded(&[0x02])), Forward, &[]),
        (Between(Excluded(&[0x02]), Excluded(&[0x02])), Backward, &[]),
        (Between(Excluded(&[0x02]), Included(&[0x02])), Forward, &[]),
        (Between(Included(&[0x02]), Included(&[0x01])), Forward, &[]),
        (Between(Included(&[0x02]), Included(&[0x01])), Backward, &[]),
        (
            Between(Excluded(&[0x01, 0xFF]), Excluded(&[0x01, 0xFF, 0xFF])),
            Forward,
            &[],
        ),
        (Between(Included(&[]), Excluded(&[0x00])), Forward, &[0]),
    ];
    for (range, direction, indices) in cases {
        let expected: Vec<Entry> = indices
            .iter()
            .map(|&index| (ORDERED[index].to_vec(), ordered_value(index)))
            .collect();
        assert_eq!(
            scan(read, A, range, direction),
            expected,
            "{range:?} {direction:?}"
        );
    }

    assert_eq!(
        scan_all(read, B),
        vec![entry(&[0x01], b"b1"), entry(&[0x03], b"b3")]
    );
    assert_eq!(get(read, A, &[0x01]), Some(ordered_value(3)));
    assert_eq!(get(read, B, &[0x01]), Some(b"b1".to_vec()));
    assert_eq!(get(read, A, &[0x03]), None);
    assert_eq!(get(read, B, &[0x02]), None);
    assert_eq!(scan_all(read, UNUSED), Vec::new());
    assert_eq!(scan(read, UNUSED, Prefix(&[]), Backward), Vec::new());
    assert_eq!(get(read, UNUSED, &[0x01]), None);
}

/// A scan returns the value its callback broke with and visits nothing after it, on
/// stored entries and on a batch's writes alike.
pub(crate) fn early_termination<H: Harness>() {
    let harness = H::fresh();
    let storage = harness.storage();
    commit(storage, Durability::Durable, |batch| {
        for key in (0..10_u8).step_by(2) {
            batch.put(A, &[key], &[b'v', key]).unwrap();
        }
    });
    let mut batch = storage.batch().unwrap();
    for key in (1..10_u8).step_by(2) {
        batch.put(A, &[key], &[b'v', key]).unwrap();
    }
    check_early_termination(&batch);
    batch.commit(Durability::Durable).unwrap();
    check_early_termination(&snapshot(storage));
}

fn check_early_termination(read: &impl StorageRead) {
    // The keys a scan visited before breaking at `stop`, and what it returned.
    let scan_until = |direction, stop: u8| {
        let mut visited = Vec::new();
        let broke = read
            .scan(A, KeyRange::All, direction, |key, value| {
                visited.push(key[0]);
                if key == [stop] {
                    ControlFlow::Break(value.to_vec())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .expect("scan");
        (visited, broke)
    };
    // Odd keys come from the batch's writes, even keys from the stored state.
    let cases: [(Direction, u8, &[u8]); 6] = [
        (Direction::Forward, 3, &[0, 1, 2, 3]),
        (Direction::Forward, 4, &[0, 1, 2, 3, 4]),
        (Direction::Forward, 0, &[0]),
        (Direction::Backward, 6, &[9, 8, 7, 6]),
        (Direction::Backward, 7, &[9, 8, 7]),
        (Direction::Backward, 9, &[9]),
    ];
    for (direction, stop, visited) in cases {
        let expected = (visited.to_vec(), Some(vec![b'v', stop]));
        assert_eq!(
            scan_until(direction, stop),
            expected,
            "{direction:?} to {stop}"
        );
    }
    let everything: Vec<u8> = (0..10).collect();
    assert_eq!(scan_until(Direction::Forward, 0xEE), (everything, None));

    let mut called = false;
    let broke = read
        .scan(A, KeyRange::Prefix(&[0x42]), Direction::Forward, |_, _| {
            called = true;
            ControlFlow::Break(())
        })
        .expect("scan");
    assert_eq!(
        (called, broke),
        (false, None),
        "an empty range calls nothing"
    );
}

/// After a mix of durable and consistent commits, a crash recovers exactly the state of
/// the last durable commit, a whole committed prefix; a clean reopen keeps the last one.
pub(crate) fn crash_consistency<H: Harness>() {
    let mut harness = H::fresh();
    let probes: Vec<FragmentId> = (0..8).map(|chunk| frag(chunk, 0)).collect();
    let commits = [
        (1, Durability::Durable),
        (2, Durability::Consistent),
        (3, Durability::Durable),
        (4, Durability::Consistent),
        (5, Durability::Consistent),
    ];
    let mut after = Vec::new();
    for (n, durability) in commits {
        let storage = harness.storage();
        commit(storage, durability, |batch| {
            write_generation(batch, n);
            batch
                .put(B, format!("only in {n}").as_bytes(), b"")
                .unwrap();
        });
        after.push(dump(&snapshot(storage), &probes));
    }

    harness.crash_and_reopen();
    let recovered = snapshot(harness.storage());
    assert_eq!(read_generation(&recovered), 3);
    assert_eq!(dump(&recovered, &probes), after[2], "the state of commit 3");

    commit(harness.storage(), Durability::Consistent, |batch| {
        write_generation(batch, 6);
    });
    let latest = dump(&snapshot(harness.storage()), &probes);
    harness.reopen();
    assert_eq!(dump(&snapshot(harness.storage()), &probes), latest);
}

const MARKER: &[u8] = b"batch id";

/// Writes batch `id`: the marker the store reads back after an unknown outcome (§7.5),
/// data in the other namespace, and a fragment.
fn write_batch(batch: &mut impl StorageWrite, id: u64) {
    batch.put(A, MARKER, &id.to_be_bytes()).unwrap();
    batch.put(B, &id.to_be_bytes(), b"data").unwrap();
    batch
        .put_fragment(frag(id, 0), &payload(frag(id, 0)))
        .unwrap();
}

fn marker(read: &impl StorageRead) -> u64 {
    let bytes = get(read, A, MARKER).expect("a marker");
    u64::from_be_bytes(bytes.try_into().expect("eight bytes"))
}

/// Whether batch `id` took effect; it has to have taken effect whole or not at all.
fn applied(read: &impl StorageRead, id: u64) -> bool {
    let data = get(read, B, &id.to_be_bytes()).is_some();
    let chunk = fragment(read, frag(id, 0)).is_some();
    assert_eq!(data, chunk, "batch {id} took effect in part");
    data
}

fn unknown(result: Result<(), CommitFailure>) -> StorageError {
    match result {
        Err(CommitFailure::Unknown { source }) => source,
        other => panic!("expected an unknown outcome, got {other:?}"),
    }
}

/// A rolled-back commit changes nothing and leaves the backend usable. An unknown outcome
/// fails the instance; after a reopen, the batch marker tells whether the batch took
/// effect, which is how the store checks its batch id (§7.5).
pub(crate) fn commit_outcomes<H: Harness>() {
    let mut harness = H::fresh();
    commit(harness.storage(), Durability::Durable, |batch| {
        write_batch(batch, 1)
    });

    harness.inject_commit(CommitOutcome::NotApplied);
    let storage = harness.storage();
    let mut batch = storage.batch().unwrap();
    write_batch(&mut batch, 2);
    match batch.commit(Durability::Durable) {
        Err(CommitFailure::NotApplied { source }) => assert_eq!(source.kind(), ErrorKind::Local),
        other => panic!("expected a rollback, got {other:?}"),
    }
    let view = snapshot(storage);
    assert_eq!((marker(&view), applied(&view, 2)), (1, false));
    commit(storage, Durability::Durable, |batch| write_batch(batch, 2));
    assert_eq!(marker(&snapshot(storage)), 2);

    for (id, took_effect) in [(3, true), (4, false)] {
        let outcome = CommitOutcome::Unknown {
            applied: took_effect,
        };
        harness.inject_commit(outcome);
        let storage = harness.storage();
        let mut batch = storage.batch().unwrap();
        write_batch(&mut batch, id);
        let error = unknown(batch.commit(Durability::Durable));
        assert_eq!(error.kind(), ErrorKind::BackendFailed, "{outcome:?}");
        assert_eq!(failure(storage.batch()).kind(), ErrorKind::BackendFailed);
        assert_eq!(failure(storage.snapshot()).kind(), ErrorKind::BackendFailed);

        harness.reopen();
        let view = snapshot(harness.storage());
        let expected = if took_effect { id } else { id - 1 };
        assert_eq!(marker(&view), expected, "{outcome:?}");
        assert_eq!(applied(&view, id), took_effect, "{outcome:?}");
    }
}

/// Operations that fail on an injected I/O failure. Reads alternate between a snapshot
/// and the batch.
#[derive(Clone, Copy, Debug, strum::VariantArray)]
enum Victim {
    Get,
    Scan,
    Fragment,
    Put,
    Delete,
    PutFragment,
    DeleteChunk,
}

impl Victim {
    fn class(self) -> OpClass {
        match self {
            Victim::Get | Victim::Scan | Victim::Fragment => OpClass::Read,
            Victim::Put | Victim::Delete | Victim::PutFragment | Victim::DeleteChunk => {
                OpClass::Write
            }
        }
    }

    fn fail(self, snapshot: &impl StorageRead, batch: &mut impl StorageWrite) -> StorageError {
        match self {
            Victim::Get => failure(snapshot.get(A, b"k", |_| ())),
            Victim::Scan => failure(batch.scan(A, KeyRange::All, Direction::Forward, |_, _| {
                ControlFlow::<()>::Continue(())
            })),
            Victim::Fragment => failure(snapshot.fragment(frag(1, 0), MAX_FRAGMENT_LEN, |_| ())),
            Victim::Put => failure(batch.put(A, b"x", b"y")),
            Victim::Delete => failure(batch.delete(A, b"k")),
            Victim::PutFragment => failure(batch.put_fragment(frag(2, 0), b"z")),
            Victim::DeleteChunk => failure(batch.delete_chunk(ChunkId(1))),
        }
    }
}

const LATCH_PROBES: [FragmentId; 2] = [
    FragmentId {
        chunk: ChunkId(1),
        index: 0,
    },
    FragmentId {
        chunk: ChunkId(2),
        index: 0,
    },
];

/// A fresh harness holding the state every latched-failure case starts from, which is
/// durable, so a reopen after the failure has to bring it back.
fn latch_baseline<H: Harness>() -> H {
    let harness = H::fresh();
    commit(harness.storage(), Durability::Durable, |batch| {
        batch.put(A, b"k", b"v").unwrap();
        batch
            .put_fragment(frag(1, 0), &payload(frag(1, 0)))
            .unwrap();
    });
    harness
}

fn baseline_with(entries: Vec<(Ns, Entry)>) -> Dump {
    Dump {
        entries,
        fragments: vec![(frag(1, 0), payload(frag(1, 0)))],
    }
}

/// Checks that every operation fails once the instance has, through `before`, a snapshot
/// captured before the failure, too; then that a reopen recovers one of `expected` and
/// works.
fn check_failed_until_reopen<H: Harness>(
    harness: &mut H,
    before: &impl StorageRead,
    expected: &[Dump],
    case: &str,
) {
    let storage = harness.storage();
    let failed = ErrorKind::BackendFailed;
    assert_eq!(failure(storage.snapshot()).kind(), failed, "{case}");
    assert_eq!(failure(storage.batch()).kind(), failed, "{case}");
    assert_eq!(
        failure(before.get(A, b"k", |_| ())).kind(),
        failed,
        "{case}"
    );
    let scanned = before.scan(A, KeyRange::All, Direction::Forward, |_, _| {
        ControlFlow::<()>::Continue(())
    });
    assert_eq!(failure(scanned).kind(), failed, "{case}");
    assert_eq!(
        failure(before.fragment(frag(1, 0), MAX_FRAGMENT_LEN, |_| ())).kind(),
        failed,
        "{case}"
    );

    harness.reopen();
    let recovered = dump(&snapshot(harness.storage()), &LATCH_PROBES);
    assert!(
        expected.contains(&recovered),
        "{case}: recovered {recovered:?}"
    );
    put_one(harness.storage(), Durability::Durable, b"after", b"1");
    let after = get(&snapshot(harness.storage()), A, b"after");
    assert_eq!(after, Some(b"1".to_vec()), "{case}");
}

/// After an injected I/O failure in a read, a write or a commit, that operation and every
/// later one report `BackendFailed`, until a reopen, which recovers the durable state.
pub(crate) fn latched_failure<H: Harness>() {
    let baseline = baseline_with(vec![(A, entry(b"k", b"v"))]);
    for &victim in <Victim as strum::VariantArray>::VARIANTS {
        let mut harness = latch_baseline::<H>();
        let storage = harness.storage();
        let before = snapshot(storage);
        let mut batch = storage.batch().unwrap();
        harness.fail_next(victim.class());
        let error = victim.fail(&before, &mut batch);
        assert_eq!(error.kind(), ErrorKind::BackendFailed, "{victim:?}");
        // The batch fails from then on too, and its commit never starts.
        assert_eq!(
            failure(batch.get(A, b"k", |_| ())).kind(),
            ErrorKind::BackendFailed
        );
        assert_eq!(
            failure(batch.put(A, b"x", b"y")).kind(),
            ErrorKind::BackendFailed
        );
        // The failure comes before any check of the arguments: an oversized fragment, which
        // a working instance refuses as `Limit`, is `Failed` here too.
        let oversized = vec![0; MAX_FRAGMENT_LEN + 1];
        assert_eq!(
            failure(batch.put_fragment(frag(2, 0), &oversized)).kind(),
            ErrorKind::BackendFailed,
            "{victim:?}"
        );
        match batch.commit(Durability::Durable) {
            Err(CommitFailure::NotApplied { source }) => {
                assert_eq!(source.kind(), ErrorKind::BackendFailed, "{victim:?}");
            }
            other => panic!("{victim:?}: expected NotApplied, got {other:?}"),
        }
        let case = format!("{victim:?}");
        check_failed_until_reopen(
            &mut harness,
            &before,
            std::slice::from_ref(&baseline),
            &case,
        );
    }

    // An I/O failure inside a commit leaves its outcome unknown: either state may follow.
    let mut harness = latch_baseline::<H>();
    let storage = harness.storage();
    let before = snapshot(storage);
    let mut batch = storage.batch().unwrap();
    batch.put(A, b"x", b"y").unwrap();
    harness.fail_next(OpClass::Commit);
    let error = unknown(batch.commit(Durability::Durable));
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    let with_batch = baseline_with(vec![(A, entry(b"k", b"v")), (A, entry(b"x", b"y"))]);
    check_failed_until_reopen(&mut harness, &before, &[baseline, with_batch], "commit");
}

/// Fragments are written, read and deleted by chunk; `delete_chunk` removes exactly one
/// chunk's fragments. The ids sit around byte boundaries, so that a chunk id or an index
/// encoded too narrowly collides with chunk 0 or index 0.
pub(crate) fn fragments<H: Harness>() {
    let harness = H::fresh();
    let storage = harness.storage();
    let chunks = [0, 1, 255, 256, 1 << 32, u64::MAX];
    let indices = [0, 1, 255, 256, u32::MAX];
    let all: Vec<FragmentId> = chunks
        .iter()
        .flat_map(|&chunk| indices.iter().map(move |&index| frag(chunk, index)))
        .collect();
    commit(storage, Durability::Durable, |batch| {
        for &id in &all {
            batch.put_fragment(id, &payload(id)).unwrap();
        }
    });
    let view = snapshot(storage);
    for &id in &all {
        assert_eq!(fragment(&view, id), Some(payload(id)), "{id:?}");
    }
    let missing = [
        frag(0, 2),
        frag(2, 0),
        frag(257, 0),
        frag((1 << 32) + 1, 0),
        frag(1 << 32, 2),
        frag(u64::MAX - 1, 0),
    ];
    for id in missing {
        assert_eq!(fragment(&view, id), None, "{id:?}");
    }

    let mut kept = chunks.to_vec();
    // The last one deletes a chunk that has no fragments.
    for deleted in [256, u64::MAX, 0, 12345] {
        commit(storage, Durability::Durable, |batch| {
            batch.delete_chunk(ChunkId(deleted)).unwrap();
        });
        kept.retain(|&chunk| chunk != deleted);
        let view = snapshot(storage);
        for &id in &all {
            let expected = kept.contains(&id.chunk.0).then(|| payload(id));
            let case = format!("{id:?} after deleting chunk {deleted}");
            assert_eq!(fragment(&view, id), expected, "{case}");
        }
    }

    commit(storage, Durability::Durable, |batch| {
        batch.put_fragment(frag(9, 0), b"").unwrap();
        batch.put_fragment(frag(1, 0), b"replaced").unwrap();
    });
    let view = snapshot(storage);
    assert_eq!(
        fragment(&view, frag(9, 0)),
        Some(Vec::new()),
        "an empty fragment"
    );
    assert_eq!(fragment(&view, frag(1, 0)), Some(b"replaced".to_vec()));
    assert_eq!(fragment(&view, frag(1, 1)), Some(payload(frag(1, 1))));
}

/// A fragment is lent only within the limit it is fetched by. One longer fails `Corrupted`
/// before the callback runs, from a snapshot and from a batch, its own writes or the state it
/// started from, and the backend stays usable; one within the limit, or a missing one, is lent
/// as ever.
pub(crate) fn fragment_limits<H: Harness>() {
    fn check(read: &impl StorageRead, chunk: u64, lens: &[usize]) {
        for (index, &len) in lens.iter().enumerate() {
            let id = frag(chunk, index as u32);
            let lent = read.fragment(id, len, |bytes| bytes.map(<[u8]>::len));
            assert_eq!(lent.unwrap(), Some(len), "{id:?}");
            if len == 0 {
                continue;
            }
            let called = Cell::new(false);
            let error = failure(read.fragment(id, len - 1, |_| called.set(true)));
            assert!(
                matches!(error, StorageError::Corrupted { .. }),
                "{id:?}: {error:?}"
            );
            assert_eq!(error.kind(), ErrorKind::Corrupted);
            assert!(!called.get(), "{id:?} was lent past its limit");
        }
        let missing = read.fragment(frag(chunk, 99), 0, |bytes| bytes.is_none());
        assert!(missing.unwrap());
    }

    let harness = H::fresh();
    let storage = harness.storage();
    let lens = [0, 100, MAX_FRAGMENT_LEN];
    commit(storage, Durability::Durable, |batch| {
        for (index, &len) in lens.iter().enumerate() {
            batch
                .put_fragment(frag(1, index as u32), &vec![7; len])
                .unwrap();
        }
    });
    check(&snapshot(storage), 1, &lens);
    let mut batch = storage.batch().unwrap();
    for (index, &len) in lens.iter().enumerate() {
        batch
            .put_fragment(frag(2, index as u32), &vec![9; len])
            .unwrap();
    }
    check(&batch, 1, &lens);
    check(&batch, 2, &lens);
    batch.commit(Durability::Durable).unwrap();
    check(&snapshot(storage), 2, &lens);
}

/// `Consistent` then a clean close: kept. `Consistent` then a crash: lost. `Durable` then
/// a crash: kept, together with the consistent commits before it.
pub(crate) fn durability_levels<H: Harness>() {
    let value = |harness: &H, key: &[u8]| get(&snapshot(harness.storage()), A, key);

    let mut harness = H::fresh();
    put_one(harness.storage(), Durability::Consistent, b"k", b"v");
    harness.crash_and_reopen();
    assert_eq!(scan_all(&snapshot(harness.storage()), A), Vec::new());

    put_one(
        harness.storage(),
        Durability::Consistent,
        b"k",
        b"consistent",
    );
    harness.reopen();
    assert_eq!(value(&harness, b"k"), Some(b"consistent".to_vec()));

    put_one(harness.storage(), Durability::Consistent, b"k", b"lost");
    harness.crash_and_reopen();
    assert_eq!(value(&harness, b"k"), Some(b"consistent".to_vec()));

    put_one(harness.storage(), Durability::Durable, b"k", b"durable");
    harness.crash_and_reopen();
    assert_eq!(value(&harness, b"k"), Some(b"durable".to_vec()));

    put_one(harness.storage(), Durability::Consistent, b"c", b"1");
    put_one(harness.storage(), Durability::Durable, b"d", b"1");
    harness.crash_and_reopen();
    assert_eq!(value(&harness, b"c"), Some(b"1".to_vec()));
    assert_eq!(value(&harness, b"d"), Some(b"1".to_vec()));
}

/// A fragment beyond the limit fails locally and changes nothing; damaged data reports
/// `Corrupted` where it is read and nowhere else; a full disk, an injected write failure,
/// fails the instance until a reopen.
pub(crate) fn failure_handling<H: Harness>() {
    let mut harness = H::fresh();
    let storage = harness.storage();
    commit(storage, Durability::Durable, |batch| {
        batch.put(A, b"a", b"1").unwrap();
        batch.put(A, b"k", b"v").unwrap();
        batch.put_fragment(frag(1, 0), b"small").unwrap();
        batch.put_fragment(frag(1, 1), b"neighbour").unwrap();
    });

    let mut batch = storage.batch().unwrap();
    let at_limit = vec![0xAB; MAX_FRAGMENT_LEN];
    batch
        .put_fragment(frag(2, 0), &at_limit)
        .expect("a fragment at the limit");
    let oversized = vec![0xCD; MAX_FRAGMENT_LEN + 1];
    for id in [frag(1, 0), frag(3, 0)] {
        let error = failure(batch.put_fragment(id, &oversized));
        assert!(matches!(error, StorageError::Limit { .. }), "{error:?}");
        assert_eq!(error.kind(), ErrorKind::Local);
    }
    assert_eq!(fragment(&batch, frag(1, 0)), Some(b"small".to_vec()));
    assert_eq!(fragment(&batch, frag(3, 0)), None);
    batch.put(A, b"b", b"2").unwrap();
    batch
        .commit(Durability::Durable)
        .expect("commit after a refused fragment");
    let view = snapshot(storage);
    assert_eq!(fragment(&view, frag(2, 0)), Some(at_limit));
    assert_eq!(fragment(&view, frag(1, 0)), Some(b"small".to_vec()));
    assert_eq!(fragment(&view, frag(3, 0)), None);
    assert_eq!(get(&view, A, b"b"), Some(b"2".to_vec()));

    harness.corrupt_entry(A, b"k");
    harness.corrupt_fragment(frag(1, 0));
    check_corrupted(&snapshot(storage));
    let batch = storage.batch().unwrap();
    check_corrupted(&batch);
    drop(batch);
    put_one(storage, Durability::Durable, b"c", b"3");
    assert_eq!(get(&snapshot(storage), A, b"c"), Some(b"3".to_vec()));

    let mut batch = storage.batch().unwrap();
    harness.fail_next(OpClass::Write);
    let error = failure(batch.put_fragment(frag(4, 0), b"no room"));
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    drop(batch);
    assert_eq!(failure(storage.snapshot()).kind(), ErrorKind::BackendFailed);
    harness.reopen();
    let view = snapshot(harness.storage());
    assert_eq!(get(&view, A, b"c"), Some(b"3".to_vec()));
    assert_eq!(fragment(&view, frag(4, 0)), None);
}

fn check_corrupted(read: &impl StorageRead) {
    let corrupted = ErrorKind::Corrupted;
    assert_eq!(failure(read.get(A, b"k", |_| ())).kind(), corrupted);
    let scanned = read.scan(A, KeyRange::All, Direction::Forward, |_, _| {
        ControlFlow::<()>::Continue(())
    });
    assert_eq!(failure(scanned).kind(), corrupted);
    assert_eq!(
        failure(read.fragment(frag(1, 0), MAX_FRAGMENT_LEN, |_| ())).kind(),
        corrupted
    );

    assert_eq!(get(read, A, b"a"), Some(b"1".to_vec()));
    let before_k = KeyRange::Between(Bound::Unbounded, Bound::Excluded(b"k"));
    let undamaged = vec![entry(b"a", b"1"), entry(b"b", b"2")];
    assert_eq!(scan(read, A, before_k, Direction::Forward), undamaged);
    assert_eq!(fragment(read, frag(1, 1)), Some(b"neighbour".to_vec()));
}

/// A snapshot whose storage was closed, cleanly or by a crash, reports `Closed`. Recovery
/// and suspension rely on it to end queries that outlive the instance (§7.4, §10.2).
pub(crate) fn closed_under_reader<H: Harness>() {
    let mut harness = H::fresh();
    commit(harness.storage(), Durability::Durable, |batch| {
        batch.put(A, b"k", b"v").unwrap();
        batch.put_fragment(frag(1, 0), b"f").unwrap();
    });
    let closed_cleanly = snapshot(harness.storage());
    harness.reopen();
    let crashed = snapshot(harness.storage());
    harness.crash_and_reopen();

    for (case, view) in [("a clean close", &closed_cleanly), ("a crash", &crashed)] {
        let scanned = view.scan(A, KeyRange::All, Direction::Forward, |_, _| {
            ControlFlow::<()>::Continue(())
        });
        let errors = [
            failure(view.get(A, b"k", |_| ())),
            failure(scanned),
            failure(view.fragment(frag(1, 0), MAX_FRAGMENT_LEN, |_| ())),
        ];
        for error in errors {
            assert!(
                matches!(error, StorageError::Closed),
                "after {case}: {error:?}"
            );
            assert_eq!(error.kind(), ErrorKind::Local);
        }
    }
    assert_eq!(
        get(&snapshot(harness.storage()), A, b"k"),
        Some(b"v".to_vec())
    );
}
