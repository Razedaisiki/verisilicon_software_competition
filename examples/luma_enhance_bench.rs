//! Processing-only benchmark for the scalar luma enhancement hot loop.

use std::env;
use std::error::Error;
use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;
use verisilicon_sr::algorithm::quality::enhance_luma;
use verisilicon_sr::spec::Dimensions;

const USAGE: &str = "Usage: luma_enhance_bench <width> <height> <iterations>";

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
    if args.len() != 3 {
        return Err("expected three arguments".into());
    }
    let width: u32 = args[0].parse()?;
    let height: u32 = args[1].parse()?;
    let iterations: usize = args[2].parse()?;
    if iterations == 0 {
        return Err("iterations must be positive".into());
    }
    let dimensions = Dimensions::new(width, height)?;
    let count = dimensions.pixel_count()?;
    let input: Vec<u8> = (0..count)
        .map(|index| {
            let x = index % width as usize;
            let y = index / width as usize;
            x.wrapping_mul(37)
                .wrapping_add(y.wrapping_mul(91))
                .wrapping_add(x.wrapping_mul(y).wrapping_mul(13)) as u8
        })
        .collect();

    black_box(enhance_luma(black_box(&input), dimensions)?);
    let started = Instant::now();
    let mut output = None;
    for _ in 0..iterations {
        output = Some(black_box(enhance_luma(black_box(&input), dimensions)?));
    }
    let elapsed = started.elapsed();
    let output = output.expect("positive iteration count checked above");
    let megapixels = count as f64 * iterations as f64 / 1_000_000.0;

    println!("input={}x{}", width, height);
    println!("warmup_iterations=1");
    println!("measured_iterations={iterations}");
    println!("elapsed_ms={:.3}", elapsed.as_secs_f64() * 1_000.0);
    println!(
        "megapixels_per_second={:.3}",
        megapixels / elapsed.as_secs_f64()
    );
    println!("checksum={:016x}", fnv1a64(&output));
    Ok(())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
