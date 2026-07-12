use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::scanner::{CompressionTarget, EntryKind};

use super::{CompressionStateKind, inspect};

const SAMPLE_RANGE_BYTES: usize = 256 * 1024;
const MAX_SAMPLE_RANGES: usize = 3;
const READ_CHUNK_BYTES: usize = 64 * 1024;
const ALLOCATION_GRANULARITY: u64 = 4 * 1024;
const ESTIMATOR_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
// Unsupported is produced only by native targets without an estimator codec.
#[allow(dead_code)]
pub(crate) enum EstimateStatus {
    Estimated,
    NotCandidate,
    Unsupported,
    Unavailable,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum EstimateConfidence {
    High,
    Medium,
    Low,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum AlgorithmFidelity {
    Exact,
    Proxy,
    None,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SavingsEstimate {
    pub status: EstimateStatus,
    pub algorithm: Option<String>,
    fidelity: AlgorithmFidelity,
    confidence: EstimateConfidence,
    pub sampled_bytes: u64,
    pub logical_bytes: u64,
    pub allocated_bytes: u64,
    pub estimated_savings_lower: Option<u64>,
    pub estimated_savings_upper: Option<u64>,
    estimator_version: u32,
    pub detail: String,
}

impl SavingsEstimate {
    fn terminal(
        status: EstimateStatus,
        target: &CompressionTarget,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            status,
            algorithm: None,
            fidelity: AlgorithmFidelity::None,
            confidence: EstimateConfidence::None,
            sampled_bytes: 0,
            logical_bytes: target.logical_bytes,
            allocated_bytes: target.allocated_bytes,
            estimated_savings_lower: None,
            estimated_savings_upper: None,
            estimator_version: ESTIMATOR_VERSION,
            detail: detail.into(),
        }
    }

    pub(crate) fn cancelled(target: &CompressionTarget) -> Self {
        Self::terminal(
            EstimateStatus::Cancelled,
            target,
            "Savings estimation was cancelled before it completed.",
        )
    }
}

#[derive(Clone, Copy)]
struct Codec {
    algorithm: &'static str,
    fidelity: AlgorithmFidelity,
    detail: &'static str,
    compress_len: fn(&[u8]) -> Result<usize, String>,
}

#[derive(Clone, Copy)]
struct SampleResult {
    input_bytes: u64,
    compressed_bytes: u64,
}

struct CodecError {
    status: EstimateStatus,
    detail: String,
}

pub(crate) fn estimate(target: &CompressionTarget, cancel: &AtomicBool) -> SavingsEstimate {
    if !matches!(target.kind, EntryKind::File) {
        return SavingsEstimate::terminal(
            EstimateStatus::NotCandidate,
            target,
            "Only regular files can be sampled for compression savings.",
        );
    }
    if target.logical_bytes == 0 || target.allocated_bytes == 0 {
        return SavingsEstimate::terminal(
            EstimateStatus::NotCandidate,
            target,
            "This entry has no independently accounted allocated data to estimate.",
        );
    }
    if cancel.load(Ordering::Relaxed) {
        return SavingsEstimate::cancelled(target);
    }

    let current_state = inspect(&target.path, target.kind);
    if current_state.state == CompressionStateKind::Compressed {
        return SavingsEstimate::terminal(
            EstimateStatus::NotCandidate,
            target,
            "The filesystem already reports this file as compressed.",
        );
    }
    if matches!(
        current_state.state,
        CompressionStateKind::NotApplicable | CompressionStateKind::Unavailable
    ) {
        return SavingsEstimate::terminal(
            EstimateStatus::Unavailable,
            target,
            current_state.detail,
        );
    }

    let file = match open_no_follow(&target.path) {
        Ok(file) => file,
        Err(error) => {
            return SavingsEstimate::terminal(
                EstimateStatus::Unavailable,
                target,
                format!("The file could not be opened safely for sampling: {error}."),
            );
        }
    };
    let metadata = match file.metadata() {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => {
            return SavingsEstimate::terminal(
                EstimateStatus::Unavailable,
                target,
                "The scanned item is no longer a regular file.",
            );
        }
        Err(error) => {
            return SavingsEstimate::terminal(
                EstimateStatus::Unavailable,
                target,
                format!("The opened file metadata query failed: {error}."),
            );
        }
    };
    if metadata.len() != target.logical_bytes {
        return SavingsEstimate::terminal(
            EstimateStatus::Unavailable,
            target,
            "The file size changed after the scan; rescan before estimating savings.",
        );
    }
    if let Some(current_allocated_bytes) = current_allocated_bytes(&metadata)
        && current_allocated_bytes != target.allocated_bytes
    {
        return SavingsEstimate::terminal(
            EstimateStatus::Unavailable,
            target,
            "The file's allocation changed after the scan; rescan before estimating savings.",
        );
    }

    let codec = match platform_codec(&file) {
        Ok(codec) => codec,
        Err(error) => {
            return SavingsEstimate::terminal(error.status, target, error.detail);
        }
    };
    let ranges = sample_ranges(target.logical_bytes);
    let mut samples = Vec::with_capacity(ranges.len());
    for (offset, length) in ranges {
        if cancel.load(Ordering::Relaxed) {
            return SavingsEstimate::cancelled(target);
        }
        let mut input = vec![0_u8; length];
        if let Err(error) = read_exact_at(&file, &mut input, offset, cancel) {
            if error.kind() == io::ErrorKind::Interrupted && cancel.load(Ordering::Relaxed) {
                return SavingsEstimate::cancelled(target);
            }
            return SavingsEstimate::terminal(
                EstimateStatus::Unavailable,
                target,
                format!("The bounded sample could not be read: {error}."),
            );
        }
        let compressed_bytes = match (codec.compress_len)(&input) {
            Ok(compressed_bytes) => compressed_bytes.min(input.len()),
            Err(error) => {
                return SavingsEstimate::terminal(
                    EstimateStatus::Unavailable,
                    target,
                    format!("The {} sample encoder failed: {error}.", codec.algorithm),
                );
            }
        };
        samples.push(SampleResult {
            input_bytes: input.len() as u64,
            compressed_bytes: compressed_bytes as u64,
        });
    }

    build_estimate(target, codec, &samples)
}

fn build_estimate(
    target: &CompressionTarget,
    codec: Codec,
    samples: &[SampleResult],
) -> SavingsEstimate {
    let sampled_bytes = samples.iter().map(|sample| sample.input_bytes).sum::<u64>();
    let full_coverage = sampled_bytes == target.logical_bytes;
    let sparse = target.allocated_bytes < target.logical_bytes;
    let ratios: Vec<_> = samples
        .iter()
        .map(|sample| sample.compressed_bytes as f64 / sample.input_bytes as f64)
        .collect();
    let observed_low = ratios.iter().copied().fold(1.0_f64, f64::min);
    let observed_high = ratios.iter().copied().fold(0.0_f64, f64::max);
    let (ratio_low, ratio_high) = if full_coverage {
        let compressed = samples
            .iter()
            .map(|sample| sample.compressed_bytes)
            .sum::<u64>();
        let ratio = compressed as f64 / sampled_bytes as f64;
        (ratio, ratio)
    } else {
        (
            (observed_low - 0.08).max(0.0),
            (observed_high + 0.08).min(1.0),
        )
    };
    let compressed_low = round_allocation((target.logical_bytes as f64 * ratio_low) as u64);
    let compressed_high = round_allocation((target.logical_bytes as f64 * ratio_high) as u64);
    let savings_lower = if sparse {
        0
    } else {
        target.allocated_bytes.saturating_sub(compressed_high)
    };
    let savings_upper = target.allocated_bytes.saturating_sub(compressed_low);
    let ratio_spread = observed_high - observed_low;
    let confidence = if sparse {
        EstimateConfidence::Low
    } else if full_coverage && codec.fidelity == AlgorithmFidelity::Exact {
        EstimateConfidence::High
    } else if full_coverage
        || (sampled_bytes as f64 / target.logical_bytes as f64 >= 0.1 && ratio_spread <= 0.1)
    {
        EstimateConfidence::Medium
    } else {
        EstimateConfidence::Low
    };
    let coverage_percent = sampled_bytes as f64 / target.logical_bytes as f64 * 100.0;
    let sparse_note = if sparse {
        " The file is sparse or already uses less allocation than its logical size, so confidence is reduced."
    } else {
        ""
    };

    SavingsEstimate {
        status: EstimateStatus::Estimated,
        algorithm: Some(codec.algorithm.to_string()),
        fidelity: codec.fidelity,
        confidence,
        sampled_bytes,
        logical_bytes: target.logical_bytes,
        allocated_bytes: target.allocated_bytes,
        estimated_savings_lower: Some(savings_lower.min(savings_upper)),
        estimated_savings_upper: Some(savings_lower.max(savings_upper)),
        estimator_version: ESTIMATOR_VERSION,
        detail: format!(
            "{} Sampled {coverage_percent:.1}% of the file in {} bounded range{}. This is an estimate of allocated savings, not a guarantee.{sparse_note}",
            codec.detail,
            samples.len(),
            if samples.len() == 1 { "" } else { "s" },
        ),
    }
}

fn round_allocation(bytes: u64) -> u64 {
    bytes.saturating_add(ALLOCATION_GRANULARITY - 1) / ALLOCATION_GRANULARITY
        * ALLOCATION_GRANULARITY
}

fn sample_ranges(logical_bytes: u64) -> Vec<(u64, usize)> {
    let maximum_sample_bytes = (SAMPLE_RANGE_BYTES * MAX_SAMPLE_RANGES) as u64;
    if logical_bytes <= maximum_sample_bytes {
        let mut ranges = Vec::new();
        let mut offset = 0_u64;
        while offset < logical_bytes {
            let length = (logical_bytes - offset).min(SAMPLE_RANGE_BYTES as u64) as usize;
            ranges.push((offset, length));
            offset += length as u64;
        }
        return ranges;
    }

    let sample = SAMPLE_RANGE_BYTES as u64;
    let align = ALLOCATION_GRANULARITY;
    let middle = ((logical_bytes - sample) / 2 / align) * align;
    let tail = ((logical_bytes - sample) / align) * align;
    vec![
        (0, SAMPLE_RANGE_BYTES),
        (middle, SAMPLE_RANGE_BYTES),
        (tail, SAMPLE_RANGE_BYTES),
    ]
}

fn read_exact_at(
    file: &File,
    buffer: &mut [u8],
    offset: u64,
    cancel: &AtomicBool,
) -> io::Result<()> {
    let mut completed = 0;
    while completed < buffer.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let end = (completed + READ_CHUNK_BYTES).min(buffer.len());
        let read = read_at(file, &mut buffer[completed..end], offset + completed as u64)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the file changed while it was being sampled",
            ));
        }
        completed += read;
    }
    Ok(())
}

