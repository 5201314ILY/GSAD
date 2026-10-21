use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use gsad_core::source::FileSource;
use gsad_core::{
    execute_to_sink, AtomicFileSink, DataKind, DataSource, EngineRequest, ErrorKind,
    ExecutionContext, Operation, ResourceLimits, RleEngine,
};

static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(1);

fn make_temp_dir() -> PathBuf {
    let serial = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("gsad-tests-{}-{serial}", std::process::id()));
    fs::create_dir(&path).expect("create unique test directory");
    path
}

fn process_file(
    input_path: &Path,
    output_path: &Path,
    operation: Operation,
    max_output_bytes: u64,
) -> gsad_core::Result<u64> {
    let source: Arc<dyn DataSource> = Arc::new(FileSource::open(input_path)?);
    let request = EngineRequest {
        operation,
        input_kind: DataKind::Bytes,
        input: source,
        parameters: BTreeMap::new(),
    };
    let context = ExecutionContext::new(ResourceLimits {
        max_input_bytes: 8 * 1024 * 1024,
        max_output_bytes,
        max_memory_bytes: 1024 * 1024,
        max_expansion_ratio: Some(200),
    })?;
    let engine = RleEngine::new();
    let mut sink = AtomicFileSink::new(output_path)?;
    let report = execute_to_sink(&engine, &request, &context, &mut sink)?;
    Ok(report.bytes_written)
}

#[test]
fn file_source_sequential_and_random_access_match() {
    let dir = make_temp_dir();
    let input = dir.join("source.bin");
    fs::write(&input, b"0123456789").expect("write source");
    let source = FileSource::open(&input).expect("open source");

    let mut sequential = source.open_reader().expect("open sequential reader");
    let mut first = [0_u8; 4];
    assert_eq!(sequential.read(&mut first).expect("sequential read"), 4);
    assert_eq!(&first, b"0123");

    let mut random = [0_u8; 3];
    assert_eq!(source.read_at(5, &mut random).expect("random read"), 3);
    assert_eq!(&random, b"567");

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn file_round_trip_is_streamed_and_published() {
    let dir = make_temp_dir();
    let input = dir.join("input.bin");
    let packed = dir.join("packed.gsarr");
    let restored = dir.join("restored.bin");
    let mut bytes = Vec::new();
    for i in 0..300_000_u32 {
        bytes.push(if i % 9 < 7 { b'A' } else { (i % 251) as u8 });
    }
    fs::write(&input, &bytes).expect("write input fixture");

    let packed_len =
        process_file(&input, &packed, Operation::Compress, 1024 * 1024).expect("compress file");
    assert!(packed_len > 8);
    let restored_len = process_file(&packed, &restored, Operation::Decompress, 1024 * 1024)
        .expect("decompress file");
    assert_eq!(restored_len, bytes.len() as u64);
    assert_eq!(fs::read(&restored).expect("read output"), bytes);

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn file_sink_never_overwrites_existing_destination() {
    let dir = make_temp_dir();
    let input = dir.join("input.bin");
    let output = dir.join("output.bin");
    fs::write(&input, b"payload").expect("write input");
    fs::write(&output, b"keep-this").expect("write existing output");

    let error = process_file(&input, &output, Operation::Compress, 1024)
        .expect_err("existing output must not be replaced");
    assert_eq!(error.kind, ErrorKind::Conflict);
    assert_eq!(
        fs::read(&output).expect("read original output"),
        b"keep-this"
    );
    assert_eq!(fs::read_dir(&dir).expect("read directory").count(), 2);

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn malformed_input_leaves_no_published_output_or_temp_file() {
    let dir = make_temp_dir();
    let input = dir.join("bad.bin");
    let output = dir.join("output.bin");

    fs::write(&input, b"not an RLE stream").expect("write malformed input");
    let error = process_file(&input, &output, Operation::Decompress, 1024)
        .expect_err("bad signature must fail");
    assert_eq!(error.kind, ErrorKind::CorruptData);
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&dir).expect("read directory").count(), 1);

    fs::write(&input, b"GSARLE01\x01").expect("write truncated record");
    let error = process_file(&input, &output, Operation::Decompress, 1024)
        .expect_err("truncated record must fail");
    assert_eq!(error.kind, ErrorKind::UnexpectedEof);
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&dir).expect("read directory").count(), 1);

    fs::write(&input, b"GSARLE01\x00A").expect("write zero-count record");
    let error = process_file(&input, &output, Operation::Decompress, 1024)
        .expect_err("zero count must fail");
    assert_eq!(error.kind, ErrorKind::CorruptData);
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&dir).expect("read directory").count(), 1);

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn decompression_output_limit_prevents_publish() {
    let dir = make_temp_dir();
    let input = dir.join("input.bin");
    let packed = dir.join("packed.bin");
    let output = dir.join("output.bin");
    fs::write(&input, vec![b'Z'; 20_000]).expect("write input");
    process_file(&input, &packed, Operation::Compress, 100_000).expect("compress file");

    let error = process_file(&packed, &output, Operation::Decompress, 128)
        .expect_err("decompression should hit output limit");
    assert_eq!(error.kind, ErrorKind::ResourceLimit);
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&dir).expect("read directory").count(), 2);

    fs::remove_dir_all(dir).expect("remove test directory");
}
