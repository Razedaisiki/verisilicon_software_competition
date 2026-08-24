//! Replaceable super-resolution algorithm interface and scalar baseline.

pub mod bicubic;
pub mod color;
pub mod quality;
pub mod recommended;

use crate::image::Image;
use crate::spec::{Dimensions, ProcessingConfig, SpecError};
use std::fmt;
use std::thread::{self, ScopedJoinHandle};

pub use bicubic::BicubicBaseline;
pub use quality::{
    BilinearChromaQualityPipeline, ConfidenceGatedQualityPipeline, FineFinalistQualityPipeline,
    QualityPipeline, SelectedQualityPipeline,
};
pub use recommended::RecommendedBaselineV1;

/// Maximum number of independent channel workers used by the scalar pipelines.
pub const MAX_CHANNEL_WORKERS: usize = 3;

/// Minimum input pixel count at which automatic channel parallelism is considered.
pub const PARALLEL_MIN_INPUT_PIXELS: usize = 131_072;

/// Requested execution policy for diagnostic and regression use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionPolicy {
    Auto,
    Serial,
    Parallel,
}

impl fmt::Display for ExecutionPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => formatter.write_str("auto"),
            Self::Serial => formatter.write_str("serial"),
            Self::Parallel => formatter.write_str("parallel"),
        }
    }
}

/// Reports the policy selected by automatic execution for these dimensions.
#[must_use]
pub fn selected_execution_policy(dimensions: Dimensions) -> ExecutionPolicy {
    let large_enough = dimensions
        .pixel_count()
        .is_ok_and(|count| count >= PARALLEL_MIN_INPUT_PIXELS);
    let has_parallelism = thread::available_parallelism().is_ok_and(|count| count.get() >= 2);
    if large_enough && has_parallelism {
        ExecutionPolicy::Parallel
    } else {
        ExecutionPolicy::Serial
    }
}

pub(crate) fn resolve_execution_policy(
    requested: ExecutionPolicy,
    dimensions: Dimensions,
) -> ExecutionPolicy {
    match requested {
        ExecutionPolicy::Auto => selected_execution_policy(dimensions),
        forced => forced,
    }
}

/// Boundary implemented by deterministic CPU algorithms.
pub trait SuperResolution {
    fn process(&self, input: &Image, config: ProcessingConfig) -> Result<Image, AlgorithmError>;
}

/// Errors exposed by an algorithm implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AlgorithmError {
    InvalidConfiguration(&'static str),
    DimensionMismatch {
        expected: Dimensions,
        actual: Dimensions,
    },
    InvalidPlaneLength {
        expected: usize,
        actual: usize,
    },
    InvalidDimensions(SpecError),
    AllocationFailed,
    ThreadSpawnFailed,
    WorkerPanicked,
    ProcessingFailed(&'static str),
}

impl fmt::Display for AlgorithmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid algorithm configuration: {message}")
            }
            Self::DimensionMismatch { expected, actual } => write!(
                formatter,
                "dimension mismatch: expected {} by {}, received {} by {}",
                expected.width(),
                expected.height(),
                actual.width(),
                actual.height()
            ),
            Self::InvalidPlaneLength { expected, actual } => write!(
                formatter,
                "plane length mismatch: expected {expected}, received {actual}"
            ),
            Self::InvalidDimensions(error) => write!(formatter, "invalid dimensions: {error}"),
            Self::AllocationFailed => formatter.write_str("image allocation failed"),
            Self::ThreadSpawnFailed => formatter.write_str("failed to spawn channel worker"),
            Self::WorkerPanicked => formatter.write_str("channel worker panicked"),
            Self::ProcessingFailed(message) => write!(formatter, "processing failed: {message}"),
        }
    }
}

impl std::error::Error for AlgorithmError {}

pub(crate) fn run_channel_jobs<T, F>(
    policy: ExecutionPolicy,
    task: F,
) -> Result<[T; MAX_CHANNEL_WORKERS], AlgorithmError>
where
    T: Send,
    F: Fn(usize) -> Result<T, AlgorithmError> + Sync,
{
    if policy == ExecutionPolicy::Serial {
        return Ok([task(0)?, task(1)?, task(2)?]);
    }

    thread::scope(|scope| {
        let mut handles = Vec::new();
        handles
            .try_reserve_exact(MAX_CHANNEL_WORKERS)
            .map_err(|_| AlgorithmError::AllocationFailed)?;
        let mut spawn_failed = false;
        for channel in 0..MAX_CHANNEL_WORKERS {
            let task = &task;
            match thread::Builder::new()
                .name(format!("sr-channel-{channel}"))
                .spawn_scoped(scope, move || task(channel))
            {
                Ok(handle) => handles.push((channel, handle)),
                Err(_) => {
                    spawn_failed = true;
                    break;
                }
            }
        }
        collect_channel_results(handles, spawn_failed)
    })
}