#[cfg(unix)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.read_at(buffer, offset)
}

#[cfg(windows)]
fn read_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(buffer, offset)
}

#[cfg(not(any(unix, windows)))]
fn read_at(_file: &File, _buffer: &mut [u8], _offset: u64) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "positioned sampling is not implemented on this platform",
    ))
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(unix)]
fn current_allocated_bytes(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.blocks().saturating_mul(512))
}

#[cfg(not(unix))]
fn current_allocated_bytes(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(windows)]
fn open_no_follow(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    let file = OpenOptions::new()
        .read(true)
        .share_mode(0x0000_0001 | 0x0000_0002 | 0x0000_0004)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is a reparse point",
        ));
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
fn open_no_follow(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

#[cfg(target_os = "macos")]
fn platform_codec(_file: &File) -> Result<Codec, CodecError> {
    Ok(Codec {
        algorithm: "zlib-proxy",
        fidelity: AlgorithmFidelity::Proxy,
        detail: "The macOS writer algorithm is undefined, so Cepa uses zlib only as a clearly labeled compressibility proxy.",
        compress_len: |input| Ok(miniz_oxide::deflate::compress_to_vec_zlib(input, 6).len()),
    })
}

#[cfg(target_os = "linux")]
fn platform_codec(file: &File) -> Result<Codec, CodecError> {
    use std::mem::MaybeUninit;
    use std::os::fd::AsRawFd;

    let mut filesystem = MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: the file descriptor is open and filesystem is writable storage.
    if unsafe { libc::fstatfs(file.as_raw_fd(), filesystem.as_mut_ptr()) } != 0 {
        return Err(CodecError {
            status: EstimateStatus::Unavailable,
            detail: format!(
                "The sampled file's filesystem query failed: {}.",
                io::Error::last_os_error()
            ),
        });
    }
    // SAFETY: fstatfs succeeded and initialized the output structure.
    if unsafe { filesystem.assume_init() }.f_type != libc::BTRFS_SUPER_MAGIC {
        return Err(CodecError {
            status: EstimateStatus::Unsupported,
            detail: "Savings estimation is currently implemented only for Btrfs on Linux.".into(),
        });
    }
    Ok(Codec {
        algorithm: "zstd",
        fidelity: AlgorithmFidelity::Exact,
        detail: "Samples use the Btrfs-supported Zstd algorithm at level 3.",
        compress_len: |input| {
            input.chunks(128 * 1024).try_fold(0_usize, |total, chunk| {
                zstd::bulk::compress(chunk, 3)
                    .map(|output| total.saturating_add(output.len().min(chunk.len())))
                    .map_err(|error| error.to_string())
            })
        },
    })
}

#[cfg(windows)]
fn platform_codec(_file: &File) -> Result<Codec, CodecError> {
    Ok(Codec {
        algorithm: "lznt1",
        fidelity: AlgorithmFidelity::Exact,
        detail: "Samples use the same 4 KiB-chunked LZNT1 format as NTFS compression.",
        compress_len: |input| {
            let mut output = Vec::new();
            lznt1::compress(input, &mut output);
            Ok(output.len())
        },
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn platform_codec(_file: &File) -> Result<Codec, CodecError> {
    Err(CodecError {
        status: EstimateStatus::Unsupported,
        detail: "Savings estimation is not implemented on this platform.".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(path: &Path, logical_bytes: u64, allocated_bytes: u64) -> CompressionTarget {
        CompressionTarget {
            path: path.to_path_buf(),
            kind: EntryKind::File,
            logical_bytes,
            allocated_bytes,
        }
    }

    #[test]
    fn bounds_large_files_to_three_non_overlapping_ranges() {
        let size = 64 * 1024 * 1024;
        let ranges = sample_ranges(size);

        assert_eq!(ranges.len(), MAX_SAMPLE_RANGES);
        assert_eq!(ranges[0], (0, SAMPLE_RANGE_BYTES));
        assert!(ranges.windows(2).all(|pair| {
            pair[0].0 + pair[0].1 as u64 <= pair[1].0 && pair[1].0 % ALLOCATION_GRANULARITY == 0
        }));
        assert!(ranges.last().expect("tail range").0 + SAMPLE_RANGE_BYTES as u64 <= size);
    }

    #[test]
    fn fully_samples_small_files_without_overlapping_ranges() {
        let size = SAMPLE_RANGE_BYTES as u64 + 17;
        let ranges = sample_ranges(size);

        assert_eq!(
            ranges,
            vec![(0, SAMPLE_RANGE_BYTES), (SAMPLE_RANGE_BYTES as u64, 17)]
        );
        assert_eq!(
            ranges.iter().map(|(_, length)| *length as u64).sum::<u64>(),
            size
        );
    }

    #[test]
    fn cancellation_returns_without_opening_the_file() {
        let cancel = AtomicBool::new(true);
        let estimate = estimate(&target(Path::new("not-opened"), 1, 1), &cancel);

        assert_eq!(estimate.status, EstimateStatus::Cancelled);
        assert_eq!(estimate.sampled_bytes, 0);
    }

    #[test]
    fn rejects_empty_or_deduplicated_entries_without_io() {
        let cancel = AtomicBool::new(false);
        let estimate = estimate(&target(Path::new("not-opened"), 0, 0), &cancel);

        assert_eq!(estimate.status, EstimateStatus::NotCandidate);
    }

    #[test]
    fn sparse_estimates_never_claim_a_minimum_savings() {
        let codec = Codec {
            algorithm: "test",
            fidelity: AlgorithmFidelity::Exact,
            detail: "test codec",
            compress_len: |_| Ok(0),
        };
        let estimate = build_estimate(
            &target(Path::new("not-opened"), 1024 * 1024, 64 * 1024),
            codec,
            &[SampleResult {
                input_bytes: 1024 * 1024,
                compressed_bytes: 4 * 1024,
            }],
        );

        assert_eq!(estimate.status, EstimateStatus::Estimated);
        assert_eq!(estimate.confidence, EstimateConfidence::Low);
        assert_eq!(estimate.estimated_savings_lower, Some(0));
        assert!(estimate.estimated_savings_upper.expect("upper bound") <= 64 * 1024);
    }

    #[test]
    fn estimate_wire_contract_includes_method_and_bounds() {
        let codec = Codec {
            algorithm: "test",
            fidelity: AlgorithmFidelity::Exact,
            detail: "test codec",
            compress_len: |_| Ok(0),
        };
        let estimate = build_estimate(
            &target(Path::new("not-opened"), 4096, 4096),
            codec,
            &[SampleResult {
                input_bytes: 4096,
                compressed_bytes: 1024,
            }],
        );
        let wire = serde_json::to_value(&estimate).expect("serialize estimate");

        assert_eq!(wire["status"], "estimated");
        assert_eq!(wire["algorithm"], "test");
        assert_eq!(wire["fidelity"], "exact");
        assert_eq!(wire["confidence"], "high");
        assert_eq!(wire["sampledBytes"], 4096);
        assert_eq!(wire["estimatorVersion"], ESTIMATOR_VERSION);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn estimates_a_bounded_range_for_compressible_and_random_files() {
        let temp = tempfile::tempdir().expect("create estimate fixture");
        let compressible_path = temp.path().join("compressible.bin");
        let random_path = temp.path().join("random.bin");
        let logical_bytes = 2 * 1024 * 1024_u64;
        std::fs::write(&compressible_path, vec![0_u8; logical_bytes as usize])
            .expect("write compressible fixture");
        let mut random_state = 0x4d59_5df4_d0f3_3173_u64;
        let random: Vec<_> = (0..logical_bytes)
            .map(|_| {
                random_state ^= random_state << 13;
                random_state ^= random_state >> 7;
                random_state ^= random_state << 17;
                (random_state >> 24) as u8
            })
            .collect();
        std::fs::write(&random_path, random).expect("write random fixture");
        let cancel = AtomicBool::new(false);

        let compressible = estimate(
            &target(&compressible_path, logical_bytes, logical_bytes),
            &cancel,
        );
        let random = estimate(&target(&random_path, logical_bytes, logical_bytes), &cancel);

        assert_eq!(compressible.status, EstimateStatus::Estimated);
        assert_eq!(compressible.sampled_bytes, (SAMPLE_RANGE_BYTES * 3) as u64);
        assert!(compressible.estimated_savings_lower.expect("lower bound") > logical_bytes / 2);
        assert_eq!(random.status, EstimateStatus::Estimated);
        assert!(random.estimated_savings_upper.expect("upper bound") < logical_bytes / 4);
    }

    #[cfg(windows)]
    #[test]
    fn estimates_compressible_data_with_the_exact_lznt1_codec() {
        let temp = tempfile::tempdir().expect("create LZNT1 estimate fixture");
        let path = temp.path().join("compressible.bin");
        let logical_bytes = 2 * 1024 * 1024_u64;
        std::fs::write(&path, vec![0_u8; logical_bytes as usize])
            .expect("write compressible fixture");

        let estimate = estimate(
            &target(&path, logical_bytes, logical_bytes),
            &AtomicBool::new(false),
        );

        assert_eq!(estimate.status, EstimateStatus::Estimated);
        assert_eq!(estimate.fidelity, AlgorithmFidelity::Exact);
        assert_eq!(estimate.algorithm.as_deref(), Some("lznt1"));
        assert!(estimate.estimated_savings_lower.expect("lower bound") > logical_bytes / 2);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn returns_a_first_class_result_on_the_native_test_filesystem() {
        use std::os::unix::fs::MetadataExt;

        let temp = tempfile::tempdir().expect("create Linux estimate fixture");
        let path = temp.path().join("sample.bin");
        std::fs::write(&path, vec![0_u8; 1024 * 1024]).expect("write estimate fixture");
        let metadata = path.metadata().expect("read fixture metadata");
        let estimate = estimate(
            &target(&path, metadata.len(), metadata.blocks().saturating_mul(512)),
            &AtomicBool::new(false),
        );

        assert!(matches!(
            estimate.status,
            EstimateStatus::Estimated | EstimateStatus::Unsupported
        ));
    }
}
