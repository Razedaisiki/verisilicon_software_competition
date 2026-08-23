//! Testable command-line routing and selected quality processing.

use crate::algorithm::{
    ExecutionPolicy, MAX_CHANNEL_WORKERS, SelectedQualityPipeline, SuperResolution,
};
use crate::image::Image;
use crate::io::ppm::PpmP6Codec;
use crate::io::raw::RawRgb8Codec;
use crate::io::{DecodeSpec, ImageDecoder, ImageEncoder, ImageFormat};
use crate::spec::{
    Dimensions, OFFICIAL_RAW_INPUT_BYTE_COUNT, ProcessingConfig, official_raw_input_dimensions,
};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_PROCESSING: u8 = 4;

/// Caps official-geometry frame concurrency to a conservative working set.
///
/// A selected-pipeline frame plus encode buffers can retain roughly 60 MiB at
/// 1920x1080 input and 3840x2160 output. Eight workers therefore keep the
/// expected batch working set within approximately 512 MiB.
const MAX_BATCH_FRAME_WORKERS: usize = 8;

pub const USAGE: &str = "Usage:\n  sr <input> <output>\n  sr --raw-rgb8 <width> <height> <input.raw> <output.raw>\n  sr --batch <in_dir> <out_dir>\n  sr --help";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Help,
    Single {
        input: PathBuf,
        output: PathBuf,
    },
    RawRgb8 {
        dimensions: Dimensions,
        input: PathBuf,
        output: PathBuf,
    },
    Batch {
        input_dir: PathBuf,
        output_dir: PathBuf,
    },
}

/// Runs the CLI with injectable output streams and returns a stable exit code.
pub fn run<I, W, E>(args: I, stdout: &mut W, stderr: &mut E) -> u8
where
    I: IntoIterator<Item = OsString>,
    W: Write,
    E: Write,
{
    let command = match parse_args(args) {
        Ok(command) => command,
        Err(message) => {
            let _ = writeln!(stderr, "Error: {message}\n{USAGE}");
            return EXIT_USAGE;
        }
    };

    match command {
        Command::Help => {
            let _ = writeln!(stdout, "{USAGE}");
            EXIT_SUCCESS
        }
        Command::Batch {
            input_dir,
            output_dir,
        } => match process_batch(&input_dir, &output_dir) {
            Ok(report) => {
                let _processing_time = report.processing_time;
                for failure in &report.failures {
                    let _ = writeln!(stderr, "Error: {failure}");
                }
                if report.succeeded > 0 && report.failures.is_empty() {
                    EXIT_SUCCESS
                } else {
                    EXIT_PROCESSING
                }
            }
            Err(error) => {
                let _ = writeln!(stderr, "Error: {error}");
                EXIT_PROCESSING
            }
        },
        Command::Single { input, output } => match process_single(&input, &output) {
            Ok(_processing_time) => EXIT_SUCCESS,
            Err(error) => {
                let _ = writeln!(stderr, "Error: {error}");
                EXIT_PROCESSING
            }
        },
        Command::RawRgb8 {
            dimensions,
            input,
            output,
        } => match process_raw_rgb8(dimensions, &input, &output) {
            Ok(_processing_time) => EXIT_SUCCESS,
            Err(error) => {
                let _ = writeln!(stderr, "Error: {error}");
                EXIT_PROCESSING
            }
        },
    }
}

fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = OsString>,
{
    let args: Vec<OsString> = args.into_iter().collect();
    match args.as_slice() {
        [flag] if flag == OsStr::new("--help") || flag == OsStr::new("-h") => Ok(Command::Help),
        [input, output] if input != OsStr::new("--batch") && input != OsStr::new("--raw-rgb8") => {
            Ok(Command::Single {
                input: PathBuf::from(input),
                output: PathBuf::from(output),
            })
        }
        [flag, input_dir, output_dir] if flag == OsStr::new("--batch") => Ok(Command::Batch {
            input_dir: PathBuf::from(input_dir),
            output_dir: PathBuf::from(output_dir),
        }),
        [flag, width, height, input, output] if flag == OsStr::new("--raw-rgb8") => {
            let width = parse_dimension(width, "width")?;
            let height = parse_dimension(height, "height")?;
            let dimensions = Dimensions::new(width, height).map_err(|error| error.to_string())?;
            Ok(Command::RawRgb8 {
                dimensions,
                input: PathBuf::from(input),
                output: PathBuf::from(output),
            })
        }
        [] => Err("missing command arguments".to_owned()),
        _ => Err("invalid command arguments".to_owned()),
    }
}

fn parse_dimension(value: &OsStr, name: &str) -> Result<u32, String> {
    let text = value
        .to_str()
        .ok_or_else(|| format!("{name} must be an ASCII decimal integer"))?;
    text.parse::<u32>()
        .map_err(|_| format!("{name} must be an unsigned decimal integer"))
}