fn collect_channel_results<T>(
    handles: Vec<(usize, ScopedJoinHandle<'_, Result<T, AlgorithmError>>)>,
    spawn_failed: bool,
) -> Result<[T; MAX_CHANNEL_WORKERS], AlgorithmError> {
    let mut results: [Option<T>; MAX_CHANNEL_WORKERS] = std::array::from_fn(|_| None);
    let mut first_worker_error = None;
    let mut worker_panicked = false;

    // Never return from this loop: every successful spawn must be joined so a
    // worker panic cannot escape through `thread::scope`.
    for (channel, handle) in handles {
        match handle.join() {
            Ok(Ok(value)) => results[channel] = Some(value),
            Ok(Err(error)) if first_worker_error.is_none() => first_worker_error = Some(error),
            Ok(Err(_)) => {}
            Err(_) => worker_panicked = true,
        }
    }

    if spawn_failed {
        return Err(AlgorithmError::ThreadSpawnFailed);
    }
    if worker_panicked {
        return Err(AlgorithmError::WorkerPanicked);
    }
    if let Some(error) = first_worker_error {
        return Err(error);
    }
    let [first, second, third] = results;
    Ok([
        first.ok_or(AlgorithmError::ProcessingFailed("missing channel result"))?,
        second.ok_or(AlgorithmError::ProcessingFailed("missing channel result"))?,
        third.ok_or(AlgorithmError::ProcessingFailed("missing channel result"))?,
    ])
}

#[cfg(test)]
mod threading_tests {
    use super::{
        AlgorithmError, ExecutionPolicy, collect_channel_results, resolve_execution_policy,
        selected_execution_policy,
    };
    use crate::spec::Dimensions;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[test]
    fn tiny_inputs_are_auto_serial_and_forced_policies_are_stable() {
        let dimensions = Dimensions::new(1, 1).unwrap();
        assert_eq!(
            selected_execution_policy(dimensions),
            ExecutionPolicy::Serial
        );
        assert_eq!(
            resolve_execution_policy(ExecutionPolicy::Serial, dimensions),
            ExecutionPolicy::Serial
        );
        assert_eq!(
            resolve_execution_policy(ExecutionPolicy::Parallel, dimensions),
            ExecutionPolicy::Parallel
        );
    }

    #[test]
    fn thread_failure_messages_are_stable() {
        assert_eq!(
            AlgorithmError::ThreadSpawnFailed.to_string(),
            "failed to spawn channel worker"
        );
        assert_eq!(
            AlgorithmError::WorkerPanicked.to_string(),
            "channel worker panicked"
        );
    }

    #[test]
    fn collector_joins_every_handle_after_a_worker_panic() {
        let joined_effects = Arc::new(AtomicUsize::new(0));
        let result = thread::scope(|scope| {
            let mut handles = Vec::new();
            for channel in 0..3 {
                let joined_effects = Arc::clone(&joined_effects);
                handles.push((
                    channel,
                    thread::Builder::new()
                        .spawn_scoped(scope, move || {
                            if channel == 0 {
                                panic!("injected worker panic");
                            }
                            joined_effects.fetch_add(1, Ordering::SeqCst);
                            Ok(channel)
                        })
                        .unwrap(),
                ));
            }
            collect_channel_results(handles, false)
        });
        assert_eq!(result, Err(AlgorithmError::WorkerPanicked));
        assert_eq!(joined_effects.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn collector_joins_partial_spawns_before_reporting_spawn_failure() {
        let joined_effects = Arc::new(AtomicUsize::new(0));
        let result = thread::scope(|scope| {
            let joined_effects_worker = Arc::clone(&joined_effects);
            let handle = thread::Builder::new()
                .spawn_scoped(scope, move || {
                    joined_effects_worker.fetch_add(1, Ordering::SeqCst);
                    Ok(7_u8)
                })
                .unwrap();
            collect_channel_results(vec![(0, handle)], true)
        });
        assert_eq!(result, Err(AlgorithmError::ThreadSpawnFailed));
        assert_eq!(joined_effects.load(Ordering::SeqCst), 1);
    }
}
