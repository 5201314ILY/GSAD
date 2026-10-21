use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use gsad_core::{AtomicFileSink, CommitSemantics, ErrorKind, OutputSink};

static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(1);

fn make_temp_dir() -> PathBuf {
    let serial = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "gsad-file-sink-tests-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&path).expect("create unique test directory");
    path
}

fn write_all_to_sink(sink: &mut AtomicFileSink, bytes: &[u8]) {
    let mut offset = 0;

    while offset < bytes.len() {
        let written = sink.write(&bytes[offset..]).expect("write to sink");
        assert!(written > 0, "sink must not make zero progress");
        offset += written;
    }
}

#[test]
fn commit_publishes_complete_file_and_removes_temporary_name() {
    let dir = make_temp_dir();
    let destination = dir.join("output.bin");
    let payload = b"GSAD atomic output payload";

    let mut sink = AtomicFileSink::new(&destination).expect("create sink");
    let temporary = sink.temporary_path().to_path_buf();

    write_all_to_sink(&mut sink, payload);

    assert_eq!(sink.commit_semantics(), CommitSemantics::Atomic);
    sink.commit().expect("commit output");

    assert_eq!(fs::read(&destination).expect("read destination"), payload);
    assert!(!temporary.exists(), "temporary name should be removed");
    assert_eq!(
        sink.write(b"late data")
            .expect_err("write after successful commit must fail")
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        sink.flush()
            .expect_err("flush after successful commit must fail")
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        sink.commit().expect_err("repeated commit must fail").kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        fs::read_dir(&dir).expect("read directory").count(),
        1,
        "only the published destination should remain"
    );

    drop(sink);
    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn existing_destination_is_never_overwritten_after_commit_conflict() {
    let dir = make_temp_dir();
    let destination = dir.join("output.bin");

    fs::write(&destination, b"original").expect("write original destination");

    let mut sink = AtomicFileSink::new(&destination).expect("create sink");
    let temporary = sink.temporary_path().to_path_buf();

    write_all_to_sink(&mut sink, b"replacement");

    let error = sink
        .commit()
        .expect_err("existing destination must conflict");

    assert!(
        !temporary.exists(),
        "failed commit should immediately remove the temporary file"
    );
    assert_eq!(
        sink.write(b"x")
            .expect_err("failed commit must finalize the sink")
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        sink.commit()
            .expect_err("failed commit cannot be retried")
            .kind,
        ErrorKind::Conflict
    );

    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(
        fs::read(&destination).expect("read original destination"),
        b"original"
    );

    // Abort remains safe after the failed commit has already attempted cleanup.
    sink.abort()
        .expect("abort after failed commit should be safe");
    assert!(
        !temporary.exists(),
        "abort should remove the temporary file"
    );
    assert_eq!(
        fs::read(&destination).expect("read destination again"),
        b"original"
    );

    drop(sink);
    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn abort_is_idempotent_and_finalizes_the_sink() {
    let dir = make_temp_dir();
    let destination = dir.join("output.bin");

    let mut sink = AtomicFileSink::new(&destination).expect("create sink");
    let temporary = sink.temporary_path().to_path_buf();

    write_all_to_sink(&mut sink, b"discard me");

    sink.abort().expect("abort sink");
    sink.abort().expect("repeated abort should be harmless");

    assert!(!temporary.exists());
    assert!(!destination.exists());

    assert_eq!(
        sink.write(b"x")
            .expect_err("write after abort must fail")
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        sink.flush().expect_err("flush after abort must fail").kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        sink.commit()
            .expect_err("commit after abort must fail")
            .kind,
        ErrorKind::Conflict
    );

    drop(sink);
    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn dropping_uncommitted_sink_removes_temporary_file() {
    let dir = make_temp_dir();
    let destination = dir.join("output.bin");

    let temporary = {
        let mut sink = AtomicFileSink::new(&destination).expect("create sink");
        let temporary = sink.temporary_path().to_path_buf();
        write_all_to_sink(&mut sink, b"discard on drop");
        temporary
    };

    assert!(!temporary.exists(), "drop should remove temporary file");
    assert!(!destination.exists(), "drop must not publish output");

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn competing_sinks_never_replace_the_first_published_destination() {
    let dir = make_temp_dir();
    let destination = dir.join("output.bin");

    let first_payload = b"first writer";
    let second_payload = b"second writer";

    let mut first = AtomicFileSink::new(&destination).expect("create first sink");
    let mut second = AtomicFileSink::new(&destination).expect("create second sink");
    let second_temporary = second.temporary_path().to_path_buf();

    write_all_to_sink(&mut first, first_payload);
    write_all_to_sink(&mut second, second_payload);

    // The first writer publishes successfully.
    first.commit().expect("first writer should publish");

    // The second writer must fail without replacing the first output.
    let error = second
        .commit()
        .expect_err("second writer must not replace destination");

    assert_eq!(error.kind, ErrorKind::Conflict);

    // The failed commit should immediately clean up and finalize the sink.
    assert!(
        !second_temporary.exists(),
        "losing writer should immediately clean up its temporary file"
    );
    assert_eq!(
        second
            .write(b"x")
            .expect_err("losing writer must be finalized")
            .kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        second
            .commit()
            .expect_err("failed commit cannot be retried")
            .kind,
        ErrorKind::Conflict
    );

    assert_eq!(
        fs::read(&destination).expect("read published destination"),
        first_payload,
        "the second writer must not replace the first writer's output"
    );

    // Abort remains safe after a failed commit.
    second
        .abort()
        .expect("abort after failed commit should be safe");

    assert_eq!(
        fs::read_dir(&dir).expect("read directory").count(),
        1,
        "only the published destination should remain"
    );

    drop(first);
    drop(second);
    fs::remove_dir_all(dir).expect("remove test directory");
}