fn process_ppm(input: &Path, output: &Path) -> Result<Duration, CliError> {
    process_ppm_with_policy(input, output, ExecutionPolicy::Auto)
}

fn process_ppm_with_policy(
    input: &Path,
    output: &Path,
    policy: ExecutionPolicy,
) -> Result<Duration, CliError> {
    let codec = PpmP6Codec::new();
    let image = codec.decode(
        input,
        DecodeSpec {
            format: ImageFormat::PpmP6,
            dimensions: None,
        },
    )?;
    let timed = process_selected_quality_with_policy(&image, policy)?;
    codec.encode(output, ImageFormat::PpmP6, &timed.image)?;
    Ok(timed.processing_time)
}

fn process_single(input: &Path, output: &Path) -> Result<Duration, CliError> {
    match select_single_format(input)? {
        ImageFormat::PpmP6 => process_ppm(input, output),
        ImageFormat::RawRgb8 => process_raw_rgb8(official_raw_input_dimensions(), input, output),
    }
}

fn select_single_format(input: &Path) -> Result<ImageFormat, CliError> {
    if let Some(format) = format_from_extension(input) {
        return Ok(format);
    }
    let data = fs::read(input).map_err(|error| {
        CliError::File(format!("failed to read input {}: {error}", input.display()))
    })?;
    detect_format_bytes(&data)
}

fn detect_format_bytes(data: &[u8]) -> Result<ImageFormat, CliError> {
    let valid_ppm = PpmP6Codec::new().decode_bytes(data).is_ok();
    let valid_raw_length = data.len() == OFFICIAL_RAW_INPUT_BYTE_COUNT;
    match (valid_ppm, valid_raw_length) {
        (true, false) => Ok(ImageFormat::PpmP6),
        (false, true) => Ok(ImageFormat::RawRgb8),
        (true, true) => Err(CliError::AmbiguousFormat),
        (false, false) => Err(CliError::UndetectedFormat { actual: data.len() }),
    }
}

fn format_from_extension(path: &Path) -> Option<ImageFormat> {
    let extension = path.extension().and_then(OsStr::to_str)?;
    if extension.eq_ignore_ascii_case("ppm") {
        Some(ImageFormat::PpmP6)
    } else if extension.eq_ignore_ascii_case("raw") || extension.eq_ignore_ascii_case("rgb") {
        Some(ImageFormat::RawRgb8)
    } else {
        None
    }
}

fn process_raw_rgb8(
    dimensions: Dimensions,
    input: &Path,
    output: &Path,
) -> Result<Duration, CliError> {
    process_raw_rgb8_with_policy(dimensions, input, output, ExecutionPolicy::Auto)
}

fn process_raw_rgb8_with_policy(
    dimensions: Dimensions,
    input: &Path,
    output: &Path,
    policy: ExecutionPolicy,
) -> Result<Duration, CliError> {
    let codec = RawRgb8Codec::new();
    let image = codec.decode(
        input,
        DecodeSpec {
            format: ImageFormat::RawRgb8,
            dimensions: Some(dimensions),
        },
    )?;
    let timed = process_selected_quality_with_policy(&image, policy)?;
    codec.encode(output, ImageFormat::RawRgb8, &timed.image)?;
    Ok(timed.processing_time)
}

fn process_selected_quality_with_policy(
    image: &Image,
    policy: ExecutionPolicy,
) -> Result<TimedImage, CliError> {
    let config = ProcessingConfig::new(image.dimensions());
    let start = Instant::now();
    let output = match policy {
        ExecutionPolicy::Auto => SelectedQualityPipeline::new().process(image, config)?,
        forced => SelectedQualityPipeline::new().process_with_policy(image, config, forced)?,
    };
    let processing_time = start.elapsed();
    Ok(TimedImage {
        image: output,
        processing_time,
    })
}

