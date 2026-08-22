use std::env;
use std::process::ExitCode;

const EXIT_USAGE: u8 = 2;
const EXIT_NOT_IMPLEMENTED: u8 = 3;

const USAGE: &str = "Usage:\n  sr <input> <output>\n  sr --batch <in_dir> <out_dir>\n  sr --help";

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Help,
    Single {
        input: String,
        output: String,
    },
    Batch {
        input_dir: String,
        output_dir: String,
    },
}

fn parse_args<I>(args: I) -> Result<Command, &'static str>
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => Ok(Command::Help),
        [input, output] if input != "--batch" => Ok(Command::Single {
            input: input.clone(),
            output: output.clone(),
        }),
        [flag, input_dir, output_dir] if flag == "--batch" => Ok(Command::Batch {
            input_dir: input_dir.clone(),
            output_dir: output_dir.clone(),
        }),
        [] => Err("missing command arguments"),
        _ => Err("invalid command arguments"),
    }
}

fn main() -> ExitCode {
    let command = match parse_args(env::args().skip(1)) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("Error: {message}\n{USAGE}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    match command {
        Command::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Command::Single { input, output } => {
            eprintln!("Error: single-file processing is not implemented yet: {input} -> {output}");
            ExitCode::from(EXIT_NOT_IMPLEMENTED)
        }
        Command::Batch {
            input_dir,
            output_dir,
        } => {
            eprintln!(
                "Error: batch processing is not implemented yet: {input_dir} -> {output_dir}"
            );
            ExitCode::from(EXIT_NOT_IMPLEMENTED)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_args};

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_help() {
        assert_eq!(parse_args(strings(&["--help"])), Ok(Command::Help));
    }

    #[test]
    fn parses_single_file_command() {
        assert_eq!(
            parse_args(strings(&["input.ppm", "output.ppm"])),
            Ok(Command::Single {
                input: "input.ppm".to_owned(),
                output: "output.ppm".to_owned()
            })
        );
    }

    #[test]
    fn parses_batch_command() {
        assert_eq!(
            parse_args(strings(&["--batch", "inputs", "outputs"])),
            Ok(Command::Batch {
                input_dir: "inputs".to_owned(),
                output_dir: "outputs".to_owned()
            })
        );
    }

    #[test]
    fn rejects_invalid_arguments() {
        assert_eq!(parse_args(strings(&[])), Err("missing command arguments"));
        assert_eq!(
            parse_args(strings(&["--unknown"])),
            Err("invalid command arguments")
        );
    }
}
