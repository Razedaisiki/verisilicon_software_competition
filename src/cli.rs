//! Testable command-line routing and baseline processing.

use crate::algorithm::{BicubicBaseline, SuperResolution};
use crate::image::Image;
use crate::io::ppm::PpmP6Codec;
use crate::io::raw::RawRgb8Codec;
use crate::io::{DecodeSpec, ImageDecoder, ImageEncoder, ImageFormat};
use crate::spec::{Dimensions, ProcessingConfig};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_PROCESSING: u8 = 4;

pub const USAGE: &str = "Usage:\n  sr <input.ppm> <output.ppm>\n  sr --raw-rgb8 <width> <height> <input.raw> <output.raw>\n  sr --batch <in_dir> <out_dir>\n  sr --help";

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
    Help,
    Ppm {
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
        Command::Ppm { input, output } => match process_ppm(&input, &output) {
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
            Ok(Command::Ppm {
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
    let timed = process_baseline(&image)?;
    codec.encode(output, ImageFormat::PpmP6, &timed.image)?;
    Ok(timed.processing_time)
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
    let timed = process_baseline(&image)?;
    codec.encode(output, ImageFormat::RawRgb8, &timed.image)?;
    Ok(timed.processing_time)
}

fn process_baseline(image: &Image) -> Result<TimedImage, CliError> {
    let config = ProcessingConfig::new(image.dimensions());
    let start = Instant::now();
    let output = BicubicBaseline::new().process(image, config)?;
    let processing_time = start.elapsed();
    Ok(TimedImage {
        image: output,
        processing_time,
    })
}

fn process_batch(input_dir: &Path, output_dir: &Path) -> Result<BatchReport, CliError> {
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
        if file_type.is_file() && has_ppm_extension(&entry.path()) {
            candidates.push(entry.path());
        }
    }
    candidates.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    if candidates.is_empty() {
        return Err(CliError::NoPpmCandidates(input_dir.to_path_buf()));
    }

    fs::create_dir_all(output_dir).map_err(|error| {
        CliError::File(format!(
            "failed to create output directory {}: {error}",
            output_dir.display()
        ))
    })?;
    let mut report = BatchReport::default();
    for input in candidates {
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
        match process_ppm(&input, &output) {
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

fn has_ppm_extension(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ppm"))
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
    NoPpmCandidates(PathBuf),
    TimingOverflow,
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageIo(error) => error.fmt(formatter),
            Self::Algorithm(error) => error.fmt(formatter),
            Self::File(message) => formatter.write_str(message),
            Self::NoPpmCandidates(path) => {
                write!(formatter, "no PPM candidates found in {}", path.display())
            }
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
    use super::{EXIT_PROCESSING, EXIT_SUCCESS, EXIT_USAGE, run};
    use crate::io::ppm::PpmP6Codec;
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
        let output = temporary_path("ppm");
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
        assert!(String::from_utf8_lossy(&stderr).contains("no PPM candidates"));
        assert!(!output.exists());
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
}