fn process_batch(input_dir: &Path, output_dir: &Path) -> Result<BatchReport, CliError> {
    let candidates = discover_batch_candidates(input_dir)?;
    if candidates.is_empty() {
        return Err(CliError::NoCandidates(input_dir.to_path_buf()));
    }

    fs::create_dir_all(output_dir).map_err(|error| {
        CliError::File(format!(
            "failed to create output directory {}: {error}",
            output_dir.display()
        ))
    })?;

    let available_processors = thread::available_parallelism().map_or(1, usize::from);
    let mut outcomes = Vec::new();
    outcomes
        .try_reserve_exact(candidates.len())
        .map_err(|_| CliError::BatchTaskAllocationFailed)?;
    outcomes.resize_with(candidates.len(), || None);
    let mut tasks = Vec::new();
    tasks
        .try_reserve_exact(candidates.len())
        .map_err(|_| CliError::BatchTaskAllocationFailed)?;

    for (index, (input, format)) in candidates.iter().enumerate() {
        let file_name = input
            .file_name()
            .ok_or_else(|| CliError::File("batch candidate has no filename".to_owned()))?;
        let output = output_dir.join(file_name);
        match output.try_exists() {
            Ok(true) => {
                outcomes[index] = Some(BatchOutcome::Failure(format!(
                    "{}: refusing to overwrite existing output {}",
                    input.display(),
                    output.display()
                )));
                continue;
            }
            Ok(false) => {}
            Err(error) => {
                outcomes[index] = Some(BatchOutcome::Failure(format!(
                    "{}: failed to inspect output {}: {error}",
                    input.display(),
                    output.display()
                )));
                continue;
            }
        }

        tasks.push(BatchTask {
            candidate_index: index,
            input: input.clone(),
            output,
            format: *format,
        });
    }

    if !tasks.is_empty() {
        let plan = batch_execution_plan_for_tasks(available_processors, &tasks);
        for result in execute_batch_tasks(tasks, plan)? {
            let candidate_index = result.candidate_index;
            outcomes[candidate_index] = Some(match result.result {
                Ok(processing_time) => BatchOutcome::Success(processing_time),
                Err(error) => BatchOutcome::Failure(format!(
                    "{}: {error}",
                    candidates[candidate_index].0.display()
                )),
            });
        }
    }

    let mut report = BatchReport::default();
    for outcome in outcomes {
        match outcome.ok_or(CliError::MissingBatchResult)? {
            BatchOutcome::Success(processing_time) => {
                report.succeeded += 1;
                report.processing_time = report
                    .processing_time
                    .checked_add(processing_time)
                    .ok_or(CliError::TimingOverflow)?;
            }
            BatchOutcome::Failure(error) => report.failures.push(error),
        }
    }
    Ok(report)
}

