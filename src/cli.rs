//! Testable command-line routing and selected quality processing.

use crate::algorithm::{SelectedQualityPipeline, SuperResolution};
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
use std::time::{Duration, Instant};

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_PROCESSING: u8 = 4;

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
    let codec = PpmP6Codec::new();
    let image = codec.decode(
        input,
        DecodeSpec {
            format: ImageFormat::PpmP6,
            dimensions: None,
        },
    )?;
    let timed = process_selected_quality(&image)?;
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
    let codec = RawRgb8Codec::new();
    let image = codec.decode(
        input,
        DecodeSpec {
            format: ImageFormat::RawRgb8,
            dimensions: Some(dimensions),
        },
    )?;
    let timed = process_selected_quality(&image)?;
    codec.encode(output, ImageFormat::RawRgb8, &timed.image)?;
    Ok(timed.processing_time)
}

fn process_selected_quality(image: &Image) -> Result<TimedImage, CliError> {
    let config = ProcessingConfig::new(image.dimensions());
    let start = Instant::now();
    let output = SelectedQualityPipeline::new().process(image, config)?;
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
    let mut report = BatchReport::default();
    for (input, format) in candidates {
        let file_name = input
            .file_name()
            .ok_or_else(|| CliError::File("batch candidate has no filename".to_owned()))?;
        let output = output_dir.join(file_name);
        match output.try_exists() {
            Ok(true) => {
                report.failures.push(format!(
                    "{}: refusing to overwrite existing output {}",
                    input.display(),
                    output.display()
                ));
                continue;
            }
            Ok(false) => {}
            Err(error) => {
                report.failures.push(format!(
                    "{}: failed to inspect output {}: {error}",
                    input.display(),
                    output.display()
                ));
                continue;
            }
        }
        let result = match format {
            ImageFormat::PpmP6 => process_ppm(&input, &output),
            ImageFormat::RawRgb8 => {
                process_raw_rgb8(official_raw_input_dimensions(), &input, &output)
            }
        };
        match result {
            Ok(processing_time) => {
                report.succeeded += 1;
                report.processing_time = report
                    .processing_time
                    .checked_add(processing_time)
                    .ok_or(CliError::TimingOverflow)?;
            }
            Err(error) => report
                .failures
                .push(format!("{}: {error}", input.display())),
        }
    }
    Ok(report)
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
        EXIT_PROCESSING, EXIT_SUCCESS, EXIT_USAGE, detect_format_bytes, discover_batch_candidates,
        format_from_extension, run,
    };
    use crate::io::ImageFormat;
    use crate::io::ppm::PpmP6Codec;
    use crate::spec::{OFFICIAL_RAW_INPUT_BYTE_COUNT, OFFICIAL_RAW_OUTPUT_BYTE_COUNT};
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

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
        let output = input.with_extension("partial_output");
        fs::write(input.join("a.ppm"), b"bad a").unwrap();
        write_ppm(&input.join("b.ppm"), [90, 100, 110]);
        fs::write(input.join("c.PPM"), b"bad c").unwrap();
        let (status, _, stderr) = invoke(&[
            OsString::from("--batch"),
            input.as_os_str().to_owned(),
            output.as_os_str().to_owned(),
        ]);
        assert_eq!(status, EXIT_PROCESSING);
        assert!(output.join("b.ppm").is_file());
        let diagnostics = String::from_utf8(stderr).unwrap();
        let first = diagnostics.find("a.ppm").unwrap();
        let second = diagnostics.find("c.PPM").unwrap();
        assert!(first < second);
        fs::remove_dir_all(input).unwrap();
        fs::remove_dir_all(output).unwrap();
    }

    fn fnv1a64(data: &[u8]) -> u64 {
        data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }
}
