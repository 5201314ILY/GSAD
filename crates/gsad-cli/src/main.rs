use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use gsad_core::source::FileSource;
use gsad_core::{
    execute_to_sink, AtomicFileSink, DataKind, DataSource, EngineRequest, ExecutionContext,
    GsadError, Operation, ProcessingEngine, ResourceLimits, RleEngine, ZstdEngine,
};

const ZSTD_MAGIC: &[u8; 4] = b"\x28\xb5\x2f\xfd";
const RLE_MAGIC: &[u8; 8] = b"GSARLE01";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("gsad: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> gsad_core::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let command = args
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(usage_error)?;
    let operation = match command.as_str() {
        "compress" => Operation::Compress,
        "decompress" => Operation::Decompress,
        "help" | "--help" | "-h" => {
            print_help();
            return Ok(());
        }
        _ => return Err(usage_error()),
    };

    // Compression defaults to Zstandard. Decompression defaults to signature-based
    // detection, because the output file extension is not a reliable format signal.
    let mut codec_override: Option<String> = None;
    let mut level: Option<String> = None;
    let mut positional: Vec<OsString> = Vec::new();
    let remaining: Vec<OsString> = args.collect();
    let mut index = 0;
    while index < remaining.len() {
        if remaining[index] == OsStr::new("--codec") {
            if codec_override.is_some() {
                return Err(GsadError::invalid_argument(
                    "--codec may only be specified once",
                ));
            }
            index += 1;
            let value = remaining.get(index).ok_or_else(usage_error)?;
            codec_override = Some(
                value
                    .to_str()
                    .ok_or_else(|| GsadError::invalid_argument("codec name must be valid UTF-8"))?
                    .to_owned(),
            );
        } else if remaining[index] == OsStr::new("--level") {
            if level.is_some() {
                return Err(GsadError::invalid_argument(
                    "--level may only be specified once",
                ));
            }
            index += 1;
            let value = remaining.get(index).ok_or_else(usage_error)?;
            level = Some(
                value
                    .to_str()
                    .ok_or_else(|| GsadError::invalid_argument("level must be valid UTF-8"))?
                    .to_owned(),
            );
        } else if remaining[index].to_string_lossy().starts_with('-') {
            return Err(GsadError::invalid_argument(format!(
                "unknown option: {}",
                remaining[index].to_string_lossy()
            )));
        } else {
            positional.push(remaining[index].clone());
        }
        index += 1;
    }
    if positional.len() != 2 {
        return Err(usage_error());
    }
    if operation == Operation::Decompress && level.is_some() {
        return Err(GsadError::invalid_argument(
            "--level is only valid with the compress operation",
        ));
    }

    let input_path = PathBuf::from(&positional[0]);
    let output_path = PathBuf::from(&positional[1]);
    let source: Arc<dyn DataSource> = Arc::new(FileSource::open(&input_path)?);
    let codec = select_codec(operation, codec_override.as_deref(), source.as_ref())?;

    if codec == "rle" && level.is_some() {
        return Err(GsadError::invalid_argument(
            "--level is only supported by the zstd codec",
        ));
    }

    let engine: Box<dyn ProcessingEngine> = match codec {
        "zstd" => Box::new(ZstdEngine::new()),
        "rle" => Box::new(RleEngine::new()),
        _ => {
            return Err(GsadError::internal(
                "codec selection returned an invalid codec",
            ))
        }
    };

    let input_length = source.info().known_length.unwrap_or(0);
    let mut parameters = BTreeMap::new();
    if let Some(level) = level {
        parameters.insert("level".to_owned(), level);
    }
    let request = EngineRequest {
        operation,
        input_kind: DataKind::Bytes,
        input: Arc::clone(&source),
        parameters,
    };
    let mut limits = ResourceLimits::default();
    if codec == "zstd" {
        // This CLI setting permits highly compressible data produced by GSAD.
        // The absolute output cap remains active; untrusted-data callers should
        // configure a stricter expansion ratio in a higher-level API.
        limits.max_expansion_ratio = Some(1_000_000);
    }
    let context = ExecutionContext::new(limits)?;
    let mut sink = AtomicFileSink::new(&output_path)?;
    let report = execute_to_sink(engine.as_ref(), &request, &context, &mut sink)?;

    println!(
        "ok operation={:?} codec={} input_bytes={} output_bytes={} output_kind={:?} output={}",
        operation,
        codec,
        input_length,
        report.bytes_written,
        report.data_kind,
        output_path.display()
    );
    Ok(())
}

