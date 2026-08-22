//! Replaceable super-resolution algorithm interface and scalar baseline.

pub mod bicubic;
pub mod color;

use crate::image::Image;
use crate::spec::{Dimensions, ProcessingConfig, SpecError};
use std::fmt;

pub use bicubic::BicubicBaseline;

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
            Self::ProcessingFailed(message) => write!(formatter, "processing failed: {message}"),
        }
    }
}

impl std::error::Error for AlgorithmError {}
