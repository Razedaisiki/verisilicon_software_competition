//! Reproducible processing-only microbenchmark for local diagnostics.

use std::env;
use std::error::Error;
use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;
use verisilicon_sr::algorithm::{
    BicubicBaseline, ConfidenceGatedQualityPipeline, ExecutionPolicy, QualityPipeline,
    RecommendedBaselineV1, SelectedQualityPipeline, selected_execution_policy,
};
use verisilicon_sr::fixtures::smooth_gradient;
use verisilicon_sr::image::Image;
use verisilicon_sr::spec::{Dimensions, ProcessingConfig};

const USAGE: &str = "Usage: processing_bench <baseline|recommended|quality|selected-ungated|confidence-gated> <auto|serial|parallel> <width> <height> <iterations>";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("expected five arguments".into());
    }
    let mode = args[0].as_str();
    if !matches!(
        mode,
        "baseline" | "recommended" | "quality" | "selected-ungated" | "confidence-gated"
    ) {
        return Err(
            "mode must be baseline, recommended, quality, selected-ungated, or confidence-gated"
                .into(),
        );
    }
    let policy = match args[1].as_str() {
        "auto" => ExecutionPolicy::Auto,
        "serial" => ExecutionPolicy::Serial,
        "parallel" => ExecutionPolicy::Parallel,
        _ => return Err("policy must be auto, serial, or parallel".into()),
    };
    let width: u32 = args[2].parse()?;
    let height: u32 = args[3].parse()?;
    let iterations: u32 = args[4].parse()?;
    if iterations == 0 {
        return Err("iterations must be positive".into());
    }

    let dimensions = Dimensions::new(width, height)?;
    let input = smooth_gradient(dimensions)?;
    let config = ProcessingConfig::new(dimensions);

    // Warm-up is deliberately outside the measured processing interval.
    black_box(process(mode, &input, config, policy)?);
    let started = Instant::now();
    let mut last = None;
    for _ in 0..iterations {
        last = Some(black_box(process(mode, black_box(&input), config, policy)?));
    }
    let elapsed = started.elapsed();
    let output = last.expect("positive iteration count checked above");
    let seconds = elapsed.as_secs_f64();
    let frames_per_second = f64::from(iterations) / seconds;
    let output_pixels = output.dimensions().pixel_count()?;
    let megapixels_per_second =
        output_pixels as f64 * f64::from(iterations) / seconds / 1_000_000.0;

    println!("mode={mode}");
    println!(
        "available_parallelism={}",
        std::thread::available_parallelism().map_or(1, usize::from)
    );
    println!("requested_policy={policy}");
    println!(
        "selected_policy={}",
        match policy {
            ExecutionPolicy::Auto => selected_execution_policy(dimensions),
            forced => forced,
        }
    );
    println!("input={}x{}", width, height);
    println!(
        "output={}x{}",
        output.dimensions().width(),
        output.dimensions().height()
    );
    println!("warmup_iterations=1");
    println!("measured_iterations={iterations}");
    println!("elapsed_ms={:.3}", elapsed.as_secs_f64() * 1_000.0);
    println!("frames_per_second={frames_per_second:.3}");
    println!("output_megapixels_per_second={megapixels_per_second:.3}");
    println!("checksum={:016x}", checksum(&output));
    Ok(())
}

fn process(
    mode: &str,
    input: &Image,
    config: ProcessingConfig,
    policy: ExecutionPolicy,
) -> Result<Image, verisilicon_sr::algorithm::AlgorithmError> {
    match mode {
        "baseline" => BicubicBaseline::new().process_with_policy(input, config, policy),
        "recommended" => RecommendedBaselineV1::new().process_with_policy(input, config, policy),
        "quality" => QualityPipeline::new().process_with_policy(input, config, policy),
        "selected-ungated" => {
            SelectedQualityPipeline::new().process_with_policy(input, config, policy)
        }
        "confidence-gated" => {
            ConfidenceGatedQualityPipeline::new().process_with_policy(input, config, policy)
        }
        _ => unreachable!("mode validated before processing"),
    }
}

fn checksum(image: &Image) -> u64 {
    image
        .pixels()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, pixel| {
            [pixel.red, pixel.green, pixel.blue]
                .into_iter()
                .fold(hash, |value, byte| {
                    (value ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
                })
        })
}
