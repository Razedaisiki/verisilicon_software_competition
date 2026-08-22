//! Testable command-line routing and baseline processing.

use crate::algorithm::{BicubicBaseline, SuperResolution};
use crate::io::ppm::PpmP6Codec;
use crate::io::raw::RawRgb8Codec;
use crate::io::{DecodeSpec, ImageDecoder, ImageEncoder, ImageFormat};
use crate::spec::{Dimensions, ProcessingConfig};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_NOT_IMPLEMENTED: u8 = 3;
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
        } => {
            let _ = writeln!(
                stderr,
                "Error: batch processing is not implemented yet: {} -> {}",
                input_dir.display(),
                output_dir.display()
            );
            EXIT_NOT_IMPLEMENTED
        }
        Command::Ppm { input, output } => match process_ppm(&input, &output) {
            Ok(()) => EXIT_SUCCESS,
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
            Ok(()) => EXIT_SUCCESS,
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

fn process_ppm(input: &Path, output: &Path) -> Result<(), CliError> {
    let codec = PpmP6Codec::new();
    let image = codec.decode(
        input,
        DecodeSpec {
            format: ImageFormat::PpmP6,
            dimensions: None,
        },
    )?;
    let config = ProcessingConfig::new(image.dimensions());
    let scaled = BicubicBaseline::new().process(&image, config)?;
    codec.encode(output, ImageFormat::PpmP6, &scaled)?;
    Ok(())
}

fn process_raw_rgb8(dimensions: Dimensions, input: &Path, output: &Path) -> Result<(), CliError> {
    let codec = RawRgb8Codec::new();
    let image = codec.decode(
        input,
        DecodeSpec {
            format: ImageFormat::RawRgb8,
            dimensions: Some(dimensions),
        },
    )?;
    let config = ProcessingConfig::new(image.dimensions());
    let scaled = BicubicBaseline::new().process(&image, config)?;
    codec.encode(output, ImageFormat::RawRgb8, &scaled)?;
    Ok(())
}

#[derive(Debug)]
enum CliError {
    ImageIo(crate::io::ImageIoError),
    Algorithm(crate::algorithm::AlgorithmError),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageIo(error) => error.fmt(formatter),
            Self::Algorithm(error) => error.fmt(formatter),
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
    use super::{EXIT_NOT_IMPLEMENTED, EXIT_PROCESSING, EXIT_SUCCESS, EXIT_USAGE, run};
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
    fn help_usage_and_batch_exit_codes_are_stable() {
        assert_eq!(invoke(&[OsString::from("--help")]).0, EXIT_SUCCESS);
        assert_eq!(invoke(&[]).0, EXIT_USAGE);
        assert_eq!(
            invoke(&[
                OsString::from("--batch"),
                OsString::from("input"),
                OsString::from("output"),
            ])
            .0,
            EXIT_NOT_IMPLEMENTED
        );
    }
}
