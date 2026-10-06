use super::{MigrationChildFailed, run_migration_command, wait_for_migration_output};
use crate::core::backup::BACKUP_FAILED_EXIT_CODE;
use std::{
    fs::File,
    io::{Read, Write},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

const FIXTURE_ENV: &str = "NYANPASU_MIGRATION_PROCESS_TEST";
const FIXTURE_TEST: &str = "utils::init::tests::migration_process_fixture";
const STDOUT_TAIL: &str = "stdout tail 完整";
const STDERR_TAIL: &str = "stderr tail 完整";
const STDOUT_LINE: &str = "stdout block 完整\n";
const STDERR_LINE: &str = "stderr block 完整\n";
const OUTPUT_LINES: usize = 16_384;

fn fixture_command(case: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", FIXTURE_TEST, "--nocapture", "--test-threads=1"])
        .env(FIXTURE_ENV, case);
    command
}

fn run_fixture(case: &str) -> (anyhow::Result<()>, String) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("migration.log");
    let result = run_migration_command(fixture_command(case), File::create(&path).unwrap());
    (result, std::fs::read_to_string(path).unwrap())
}

#[test]
fn migration_collects_both_unterminated_utf8_tails_before_returning() {
    let (result, log) = run_fixture("success");
    result.unwrap();
    assert!(log.contains(STDOUT_TAIL));
    assert!(log.contains(STDERR_TAIL));
}

#[test]
fn migration_failure_preserves_complete_stderr_and_exit_code() {
    for (case, code) in [("failure", 1), ("backup", BACKUP_FAILED_EXIT_CODE)] {
        let (result, log) = run_fixture(case);
        let error = result.unwrap_err();
        let failed = error.downcast_ref::<MigrationChildFailed>().unwrap();
        assert_eq!(failed.status.code(), Some(code));
        assert_eq!(failed.stderr, STDERR_TAIL);
        assert!(log.contains(STDOUT_TAIL));
        assert!(log.contains(STDERR_TAIL));
    }
}

#[test]
fn migration_drains_both_streams_beyond_pipe_capacity() {
    let (result, log) = run_fixture("large");
    result.unwrap();
    assert_eq!(log.matches(STDOUT_LINE).count(), OUTPUT_LINES);
    assert_eq!(log.matches(STDERR_LINE).count(), OUTPUT_LINES);
    assert!(log.contains(STDOUT_TAIL));
    assert!(log.contains(STDERR_TAIL));
}

#[test]
fn migration_waits_for_delayed_readers_even_after_the_child_has_exited() {
    let mut child = fixture_command("success")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    assert!(child.wait().unwrap().success());

    let completed = Arc::new([AtomicBool::new(false), AtomicBool::new(false)]);
    let (ready_tx, ready_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let mut releases = Vec::new();
    let readers = std::array::from_fn(|index| {
        let (release_tx, release_rx) = mpsc::channel();
        releases.push(release_tx);
        let ready = ready_tx.clone();
        let finished = finished_tx.clone();
        let completed = completed.clone();
        thread::spawn(move || {
            ready.send(index).unwrap();
            release_rx.recv().unwrap();
            completed[index].store(true, Ordering::SeqCst);
            finished.send(index).unwrap();
        })
    });
    ready_rx.recv().unwrap();
    ready_rx.recv().unwrap();

    let (waiting_tx, waiting_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        waiting_tx.send(()).unwrap();
        let status = wait_for_migration_output(&mut child, readers).unwrap();
        result_tx
            .send((
                status,
                completed.each_ref().map(|done| done.load(Ordering::SeqCst)),
            ))
            .unwrap();
    });
    waiting_rx.recv().unwrap();
    releases[0].send(()).unwrap();
    assert_eq!(finished_rx.recv().unwrap(), 0);
    let early_result = result_rx.try_recv().ok();
    releases[1].send(()).unwrap();
    assert_eq!(finished_rx.recv().unwrap(), 1);
    let returned_early = early_result.is_some();
    let (status, completed) = early_result.unwrap_or_else(|| result_rx.recv().unwrap());
    waiter.join().unwrap();
    assert!(!returned_early, "returned while stderr was still blocked");
    assert!(status.success());
    assert_eq!(completed, [true, true]);
}

