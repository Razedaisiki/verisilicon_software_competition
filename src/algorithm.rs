//! Replaceable super-resolution algorithm interface.

use crate::image::Image;
use crate::spec::ProcessingConfig;
use std::fmt;

/// Boundary implemented by deterministic CPU algorithms.
pub trait SuperResolution {
    fn process(&self, input: &Image, config: ProcessingConfig) -> Result<Image, AlgorithmError>;
}

/// Errors exposed by an algorithm implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AlgorithmError {
    InvalidConfiguration(&'static str),
    ProcessingFailed(&'static str),
}

impl fmt::Display for AlgorithmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid algorithm configuration: {message}")
            }
            Self::ProcessingFailed(message) => write!(formatter, "processing failed: {message}"),
        }
    }
}

impl std::error::Error for AlgorithmError {}
