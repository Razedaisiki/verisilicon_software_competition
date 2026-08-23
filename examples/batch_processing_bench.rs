//! Processing-only batch concurrency benchmark for the selected pipeline.

use std::env;
use std::error::Error;
use std::hint::black_box;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Instant;
use verisilicon_sr::algorithm::{AlgorithmError, ExecutionPolicy, SelectedQualityPipeline};
use verisilicon_sr::fixtures::smooth_gradient;
use verisilicon_sr::image::{Image, Rgb8};
use verisilicon_sr::spec::{Dimensions, ProcessingConfig};

const USAGE: &str = "Usage: batch_processing_bench <serial|parallel> <frame-workers> <width> <height> <frames-per-batch> <measured-batch-iterations>";

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
    if args.len() != 6 {
        return Err("expected six arguments".into());
    }

    let policy = match args[0].as_str() {
        "serial" => ExecutionPolicy::Serial,
        "parallel" => ExecutionPolicy::Parallel,
        _ => return Err("policy must be serial or parallel".into()),
    };
    let frame_workers: usize = parse_positive(&args[1], "frame-workers")?;
    let width: u32 = args[2].parse()?;
    let height: u32 = args[3].parse()?;
    let frames_per_batch: usize = parse_positive(&args[4], "frames-per-batch")?;
    let measured_batches: usize = parse_positive(&args[5], "measured-batch-iterations")?;

    let dimensions = Dimensions::new(width, height)?;
    let config = ProcessingConfig::new(dimensions);

    // Fixture generation and worker creation are deliberately outside both the
    // warm-up and measured processing intervals.
    let inputs = make_inputs(dimensions, frames_per_batch)?;
    let peak_active = Arc::new(AtomicUsize::new(0));
    let pool = FrameWorkerPool::new(frame_workers, config, policy, Arc::clone(&peak_active))?;

    // One complete batch warms the same fixed workers used by measurement.
    black_box(pool.process_batch(&inputs)?);
    peak_active.store(0, Ordering::Relaxed);

    let started = Instant::now();
    let mut last_outputs = None;
    for _ in 0..measured_batches {
        last_outputs = Some(black_box(pool.process_batch(black_box(&inputs))?));
    }
    let elapsed = started.elapsed();

    // Output hashing is excluded from the processing interval.
    let outputs = last_outputs.expect("positive measured batch count checked above");
    let checksum = ordered_batch_checksum(&outputs);
    let total_frames = frames_per_batch
        .checked_mul(measured_batches)
        .ok_or("total measured frame count overflowed")?;
    let seconds = elapsed.as_secs_f64();
    let frames_per_second = total_frames as f64 / seconds;

    println!(
        "available_parallelism={}",
        thread::available_parallelism().map_or(1, usize::from)
    );
    println!("requested_frame_workers={frame_workers}");
    println!("requested_inner_policy={policy}");
    println!("input={}x{}", width, height);
    println!(
        "output={}x{}",
        outputs[0].dimensions().width(),
        outputs[0].dimensions().height()
    );
    println!("frames_per_batch={frames_per_batch}");
    println!("warmup_batch_iterations=1");
    println!("measured_batch_iterations={measured_batches}");
    println!("measured_frames={total_frames}");
    println!("elapsed_ms={:.3}", elapsed.as_secs_f64() * 1_000.0);
    println!("batch_frames_per_second={frames_per_second:.3}");
    println!(
        "peak_simultaneous_frames={}",
        peak_active.load(Ordering::Relaxed)
    );
    println!("checksum={checksum:016x}");
    Ok(())
}

fn parse_positive(value: &str, name: &str) -> Result<usize, Box<dyn Error>> {
    let parsed: usize = value.parse()?;
    if parsed == 0 {
        return Err(format!("{name} must be positive").into());
    }
    Ok(parsed)
}

fn make_inputs(
    dimensions: Dimensions,
    frames_per_batch: usize,
) -> Result<Vec<Arc<Image>>, Box<dyn Error>> {
    let base = smooth_gradient(dimensions)?;
    let mut inputs = Vec::new();
    inputs.try_reserve_exact(frames_per_batch)?;
    for frame_index in 0..frames_per_batch {
        let phase = frame_index.wrapping_mul(37) as u8;
        let pixels = base
            .pixels()
            .iter()
            .map(|pixel| {
                Rgb8::new(
                    pixel.red.wrapping_add(phase),
                    pixel.green.wrapping_add(phase.rotate_left(1)),
                    pixel.blue.wrapping_add(phase.rotate_left(2)),
                )
            })
            .collect();
        inputs.push(Arc::new(Image::new(dimensions, pixels)?));
    }
    Ok(inputs)
}