#[test]
fn migration_reader_panic_is_resumed_after_joining_both_readers() {
    let mut child = fixture_command("success")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let completed = Arc::new(AtomicBool::new(false));
    let completed_ = completed.clone();
    let readers = [
        thread::spawn(|| panic!("stdout reader panic")),
        thread::spawn(move || completed_.store(true, Ordering::SeqCst)),
    ];
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wait_for_migration_output(&mut child, readers)
    }))
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"stdout reader panic"));
    assert!(completed.load(Ordering::SeqCst));
}

#[test]
fn relaunch_keeps_inherited_stdout_and_stderr_after_helper_exit() {
    let mut launcher = fixture_command("launcher")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Keep stdin outside Child: wait() would otherwise close the descendant's control pipe.
    let mut release = launcher.stdin.take().unwrap();
    let mut stdout = launcher.stdout.take().unwrap();
    let mut stderr = launcher.stderr.take().unwrap();
    let stdout_reader = thread::spawn(move || {
        let mut output = String::new();
        stdout.read_to_string(&mut output).unwrap();
        output
    });
    let stderr_reader = thread::spawn(move || {
        let mut output = String::new();
        stderr.read_to_string(&mut output).unwrap();
        output
    });
    assert!(launcher.wait().unwrap().success());
    // Only now may the new instance write: its helper has already terminated.
    release.write_all(b"!").unwrap();
    assert!(stdout_reader.join().unwrap().contains(STDOUT_TAIL));
    assert!(stderr_reader.join().unwrap().contains(STDERR_TAIL));
}

#[test]
fn migration_process_fixture() {
    let Ok(case) = std::env::var(FIXTURE_ENV) else {
        return;
    };
    match case.as_str() {
        "launcher" => {
            // Mirror the synchronous spawn and default stdio inheritance of the launch bridge.
            #[allow(clippy::zombie_processes)]
            let child = fixture_command("descendant").spawn().unwrap();
            drop(child);
            std::process::exit(0);
        }
        "descendant" => {
            let mut release = [0];
            std::io::stdin().read_exact(&mut release).unwrap();
        }
        "large" => {
            std::io::stdout()
                .write_all(STDOUT_LINE.repeat(OUTPUT_LINES).as_bytes())
                .unwrap();
            std::io::stderr()
                .write_all(STDERR_LINE.repeat(OUTPUT_LINES).as_bytes())
                .unwrap();
        }
        "success" | "failure" | "backup" => {}
        _ => panic!("unknown process fixture: {case}"),
    }
    std::io::stdout().write_all(STDOUT_TAIL.as_bytes()).unwrap();
    std::io::stderr().write_all(STDERR_TAIL.as_bytes()).unwrap();
    let code = match case.as_str() {
        "failure" => 1,
        "backup" => BACKUP_FAILED_EXIT_CODE,
        _ => 0,
    };
    std::process::exit(code);
}

#[cfg(windows)]
#[test]
fn test_get_current_user_sid() {
    let sid = super::current_user_sid().unwrap();
    assert!(!sid.is_empty());
    // SID should start with "S-" followed by numbers
    assert!(sid.starts_with("S-"));
    println!("Current user SID: {}", sid);
}

#[cfg(windows)]
#[test]
fn test_get_single_instance_placeholder_with_sid() {
    let dir = tempfile::tempdir().unwrap();
    let placeholder = super::single_instance_placeholder("clash-nyanpasu", dir.path());
    assert!(!placeholder.is_empty());
    // Should contain the app name
    assert!(placeholder.contains("clash-nyanpasu") || placeholder.contains("clash-nyanpasu-dev"));
    println!("Single instance placeholder: {}", placeholder);
}