fn select_codec(
    operation: Operation,
    override_codec: Option<&str>,
    source: &dyn DataSource,
) -> gsad_core::Result<&'static str> {
    match operation {
        Operation::Compress => match override_codec.unwrap_or("zstd") {
            "zstd" => Ok("zstd"),
            "rle" => Ok("rle"),
            "auto" => Err(GsadError::invalid_argument(
                "--codec auto is only valid with decompress",
            )),
            _ => Err(GsadError::invalid_argument(
                "unsupported codec; choose 'zstd' or 'rle' for compression",
            )),
        },
        Operation::Decompress => match override_codec {
            None | Some("auto") => detect_codec(source),
            Some("zstd") => Ok("zstd"),
            Some("rle") => Ok("rle"),
            Some(_) => Err(GsadError::invalid_argument(
                "unsupported codec; choose 'auto', 'zstd', or 'rle' for decompression",
            )),
        },
        other => Err(GsadError::unsupported(format!(
            "CLI operation is not supported: {other:?}"
        ))),
    }
}

fn detect_codec(source: &dyn DataSource) -> gsad_core::Result<&'static str> {
    let mut reader = source.open_reader()?;
    let mut prefix = [0_u8; 8];
    let mut filled = 0;
    while filled < prefix.len() {
        let count = reader.read(&mut prefix[filled..])?;
        if count == 0 {
            break;
        }
        if count > prefix.len() - filled {
            return Err(GsadError::internal(
                "data source returned more bytes than the requested signature buffer",
            ));
        }
        filled += count;
    }
    detect_codec_from_prefix(&prefix[..filled]).ok_or_else(|| {
        GsadError::unsupported(
            "compressed format signature is unknown; specify --codec zstd or --codec rle only if the format is known",
        )
    })
}

fn detect_codec_from_prefix(prefix: &[u8]) -> Option<&'static str> {
    if prefix.starts_with(RLE_MAGIC) {
        Some("rle")
    } else if prefix.starts_with(ZSTD_MAGIC) {
        Some("zstd")
    } else {
        None
    }
}

fn usage_error() -> GsadError {
    GsadError::invalid_argument(
        "usage: gsad <compress|decompress> [--codec zstd|rle|auto] [--level N] <input-file> <new-output-file>",
    )
}

fn print_help() {
    println!(
        "GSAD 鈥?Generalized Stray-Arrow Decoding\n\n\
         Usage:\n\
           gsad compress [--codec zstd|rle] [--level N] <input-file> <new-output-file>\n\
           gsad decompress [--codec auto|zstd|rle] <input-file> <new-output-file>\n\n\
         Compression defaults to Zstandard. Decompression defaults to signature-based auto detection.\n\
         Supported signatures: standard Zstandard frame and experimental GSARLE01.\n\
         The RLE codec is for contract testing, not general-purpose compression.\n\
         Existing output files are never overwritten. Built-in resource limits apply."
    );
}

#[cfg(test)]
mod tests {
    use super::detect_codec_from_prefix;

    #[test]
    fn detects_standard_zstandard_frame_magic() {
        assert_eq!(
            detect_codec_from_prefix(b"\x28\xb5\x2f\xfd\x00\x01"),
            Some("zstd")
        );
    }

    #[test]
    fn detects_experimental_rle_signature() {
        assert_eq!(detect_codec_from_prefix(b"GSARLE01\x01\x42"), Some("rle"));
    }

    #[test]
    fn unknown_or_truncated_signature_is_not_guessed() {
        assert_eq!(detect_codec_from_prefix(b""), None);
        assert_eq!(detect_codec_from_prefix(b"\x28\xb5"), None);
        assert_eq!(detect_codec_from_prefix(b"not-a-format"), None);
    }
}