fn batch_execution_plan_for_tasks(
    available_processors: usize,
    tasks: &[BatchTask],
) -> BatchExecutionPlan {
    batch_execution_plan(available_processors, tasks.len())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BatchExecutionPlan {
    frame_workers: usize,
    inner_policy: ExecutionPolicy,
}

fn batch_execution_plan(available_processors: usize, candidate_count: usize) -> BatchExecutionPlan {
    let available_processors = available_processors.max(1);
    if candidate_count == 1 {
        return BatchExecutionPlan {
            frame_workers: 1,
            inner_policy: ExecutionPolicy::Auto,
        };
    }
    let all_frames_fit_channel_workers = candidate_count > 0
        && candidate_count
            .checked_mul(MAX_CHANNEL_WORKERS)
            .is_some_and(|workers| workers <= available_processors);

    if all_frames_fit_channel_workers {
        BatchExecutionPlan {
            frame_workers: candidate_count,
            inner_policy: ExecutionPolicy::Parallel,
        }
    } else {
        BatchExecutionPlan {
            frame_workers: candidate_count
                .min(available_processors)
                .min(MAX_BATCH_FRAME_WORKERS),
            inner_policy: ExecutionPolicy::Serial,
        }
    }
}

struct BatchTask {
    candidate_index: usize,
    input: PathBuf,
    output: PathBuf,
    format: ImageFormat,
}

struct BatchTaskResult {
    candidate_index: usize,
    result: Result<Duration, CliError>,
}

enum BatchOutcome {
    Success(Duration),
    Failure(String),
}

fn execute_batch_tasks(
    tasks: Vec<BatchTask>,
    plan: BatchExecutionPlan,
) -> Result<Vec<BatchTaskResult>, CliError> {
    debug_assert!(!tasks.is_empty());
    debug_assert!(plan.frame_workers > 0);

    let task_count = tasks.len();
    let worker_count = plan.frame_workers.min(task_count);
    let mut results = Vec::new();
    results
        .try_reserve_exact(task_count)
        .map_err(|_| CliError::BatchTaskAllocationFailed)?;
    let (task_sender, task_receiver) = mpsc::channel::<BatchTask>();
    let (result_sender, result_receiver) = mpsc::channel::<BatchTaskResult>();
    let task_receiver = Arc::new(Mutex::new(task_receiver));
    let mut workers = Vec::new();
    workers
        .try_reserve_exact(worker_count)
        .map_err(|_| CliError::BatchTaskAllocationFailed)?;

    for worker_index in 0..worker_count {
        let task_receiver = Arc::clone(&task_receiver);
        let worker_result_sender = result_sender.clone();
        let spawn = thread::Builder::new()
            .name(format!("sr-batch-frame-{worker_index}"))
            .spawn(move || {
                batch_worker_loop(&task_receiver, &worker_result_sender, plan.inner_policy);
            });
        match spawn {
            Ok(worker) => workers.push(worker),
            Err(_) => {
                drop(task_sender);
                drop(result_sender);
                let _ = join_batch_workers(workers);
                return Err(CliError::BatchWorkerSpawnFailed);
            }
        }
    }
    drop(result_sender);

    let mut send_failed = false;
    for task in tasks {
        if task_sender.send(task).is_err() {
            send_failed = true;
            break;
        }
    }
    drop(task_sender);

    while results.len() < task_count {
        match result_receiver.recv() {
            Ok(result) => results.push(result),
            Err(_) => break,
        }
    }

    join_batch_workers(workers)?;
    if send_failed || results.len() != task_count {
        return Err(CliError::BatchWorkerDisconnected);
    }
    Ok(results)
}

fn batch_worker_loop(
    task_receiver: &Mutex<mpsc::Receiver<BatchTask>>,
    result_sender: &mpsc::Sender<BatchTaskResult>,
    inner_policy: ExecutionPolicy,
) {
    loop {
        let task = match task_receiver.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let Ok(task) = task else {
            return;
        };

        let result = match task.format {
            ImageFormat::PpmP6 => process_ppm_with_policy(&task.input, &task.output, inner_policy),
            ImageFormat::RawRgb8 => process_raw_rgb8_with_policy(
                official_raw_input_dimensions(),
                &task.input,
                &task.output,
                inner_policy,
            ),
        };
        if result_sender
            .send(BatchTaskResult {
                candidate_index: task.candidate_index,
                result,
            })
            .is_err()
        {
            return;
        }
    }
}

fn join_batch_workers(workers: Vec<JoinHandle<()>>) -> Result<(), CliError> {
    let mut panicked = false;
    for worker in workers {
        if worker.join().is_err() {
            panicked = true;
        }
    }
    if panicked {
        Err(CliError::BatchWorkerPanicked)
    } else {
        Ok(())
    }
}

fn discover_batch_candidates(input_dir: &Path) -> Result<Vec<(PathBuf, ImageFormat)>, CliError> {
    let entries = fs::read_dir(input_dir).map_err(|error| {
        CliError::File(format!(
            "failed to read input directory {}: {error}",
            input_dir.display()
        ))
    })?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            CliError::File(format!(
                "failed to read an entry in {}: {error}",
                input_dir.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            CliError::File(format!(
                "failed to inspect {}: {error}",
                entry.path().display()
            ))
        })?;
        if file_type.is_file() {
            if let Some(format) = format_from_extension(&entry.path()) {
                candidates.push((entry.path(), format));
            }
        }
    }
    candidates.sort_by(|left, right| left.0.file_name().cmp(&right.0.file_name()));
    Ok(candidates)
}

struct TimedImage {
    image: Image,
    processing_time: Duration,
}

#[derive(Default)]
struct BatchReport {
    succeeded: usize,
    failures: Vec<String>,
    processing_time: Duration,
}

#[derive(Debug)]
enum CliError {
    ImageIo(crate::io::ImageIoError),
    Algorithm(crate::algorithm::AlgorithmError),
    File(String),
    NoCandidates(PathBuf),
    AmbiguousFormat,
    UndetectedFormat { actual: usize },
    BatchTaskAllocationFailed,
    BatchWorkerSpawnFailed,
    BatchWorkerPanicked,
    BatchWorkerDisconnected,
    MissingBatchResult,
    TimingOverflow,
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageIo(error) => error.fmt(formatter),
            Self::Algorithm(error) => error.fmt(formatter),
            Self::File(message) => formatter.write_str(message),
            Self::NoCandidates(path) => {
                write!(formatter, "no supported image candidates found in {}", path.display())
            }
            Self::AmbiguousFormat => formatter.write_str(
                "ambiguous input format: valid PPM P6 also has the official raw byte length; use a .ppm, .raw, or .rgb extension",
            ),
            Self::UndetectedFormat { actual } => write!(
                formatter,
                "unable to detect input format: input is neither valid PPM P6 nor {OFFICIAL_RAW_INPUT_BYTE_COUNT}-byte official raw RGB888 (received {actual} bytes)"
            ),
            Self::BatchTaskAllocationFailed => {
                formatter.write_str("failed to allocate batch scheduling state")
            }
            Self::BatchWorkerSpawnFailed => formatter.write_str("failed to spawn batch worker"),
            Self::BatchWorkerPanicked => formatter.write_str("batch worker panicked"),
            Self::BatchWorkerDisconnected => {
                formatter.write_str("batch worker disconnected before completing every candidate")
            }
            Self::MissingBatchResult => formatter.write_str("missing ordered batch result"),
            Self::TimingOverflow => formatter.write_str("aggregate processing time overflow"),
        }
    }
}

