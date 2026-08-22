//! Replaceable image input and output interfaces.

pub mod ppm;

use crate::image::Image;
use crate::spec::Dimensions;
use std::fmt;
use std::path::Path;

/// Formats required by the public interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageFormat {
    PpmP6,
    RawRgb8,
}

/// Information needed to decode an input image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeSpec {
    pub format: ImageFormat,
    pub dimensions: Option<Dimensions>,
}

/// Boundary for a replaceable image decoder.
pub trait ImageDecoder {
    fn decode(&self, path: &Path, spec: DecodeSpec) -> Result<Image, ImageIoError>;
}

/// Boundary for a replaceable image encoder.
pub trait ImageEncoder {
    fn encode(&self, path: &Path, format: ImageFormat, image: &Image) -> Result<(), ImageIoError>;
}

/// Errors exposed by image format adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImageIoError {
    UnsupportedFormat(ImageFormat),
    InvalidData(&'static str),
    File(String),
    Ppm(ppm::PpmError),
}

impl fmt::Display for ImageIoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFormat(format) => {
                write!(formatter, "unsupported image format: {format:?}")
            }
            Self::InvalidData(message) => write!(formatter, "invalid image data: {message}"),
            Self::File(message) => write!(formatter, "image file error: {message}"),
            Self::Ppm(error) => write!(formatter, "PPM P6 error: {error}"),
        }
    }
}

impl std::error::Error for ImageIoError {}

impl From<ppm::PpmError> for ImageIoError {
    fn from(error: ppm::PpmError) -> Self {
        Self::Ppm(error)
    }
}
