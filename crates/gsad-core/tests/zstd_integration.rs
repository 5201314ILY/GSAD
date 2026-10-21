use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use gsad_core::source::FileSource;
use gsad_core::{
    execute_to_sink, AtomicFileSink, DataKind, DataSource, EngineRequest, ErrorKind,
    ExecutionContext, Operation, ProcessingEngine, ResourceLimits, ZstdEngine,
};

static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(1);

fn make_temp_dir() -> PathBuf {
    let serial = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("gsad-zstd-tests-{}-{serial}", std::process::id()));
    fs::create_dir(&path).expect("create unique test directory");
    path
}

fn process_zstd(
    input_path: &Path,
    output_path: &Path,
    operation: Operation,
    max_input_bytes: u64,
    max_output_bytes: u64,
    max_expansion_ratio: Option<u32>,
    parameters: BTreeMap<String, String>,
) -> gsad_core::Result<u64> {
    let source: Arc<dyn DataSource> = Arc::new(FileSource::open(input_path)?);
    let request = EngineRequest {
        operation,
        input_kind: DataKind::Bytes,
        input: source,
        parameters,
    };
    let context = ExecutionContext::new(ResourceLimits {
        max_input_bytes,
        max_output_bytes,
        max_memory_bytes: 1024 * 1024,
        max_expansion_ratio,
    })?;
    let engine = ZstdEngine::new();
    let engine_ref: &dyn ProcessingEngine = &engine;
    let mut sink = AtomicFileSink::new(output_path)?;
    let report = execute_to_sink(engine_ref, &request, &context, &mut sink)?;
    Ok(report.bytes_written)
}

#[test]
fn zstd_stream_round_trip_and_standard_frame_magic() {
    let dir = make_temp_dir();
    let input = dir.join("input.bin");
    let packed = dir.join("input.zst");
    let restored = dir.join("restored.bin");

    let mut bytes = Vec::with_capacity(400_000);
    for index in 0..400_000_u32 {
        if index % 17 < 13 {
            bytes.push(b'A' + (index % 4) as u8);
        } else {
            bytes.push(((index * 73 + 19) % 251) as u8);
        }
    }
    fs::write(&input, &bytes).expect("write source fixture");

    let packed_len = process_zstd(
        &input,
        &packed,
        Operation::Compress,
        2 * 1024 * 1024,
        2 * 1024 * 1024,
        Some(200),
        BTreeMap::new(),
    )
    .expect("compress with Zstandard");
    let frame = fs::read(&packed).expect("read compressed frame");
    assert!(packed_len > 4);
    assert_eq!(&frame[..4], &[0x28, 0xb5, 0x2f, 0xfd]);

    let restored_len = process_zstd(
        &packed,
        &restored,
        Operation::Decompress,
        2 * 1024 * 1024,
        2 * 1024 * 1024,
        Some(200),
        BTreeMap::new(),
    )
    .expect("decompress Zstandard frame");
    assert_eq!(restored_len, bytes.len() as u64);
    assert_eq!(fs::read(&restored).expect("read restored data"), bytes);

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn zstd_checksum_corruption_does_not_publish_output() {
    let dir = make_temp_dir();
    let input = dir.join("input.bin");
    let packed = dir.join("input.zst");
    let corrupted = dir.join("corrupted.zst");
    let output = dir.join("output.bin");
    fs::write(&input, vec![b'Q'; 96_000]).expect("write input");

    process_zstd(
        &input,
        &packed,
        Operation::Compress,
        1024 * 1024,
        1024 * 1024,
        Some(200),
        BTreeMap::new(),
    )
    .expect("compress input");
    let mut frame = fs::read(&packed).expect("read frame");
    let last = frame.last_mut().expect("non-empty frame");
    *last ^= 0x5a;
    fs::write(&corrupted, frame).expect("write corrupted frame");

    let error = process_zstd(
        &corrupted,
        &output,
        Operation::Decompress,
        1024 * 1024,
        1024 * 1024,
        Some(1_000_000),
        BTreeMap::new(),
    )
    .expect_err("checksum corruption should fail");
    assert_eq!(error.kind, ErrorKind::CorruptData);
    assert!(!output.exists());

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn zstd_expansion_ratio_limit_aborts_without_publishing() {
    let dir = make_temp_dir();
    let input = dir.join("input.bin");
    let packed = dir.join("packed.zst");
    let output = dir.join("output.bin");
    fs::write(&input, vec![b'Z'; 256 * 1024]).expect("write repetitive input");
    process_zstd(
        &input,
        &packed,
        Operation::Compress,
        1024 * 1024,
        1024 * 1024,
        Some(200),
        BTreeMap::new(),
    )
    .expect("compress repetitive input");

    let error = process_zstd(
        &packed,
        &output,
        Operation::Decompress,
        1024 * 1024,
        1024 * 1024,
        Some(2),
        BTreeMap::new(),
    )
    .expect_err("expansion ratio limit should reject this frame");
    assert_eq!(error.kind, ErrorKind::ResourceLimit);
    assert!(!output.exists());

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn highly_compressible_stream_round_trips_with_generic_default_ratio() {
    let dir = make_temp_dir();
    let input = dir.join("zeroes.bin");
    let packed = dir.join("zeroes.zst");
    let restored = dir.join("restored.bin");
    let original = vec![0_u8; 1024 * 1024];
    fs::write(&input, &original).expect("write highly repetitive source");

    process_zstd(
        &input,
        &packed,
        Operation::Compress,
        2 * 1024 * 1024,
        2 * 1024 * 1024,
        Some(1_000_000),
        BTreeMap::new(),
    )
    .expect("compress highly repetitive source");
    process_zstd(
        &packed,
        &restored,
        Operation::Decompress,
        2 * 1024 * 1024,
        2 * 1024 * 1024,
        Some(1_000_000),
        BTreeMap::new(),
    )
    .expect("decompress highly repetitive source");
    assert_eq!(fs::read(&restored).expect("read restored data"), original);

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn empty_input_round_trips() {
    let dir = make_temp_dir();
    let input = dir.join("empty.bin");
    let packed = dir.join("empty.zst");
    let restored = dir.join("restored.bin");
    fs::write(&input, b"").expect("write empty source");

    process_zstd(
        &input,
        &packed,
        Operation::Compress,
        1024,
        1024,
        Some(1_000_000),
        BTreeMap::new(),
    )
    .expect("compress empty source");
    process_zstd(
        &packed,
        &restored,
        Operation::Decompress,
        1024,
        1024,
        Some(1_000_000),
        BTreeMap::new(),
    )
    .expect("decompress empty source");
    assert_eq!(fs::read(&restored).expect("read restored output"), b"");

    fs::remove_dir_all(dir).expect("remove test directory");
}

#[test]
fn unsupported_zstd_parameter_fails_before_output_is_published() {
    let dir = make_temp_dir();
    let input = dir.join("input.bin");
    let output = dir.join("output.zst");
    fs::write(&input, b"hello").expect("write input");
    let mut parameters = BTreeMap::new();
    parameters.insert("unknown".to_owned(), "value".to_owned());

    let error = process_zstd(
        &input,
        &output,
        Operation::Compress,
        1024,
        1024,
        Some(200),
        parameters,
    )
    .expect_err("unknown parameters should be rejected");
    assert_eq!(error.kind, ErrorKind::InvalidArgument);
    assert!(!output.exists());

    fs::remove_dir_all(dir).expect("remove test directory");
}