impl From<crate::io::ImageIoError> for CliError {
    fn from(error: crate::io::ImageIoError) -> Self {
        Self::ImageIo(error)
    }
}

impl From<crate::algorithm::AlgorithmError> for CliError {
    fn from(error: crate::algorithm::AlgorithmError) -> Self {
        Self::Algorithm(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BatchExecutionPlan, BatchTask, CliError, EXIT_PROCESSING, EXIT_SUCCESS, EXIT_USAGE,
        MAX_BATCH_FRAME_WORKERS, batch_execution_plan, batch_execution_plan_for_tasks,
        detect_format_bytes, discover_batch_candidates, format_from_extension, join_batch_workers,
        run,
    };
    use crate::algorithm::ExecutionPolicy;
    use crate::io::ImageFormat;
    use crate::io::ppm::PpmP6Codec;
    use crate::spec::{OFFICIAL_RAW_INPUT_BYTE_COUNT, OFFICIAL_RAW_OUTPUT_BYTE_COUNT};
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::thread;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temporary_path(extension: &str) -> PathBuf {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "verisilicon_sr_cli_{}_{}.{}",
            std::process::id(),
            sequence,
            extension
        ))
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "verisilicon_sr_batch_{}_{}_{}",
            std::process::id(),
            sequence,
            label
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn write_ppm(path: &std::path::Path, rgb: [u8; 3]) {
        let mut data = b"P6\n1 1\n255\n".to_vec();
        data.extend_from_slice(&rgb);
        fs::write(path, data).unwrap();
    }