struct FrameTask {
    order: usize,
    input: Arc<Image>,
}

struct FrameResult {
    order: usize,
    output: Result<Image, AlgorithmError>,
}

struct FrameWorkerPool {
    task_sender: Option<mpsc::Sender<FrameTask>>,
    result_receiver: mpsc::Receiver<FrameResult>,
    workers: Vec<JoinHandle<()>>,
}

impl FrameWorkerPool {
    fn new(
        worker_count: usize,
        config: ProcessingConfig,
        policy: ExecutionPolicy,
        peak_active: Arc<AtomicUsize>,
    ) -> Result<Self, Box<dyn Error>> {
        let (task_sender, task_receiver) = mpsc::channel::<FrameTask>();
        let (result_sender, result_receiver) = mpsc::channel::<FrameResult>();
        let task_receiver = Arc::new(Mutex::new(task_receiver));
        let active = Arc::new(AtomicUsize::new(0));
        let mut workers = Vec::new();
        workers.try_reserve_exact(worker_count)?;

        for worker_index in 0..worker_count {
            let task_receiver = Arc::clone(&task_receiver);
            let result_sender = result_sender.clone();
            let active = Arc::clone(&active);
            let peak_active = Arc::clone(&peak_active);
            let worker = thread::Builder::new()
                .name(format!("sr-frame-{worker_index}"))
                .spawn(move || {
                    worker_loop(
                        &task_receiver,
                        &result_sender,
                        &active,
                        &peak_active,
                        config,
                        policy,
                    );
                })?;
            workers.push(worker);
        }
        drop(result_sender);

        Ok(Self {
            task_sender: Some(task_sender),
            result_receiver,
            workers,
        })
    }

    fn process_batch(&self, inputs: &[Arc<Image>]) -> Result<Vec<Image>, Box<dyn Error>> {
        let task_sender = self
            .task_sender
            .as_ref()
            .ok_or("frame worker pool is stopped")?;
        for (order, input) in inputs.iter().enumerate() {
            task_sender.send(FrameTask {
                order,
                input: Arc::clone(input),
            })?;
        }

        let mut ordered: Vec<Option<Image>> = (0..inputs.len()).map(|_| None).collect();
        for _ in 0..inputs.len() {
            let result = self.result_receiver.recv()?;
            ordered[result.order] = Some(result.output?);
        }
        ordered
            .into_iter()
            .map(|output| output.ok_or_else(|| "missing frame result".into()))
            .collect()
    }
}

impl Drop for FrameWorkerPool {
    fn drop(&mut self) {
        self.task_sender.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    task_receiver: &Mutex<mpsc::Receiver<FrameTask>>,
    result_sender: &mpsc::Sender<FrameResult>,
    active: &AtomicUsize,
    peak_active: &AtomicUsize,
    config: ProcessingConfig,
    policy: ExecutionPolicy,
) {
    loop {
        let task = match task_receiver.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let Ok(task) = task else {
            return;
        };

        let simultaneous = active.fetch_add(1, Ordering::Relaxed) + 1;
        update_peak(peak_active, simultaneous);
        let output =
            SelectedQualityPipeline::new().process_with_policy(&task.input, config, policy);
        active.fetch_sub(1, Ordering::Relaxed);

        if result_sender
            .send(FrameResult {
                order: task.order,
                output,
            })
            .is_err()
        {
            return;
        }
    }
}

fn update_peak(peak: &AtomicUsize, candidate: usize) {
    let mut observed = peak.load(Ordering::Relaxed);
    while candidate > observed {
        match peak.compare_exchange_weak(observed, candidate, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return,
            Err(current) => observed = current,
        }
    }
}

fn ordered_batch_checksum(outputs: &[Image]) -> u64 {
    outputs
        .iter()
        .enumerate()
        .fold(0xcbf2_9ce4_8422_2325, |hash, (frame_index, image)| {
            let hash = frame_index.to_le_bytes().into_iter().fold(hash, fnv_byte);
            image.pixels().iter().fold(hash, |hash, pixel| {
                [pixel.red, pixel.green, pixel.blue]
                    .into_iter()
                    .fold(hash, fnv_byte)
            })
        })
}

fn fnv_byte(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
}
