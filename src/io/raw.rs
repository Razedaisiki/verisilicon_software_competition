//! Strict provisional packed row-major RGB8 codec for assumption A-002.

use super::{DecodeSpec, ImageDecoder, ImageEncoder, ImageFormat, ImageIoError};
use crate::image::{Image, ImageError, Rgb8};
use crate::spec::{Dimensions, SpecError};
use std::fmt;
use std::fs;
use std::path::Path;

/// Stateless packed row-major RGB8 byte and file codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct RawRgb8Codec;

impl RawRgb8Codec {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Decodes exactly width times height packed RGB8 pixels.
    pub fn decode_bytes(self, input: &[u8], dimensions: Dimensions) -> Result<Image, RawRgb8Error> {
        let pixel_count = dimensions
            .pixel_count()
            .map_err(RawRgb8Error::InvalidDimensions)?;
        let expected = pixel_count
            .checked_mul(3)
            .ok_or(RawRgb8Error::ByteCountOverflow)?;
        if input.len() < expected {
            return Err(RawRgb8Error::TruncatedData {
                expected,
                actual: input.len(),
            });
        }
        if input.len() > expected {
            return Err(RawRgb8Error::UnexpectedTrailingBytes {
                expected,
                actual: input.len(),
            });
        }

        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(pixel_count)
            .map_err(|_| RawRgb8Error::AllocationFailed)?;
        pixels.extend(
            input
                .chunks_exact(3)
                .map(|pixel| Rgb8::new(pixel[0], pixel[1], pixel[2])),
        );
        Image::new(dimensions, pixels).map_err(RawRgb8Error::InvalidImage)
    }

    /// Encodes exactly packed row-major RGB8 samples without a header.
    pub fn encode_bytes(self, image: &Image) -> Result<Vec<u8>, RawRgb8Error> {
        let pixel_count = image
            .dimensions()
            .pixel_count()
            .map_err(RawRgb8Error::InvalidDimensions)?;
        let byte_count = pixel_count
            .checked_mul(3)
            .ok_or(RawRgb8Error::ByteCountOverflow)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(byte_count)
            .map_err(|_| RawRgb8Error::AllocationFailed)?;
        for pixel in image.pixels() {
            output.extend_from_slice(&[pixel.red, pixel.green, pixel.blue]);
        }
        Ok(output)
    }
}

impl ImageDecoder for RawRgb8Codec {
    fn decode(&self, path: &Path, spec: DecodeSpec) -> Result<Image, ImageIoError> {
        if spec.format != ImageFormat::RawRgb8 {
            return Err(ImageIoError::UnsupportedFormat(spec.format));
        }
        let dimensions = spec.dimensions.ok_or(RawRgb8Error::MissingDimensions)?;
        let input = fs::read(path).map_err(|error| {
            ImageIoError::File(format!("failed to read {}: {error}", path.display()))
        })?;
        self.decode_bytes(&input, dimensions).map_err(Into::into)
    }
}

impl ImageEncoder for RawRgb8Codec {
    fn encode(&self, path: &Path, format: ImageFormat, image: &Image) -> Result<(), ImageIoError> {
        if format != ImageFormat::RawRgb8 {
            return Err(ImageIoError::UnsupportedFormat(format));
        }
        let output = self.encode_bytes(image)?;
        fs::write(path, output).map_err(|error| {
            ImageIoError::File(format!("failed to write {}: {error}", path.display()))
        })
    }
}

/// Strict raw RGB8 validation errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RawRgb8Error {
    MissingDimensions,
    InvalidDimensions(SpecError),
    ByteCountOverflow,
    TruncatedData { expected: usize, actual: usize },
    UnexpectedTrailingBytes { expected: usize, actual: usize },
    AllocationFailed,
    InvalidImage(ImageError),
}

impl fmt::Display for RawRgb8Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDimensions => formatter.write_str("explicit dimensions are required"),
            Self::InvalidDimensions(error) => write!(formatter, "invalid dimensions: {error}"),
            Self::ByteCountOverflow => formatter.write_str("raw RGB8 byte count overflow"),
            Self::TruncatedData { expected, actual } => write!(
                formatter,
                "truncated data: expected {expected} bytes, received {actual}"
            ),
            Self::UnexpectedTrailingBytes { expected, actual } => write!(
                formatter,
                "unexpected trailing bytes: expected {expected} bytes, received {actual}"
            ),
            Self::AllocationFailed => formatter.write_str("raw RGB8 allocation failed"),
            Self::InvalidImage(error) => write!(formatter, "invalid decoded image: {error}"),
        }
    }
}

impl std::error::Error for RawRgb8Error {}

#[cfg(test)]
mod tests {
    use super::{RawRgb8Codec, RawRgb8Error};
    use crate::image::{Image, Rgb8};
    use crate::io::{DecodeSpec, ImageDecoder, ImageEncoder, ImageFormat, ImageIoError};
    use crate::spec::Dimensions;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn dimensions(width: u32, height: u32) -> Dimensions {
        Dimensions::new(width, height).unwrap()
    }

    fn temporary_path() -> PathBuf {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "verisilicon_sr_raw_{}_{}.raw",
            std::process::id(),
            sequence
        ))
    }

    #[test]
    fn byte_codec_round_trips_exact_packed_rgb() {
        let source = [1, 2, 3, 4, 5, 6];
        let codec = RawRgb8Codec::new();
        let image = codec.decode_bytes(&source, dimensions(2, 1)).unwrap();
        assert_eq!(image.pixels(), &[Rgb8::new(1, 2, 3), Rgb8::new(4, 5, 6)]);
        assert_eq!(codec.encode_bytes(&image).unwrap(), source);
    }

    #[test]
    fn rejects_truncated_and_trailing_data() {
        let codec = RawRgb8Codec::new();
        assert_eq!(
            codec.decode_bytes(&[1, 2], dimensions(1, 1)),
            Err(RawRgb8Error::TruncatedData {
                expected: 3,
                actual: 2
            })
        );
        assert_eq!(
            codec.decode_bytes(&[1, 2, 3, 4], dimensions(1, 1)),
            Err(RawRgb8Error::UnexpectedTrailingBytes {
                expected: 3,
                actual: 4
            })
        );
    }

    #[test]
    fn file_adapter_requires_dimensions_and_round_trips() {
        let path = temporary_path();
        let codec = RawRgb8Codec::new();
        let image = Image::new(dimensions(1, 1), vec![Rgb8::new(7, 8, 9)]).unwrap();
        codec.encode(&path, ImageFormat::RawRgb8, &image).unwrap();
        assert_eq!(fs::read(&path).unwrap(), [7, 8, 9]);
        let decoded = codec
            .decode(
                &path,
                DecodeSpec {
                    format: ImageFormat::RawRgb8,
                    dimensions: Some(dimensions(1, 1)),
                },
            )
            .unwrap();
        assert_eq!(decoded, image);
        assert_eq!(
            codec.decode(
                &path,
                DecodeSpec {
                    format: ImageFormat::RawRgb8,
                    dimensions: None,
                }
            ),
            Err(ImageIoError::Raw(RawRgb8Error::MissingDimensions))
        );
        fs::remove_file(path).unwrap();
    }
}