    fn invoke(args: &[OsString]) -> (u8, Vec<u8>, Vec<u8>) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let status = run(args.to_vec(), &mut stdout, &mut stderr);
        (status, stdout, stderr)
    }

    #[test]
    fn ppm_command_scales_end_to_end() {
        let input = temporary_path("ppm");
        let output = temporary_path("raw");
        fs::write(&input, b"P6\n1 1\n255\n\x10\x20\x30").unwrap();
        let (status, _, stderr) =
            invoke(&[input.as_os_str().to_owned(), output.as_os_str().to_owned()]);
        assert_eq!(status, EXIT_SUCCESS, "{}", String::from_utf8_lossy(&stderr));
        let decoded = PpmP6Codec::new()
            .decode_bytes(&fs::read(&output).unwrap())
            .unwrap();
        assert_eq!(decoded.dimensions().width(), 2);
        assert_eq!(decoded.dimensions().height(), 2);
        assert_eq!(decoded.pixels().len(), 4);
        fs::remove_file(input).unwrap();
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn extension_selection_is_case_insensitive_and_input_locked() {
        assert_eq!(
            format_from_extension(std::path::Path::new("frame.PPM")),
            Some(ImageFormat::PpmP6)
        );
        assert_eq!(
            format_from_extension(std::path::Path::new("frame.RAW")),
            Some(ImageFormat::RawRgb8)
        );
        assert_eq!(
            format_from_extension(std::path::Path::new("frame.RgB")),
            Some(ImageFormat::RawRgb8)
        );
        assert_eq!(
            format_from_extension(std::path::Path::new("frame.bin")),
            None
        );
    }

    #[test]
    fn unknown_extension_detection_distinguishes_valid_ppm_and_exact_raw() {
        assert_eq!(
            detect_format_bytes(b"P6\n1 1\n255\n\x01\x02\x03").unwrap(),
            ImageFormat::PpmP6
        );
        let mut raw = vec![0_u8; OFFICIAL_RAW_INPUT_BYTE_COUNT];
        raw[..2].copy_from_slice(b"P6");
        assert_eq!(detect_format_bytes(&raw).unwrap(), ImageFormat::RawRgb8);
        assert!(detect_format_bytes(b"not an image").is_err());

        let suffix = b"\n1 1\n255\n\x01\x02\x03";
        let mut ambiguous = b"P6\n#".to_vec();
        ambiguous.resize(OFFICIAL_RAW_INPUT_BYTE_COUNT - suffix.len(), b'a');
        ambiguous.extend_from_slice(suffix);
        assert_eq!(
            detect_format_bytes(&ambiguous).unwrap_err().to_string(),
            "ambiguous input format: valid PPM P6 also has the official raw byte length; use a .ppm, .raw, or .rgb extension"
        );
    }

    #[test]
    fn locked_extensions_report_format_specific_failures() {
        let malformed_ppm = temporary_path("PPM");
        let short_raw = temporary_path("RAW");
        let output = temporary_path("out");
        fs::write(&malformed_ppm, b"not ppm").unwrap();
        fs::write(&short_raw, b"P6").unwrap();
        let ppm_result = invoke(&[
            malformed_ppm.as_os_str().to_owned(),
            output.as_os_str().to_owned(),
        ]);
        assert_eq!(ppm_result.0, EXIT_PROCESSING);
        assert!(String::from_utf8_lossy(&ppm_result.2).contains("PPM P6 error"));
        let raw_result = invoke(&[
            short_raw.as_os_str().to_owned(),
            output.as_os_str().to_owned(),
        ]);
        assert_eq!(raw_result.0, EXIT_PROCESSING);
        assert!(String::from_utf8_lossy(&raw_result.2).contains("expected 6220800 bytes"));
        fs::remove_file(malformed_ppm).unwrap();
        fs::remove_file(short_raw).unwrap();
    }

    #[test]
    fn official_raw_two_argument_path_is_exact_and_deterministic() {
        let input = temporary_path("RAW");
        let output = temporary_path("ppm");
        let mut source = vec![0_u8; OFFICIAL_RAW_INPUT_BYTE_COUNT];
        source[..3].copy_from_slice(&[b'P', b'6', 0]);
        fs::write(&input, source).unwrap();
        let (status, _, stderr) =
            invoke(&[input.as_os_str().to_owned(), output.as_os_str().to_owned()]);
        assert_eq!(status, EXIT_SUCCESS, "{}", String::from_utf8_lossy(&stderr));
        let encoded = fs::read(&output).unwrap();
        assert_eq!(encoded.len(), OFFICIAL_RAW_OUTPUT_BYTE_COUNT);
        assert!(!encoded.starts_with(b"P6\n"));
        assert_eq!(fnv1a64(&encoded), 12_454_278_094_118_301_282);
        fs::remove_file(input).unwrap();
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn raw_command_scales_to_exact_packed_byte_count() {
        let input = temporary_path("raw");
        let output = temporary_path("raw");
        fs::write(&input, [0, 0, 0, 255, 255, 255]).unwrap();
        let (status, _, stderr) = invoke(&[
            OsString::from("--raw-rgb8"),
            OsString::from("2"),
            OsString::from("1"),
            input.as_os_str().to_owned(),
            output.as_os_str().to_owned(),
        ]);
        assert_eq!(status, EXIT_SUCCESS, "{}", String::from_utf8_lossy(&stderr));
        assert_eq!(fs::read(&output).unwrap().len(), 4 * 2 * 3);
        fs::remove_file(input).unwrap();
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn malformed_inputs_and_dimensions_return_stable_statuses() {
        let input = temporary_path("ppm");
        let output = temporary_path("ppm");
        fs::write(&input, b"not ppm").unwrap();
        assert_eq!(
            invoke(&[input.as_os_str().to_owned(), output.as_os_str().to_owned()]).0,
            EXIT_PROCESSING
        );
        assert_eq!(
            invoke(&[
                OsString::from("--raw-rgb8"),
                OsString::from("0"),
                OsString::from("1"),
                input.as_os_str().to_owned(),
                output.as_os_str().to_owned(),
            ])
            .0,
            EXIT_USAGE
        );
        fs::remove_file(input).unwrap();
    }

    #[test]
    fn help_and_usage_exit_codes_are_stable() {
        assert_eq!(invoke(&[OsString::from("--help")]).0, EXIT_SUCCESS);
        assert_eq!(invoke(&[]).0, EXIT_USAGE);
    }

    #[test]
    fn batch_plan_preserves_one_candidate_auto_policy() {
        for available in [1, 2, 4, 8, 12] {
            assert_eq!(
                batch_execution_plan(available, 1),
                BatchExecutionPlan {
                    frame_workers: 1,
                    inner_policy: ExecutionPolicy::Auto,
                }
            );
        }
    }

    #[test]
    fn batch_plan_uses_runnable_tasks_after_preflight_skips() {
        let discovered_candidate_count = 20;
        let runnable_tasks = vec![BatchTask {
            candidate_index: 19,
            input: PathBuf::from("remaining.ppm"),
            output: PathBuf::from("output/remaining.ppm"),
            format: ImageFormat::PpmP6,
        }];

        assert_eq!(
            batch_execution_plan_for_tasks(12, &runnable_tasks),
            BatchExecutionPlan {
                frame_workers: 1,
                inner_policy: ExecutionPolicy::Auto,
            }
        );
        assert_eq!(
            batch_execution_plan(12, discovered_candidate_count),
            BatchExecutionPlan {
                frame_workers: MAX_BATCH_FRAME_WORKERS,
                inner_policy: ExecutionPolicy::Serial,
            }
        );
    }

    #[test]
    fn batch_plan_uses_small_parallel_and_large_serial_modes() {
        let small_cases = [
            (2, 2, 2, ExecutionPolicy::Serial),
            (4, 2, 2, ExecutionPolicy::Serial),
            (8, 2, 2, ExecutionPolicy::Parallel),
            (8, 3, 3, ExecutionPolicy::Serial),
            (12, 4, 4, ExecutionPolicy::Parallel),
        ];
        for (available, candidates, frame_workers, inner_policy) in small_cases {
            assert_eq!(
                batch_execution_plan(available, candidates),
                BatchExecutionPlan {
                    frame_workers,
                    inner_policy,
                }
            );
        }

        for (available, expected_workers) in [(1, 1), (2, 2), (4, 4), (8, 8), (12, 8)] {
            assert_eq!(
                batch_execution_plan(available, 20),
                BatchExecutionPlan {
                    frame_workers: expected_workers,
                    inner_policy: ExecutionPolicy::Serial,
                }
            );
        }
        assert_eq!(MAX_BATCH_FRAME_WORKERS, 8);
    }

    #[test]
    fn batch_worker_join_reports_panic_after_joining_every_worker() {
        let joined_effect = Arc::new(AtomicUsize::new(0));
        let effect = Arc::clone(&joined_effect);
        let workers = vec![
            thread::spawn(|| panic!("synthetic batch worker panic")),
            thread::spawn(move || {
                effect.fetch_add(1, Ordering::Relaxed);
            }),
        ];
        assert_eq!(
            join_batch_workers(workers).unwrap_err().to_string(),
            "batch worker panicked"
        );
        assert_eq!(joined_effect.load(Ordering::Relaxed), 1);
        assert_eq!(
            CliError::BatchWorkerSpawnFailed.to_string(),
            "failed to spawn batch worker"
        );
    }

    #[test]
    fn batch_sorts_candidates_skips_unrelated_files_and_is_deterministic() {
        let input = temporary_directory("sorted_input");
        let output_one = input.with_extension("output_one");
        let output_two = input.with_extension("output_two");
        write_ppm(&input.join("b.ppm"), [40, 50, 60]);
        write_ppm(&input.join("A.PPM"), [10, 20, 30]);
        fs::write(input.join("notes.txt"), b"ignored").unwrap();
        fs::create_dir(input.join("nested")).unwrap();
        write_ppm(&input.join("nested").join("nested.ppm"), [1, 2, 3]);

        for output in [&output_one, &output_two] {
            let (status, _, stderr) = invoke(&[
                OsString::from("--batch"),
                input.as_os_str().to_owned(),
                output.as_os_str().to_owned(),
            ]);
            assert_eq!(status, EXIT_SUCCESS, "{}", String::from_utf8_lossy(&stderr));
            assert!(output.join("A.PPM").is_file());
            assert!(output.join("b.ppm").is_file());
            assert!(!output.join("notes.txt").exists());
            assert!(!output.join("nested").exists());
        }
        assert_eq!(
            fs::read(output_one.join("A.PPM")).unwrap(),
            fs::read(output_two.join("A.PPM")).unwrap()
        );
        assert_eq!(
            fs::read(output_one.join("b.ppm")).unwrap(),
            fs::read(output_two.join("b.ppm")).unwrap()
        );
        fs::remove_dir_all(input).unwrap();
        fs::remove_dir_all(output_one).unwrap();
        fs::remove_dir_all(output_two).unwrap();
    }

    #[test]
    fn concurrent_mixed_batch_matches_single_file_outputs_exactly() {
        let input = temporary_directory("mixed_exact_input");
        let batch_output = input.with_extension("mixed_exact_batch");
        let single_output = input.with_extension("mixed_exact_single");
        fs::create_dir(&single_output).unwrap();
        write_ppm(&input.join("a.ppm"), [10, 20, 30]);
        write_ppm(&input.join("b.PPM"), [210, 120, 30]);
        let mut raw = vec![0_u8; OFFICIAL_RAW_INPUT_BYTE_COUNT];
        raw[..6].copy_from_slice(&[0, 32, 64, 128, 192, 255]);
        fs::write(input.join("c.raw"), raw).unwrap();
        write_ppm(&input.join("d.ppm"), [5, 100, 205]);
        write_ppm(&input.join("e.ppm"), [250, 125, 0]);

        let names = ["a.ppm", "b.PPM", "c.raw", "d.ppm", "e.ppm"];
        for name in names {
            let source = input.join(name);
            let destination = single_output.join(name);
            let (status, _, stderr) = invoke(&[
                source.as_os_str().to_owned(),
                destination.as_os_str().to_owned(),
            ]);
            assert_eq!(status, EXIT_SUCCESS, "{}", String::from_utf8_lossy(&stderr));
        }

        let (status, _, stderr) = invoke(&[
            OsString::from("--batch"),
            input.as_os_str().to_owned(),
            batch_output.as_os_str().to_owned(),
        ]);
        assert_eq!(status, EXIT_SUCCESS, "{}", String::from_utf8_lossy(&stderr));
        for name in names {
            assert_eq!(
                fs::read(batch_output.join(name)).unwrap(),
                fs::read(single_output.join(name)).unwrap(),
                "batch output differs for {name}"
            );
        }

        fs::remove_dir_all(input).unwrap();
        fs::remove_dir_all(batch_output).unwrap();
        fs::remove_dir_all(single_output).unwrap();
    }

    #[test]
    fn batch_rejects_no_candidates_with_stable_status() {
        let input = temporary_directory("empty_input");
        let output = input.with_extension("empty_output");
        fs::write(input.join("readme.txt"), b"ignored").unwrap();
        let (status, _, stderr) = invoke(&[
            OsString::from("--batch"),
            input.as_os_str().to_owned(),
            output.as_os_str().to_owned(),
        ]);
        assert_eq!(status, EXIT_PROCESSING);
        assert!(String::from_utf8_lossy(&stderr).contains("no supported image candidates"));
        assert!(!output.exists());
        fs::remove_dir_all(input).unwrap();
    }

    #[test]
    fn batch_discovers_mixed_supported_extensions_in_filename_order() {
        let input = temporary_directory("mixed_discovery");
        fs::write(input.join("c.RgB"), b"").unwrap();
        fs::write(input.join("A.raw"), b"").unwrap();
        fs::write(input.join("b.PPM"), b"").unwrap();
        fs::write(input.join("ignored.txt"), b"").unwrap();
        fs::create_dir(input.join("nested")).unwrap();
        fs::write(input.join("nested").join("nested.raw"), b"").unwrap();
        let candidates = discover_batch_candidates(&input).unwrap();
        let names: Vec<_> = candidates
            .iter()
            .map(|(path, format)| (path.file_name().unwrap().to_owned(), *format))
            .collect();
        assert_eq!(
            names,
            vec![
                (OsString::from("A.raw"), ImageFormat::RawRgb8),
                (OsString::from("b.PPM"), ImageFormat::PpmP6),
                (OsString::from("c.RgB"), ImageFormat::RawRgb8),
            ]
        );
        fs::remove_dir_all(input).unwrap();
    }

    #[test]
    fn batch_refuses_existing_output_without_replacement() {
        let input = temporary_directory("existing_input");
        let output = input.with_extension("existing_output");
        fs::create_dir(&output).unwrap();
        write_ppm(&input.join("frame.ppm"), [1, 2, 3]);
        fs::write(output.join("frame.ppm"), b"keep").unwrap();
        let (status, _, stderr) = invoke(&[
            OsString::from("--batch"),
            input.as_os_str().to_owned(),
            output.as_os_str().to_owned(),
        ]);
        assert_eq!(status, EXIT_PROCESSING);
        assert!(String::from_utf8_lossy(&stderr).contains("refusing to overwrite"));
        assert_eq!(fs::read(output.join("frame.ppm")).unwrap(), b"keep");
        fs::remove_dir_all(input).unwrap();
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn batch_continues_after_failures_and_reports_them_in_order() {
        let input = temporary_directory("partial_input");
        let output_one = input.with_extension("partial_output_one");
        let output_two = input.with_extension("partial_output_two");
        fs::write(input.join("a.ppm"), b"bad a").unwrap();
        fs::write(input.join("b.raw"), b"bad raw").unwrap();
        write_ppm(&input.join("c.ppm"), [90, 100, 110]);
        fs::write(input.join("d.PPM"), b"bad d").unwrap();
        let mut diagnostics = Vec::new();
        for output in [&output_one, &output_two] {
            let (status, _, stderr) = invoke(&[
                OsString::from("--batch"),
                input.as_os_str().to_owned(),
                output.as_os_str().to_owned(),
            ]);
            assert_eq!(status, EXIT_PROCESSING);
            assert!(output.join("c.ppm").is_file());
            diagnostics.push(String::from_utf8(stderr).unwrap());
        }
        assert_eq!(diagnostics[0], diagnostics[1]);
        let diagnostics = &diagnostics[0];
        let first = diagnostics.find("a.ppm").unwrap();
        let second = diagnostics.find("b.raw").unwrap();
        let third = diagnostics.find("d.PPM").unwrap();
        assert!(first < second && second < third);
        fs::remove_dir_all(input).unwrap();
        fs::remove_dir_all(output_one).unwrap();
        fs::remove_dir_all(output_two).unwrap();
    }

    fn fnv1a64(data: &[u8]) -> u64 {
        data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }
}
