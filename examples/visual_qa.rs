use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use verisilicon_sr::algorithm::{BicubicBaseline, QualityPipeline, SuperResolution};
use verisilicon_sr::fixtures::{HardEdge, checker_detail, constant, hard_edge, smooth_gradient};
use verisilicon_sr::image::{Image, Rgb8};
use verisilicon_sr::io::ppm::PpmP6Codec;
use verisilicon_sr::io::{ImageEncoder, ImageFormat};
use verisilicon_sr::metrics::{luma_psnr, luma_ssim};
use verisilicon_sr::spec::{Dimensions, ProcessingConfig};

const USAGE: &str = "Usage: visual_qa <output_dir>";

struct Case {
    name: &'static str,
    input: Image,
    reference: Image,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.len() != 1 {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    match run(Path::new(&args[0])) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(output_dir: &Path) -> Result<(), Box<dyn Error>> {
    let low = Dimensions::new(16, 12)?;
    let high = low.scaled(verisilicon_sr::spec::Scale::X2)?;
    let cases = vec![
        Case {
            name: "constant",
            input: constant(low, Rgb8::new(96, 96, 96))?,
            reference: constant(high, Rgb8::new(96, 96, 96))?,
        },
        Case {
            name: "gradient",
            input: smooth_gradient(low)?,
            reference: smooth_gradient(high)?,
        },
        Case {
            name: "hard_edge",
            input: hard_edge(low, HardEdge::Vertical)?,
            reference: hard_edge(high, HardEdge::Vertical)?,
        },
        Case {
            name: "checker",
            input: checker_detail(low, 2)?,
            reference: checker_detail(high, 4)?,
        },
    ];

    let planned = planned_paths(output_dir, &cases);
    if output_dir.exists() && !output_dir.is_dir() {
        return Err(format!("output path is not a directory: {}", output_dir.display()).into());
    }
    for path in &planned {
        if path.exists() {
            return Err(format!(
                "refusing to overwrite existing artifact: {}",
                path.display()
            )
            .into());
        }
    }
    fs::create_dir_all(output_dir)?;

    let codec = PpmP6Codec::new();
    println!("Diagnostic metrics are provisional and are not official contest scores.");
    for case in cases {
        let config = ProcessingConfig::new(case.input.dimensions());
        let baseline = BicubicBaseline::new().process(&case.input, config)?;
        let quality = QualityPipeline::new().process(&case.input, config)?;
        codec.encode(
            &output_dir.join(format!("{}_input.ppm", case.name)),
            ImageFormat::PpmP6,
            &case.input,
        )?;
        codec.encode(
            &output_dir.join(format!("{}_reference.ppm", case.name)),
            ImageFormat::PpmP6,
            &case.reference,
        )?;
        codec.encode(
            &output_dir.join(format!("{}_baseline.ppm", case.name)),
            ImageFormat::PpmP6,
            &baseline,
        )?;
        codec.encode(
            &output_dir.join(format!("{}_quality.ppm", case.name)),
            ImageFormat::PpmP6,
            &quality,
        )?;
        println!(
            "{}: baseline_psnr={} baseline_ssim={:.6} quality_psnr={} quality_ssim={:.6}",
            case.name,
            luma_psnr(&case.reference, &baseline)?,
            luma_ssim(&case.reference, &baseline)?,
            luma_psnr(&case.reference, &quality)?,
            luma_ssim(&case.reference, &quality)?
        );
    }
    Ok(())
}

fn planned_paths(output_dir: &Path, cases: &[Case]) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(cases.len() * 4);
    for case in cases {
        for suffix in ["input", "reference", "baseline", "quality"] {
            paths.push(output_dir.join(format!("{}_{}.ppm", case.name, suffix)));
        }
    }
    paths
}
