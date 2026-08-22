//! Strict dependency-free PPM P6 decoding and encoding.

use super::{DecodeSpec, ImageDecoder, ImageEncoder, ImageFormat, ImageIoError};
use crate::image::{Image, ImageError, Rgb8};
use crate::spec::{Dimensions, SpecError};
use std::fmt;
use std::fs;
use std::path::Path;

/// Stateless PPM P6 byte and file codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct PpmP6Codec;

impl PpmP6Codec {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Decodes a complete PPM P6 file held in memory.
    pub fn decode_bytes(self, input: &[u8]) -> Result<Image, PpmError> {
        let mut cursor = HeaderCursor::new(input);
        if cursor.next_token("magic")? != b"P6" {
            return Err(PpmError::InvalidMagic);
        }

        let width = parse_decimal(cursor.next_token("width")?, "width")?;
        let height = parse_decimal(cursor.next_token("height")?, "height")?;
        let max_value = parse_decimal(cursor.next_token("maxval")?, "maxval")?;
        if max_value != 255 {
            return Err(PpmError::UnsupportedMaxValue(max_value));
        }

        let dimensions = Dimensions::new(width, height).map_err(PpmError::InvalidDimensions)?;
        let pixel_count = dimensions
            .pixel_count()
            .map_err(PpmError::InvalidDimensions)?;
        let raster_byte_count = pixel_count
            .checked_mul(3)
            .ok_or(PpmError::RasterSizeOverflow)?;
        let raster_start = cursor.consume_raster_separator()?;
        let raster = &input[raster_start..];

        if raster.len() < raster_byte_count {
            return Err(PpmError::TruncatedRaster {
                expected: raster_byte_count,
                actual: raster.len(),
            });
        }
        if raster.len() > raster_byte_count {
            return Err(PpmError::UnexpectedTrailingBytes {
                expected: raster_byte_count,
                actual: raster.len(),
            });
        }

        let pixels = raster
            .chunks_exact(3)
            .map(|pixel| Rgb8::new(pixel[0], pixel[1], pixel[2]))
            .collect();
        Image::new(dimensions, pixels).map_err(PpmError::InvalidImage)
    }

    /// Encodes an image using one deterministic representation.
    pub fn encode_bytes(self, image: &Image) -> Result<Vec<u8>, PpmError> {
        let dimensions = image.dimensions();
        let raster_byte_count = dimensions
            .pixel_count()
            .map_err(PpmError::InvalidDimensions)?
            .checked_mul(3)
            .ok_or(PpmError::RasterSizeOverflow)?;
        let header = format!("P6\n{} {}\n255\n", dimensions.width(), dimensions.height());
        let capacity = header
            .len()
            .checked_add(raster_byte_count)
            .ok_or(PpmError::RasterSizeOverflow)?;
        let mut output = Vec::with_capacity(capacity);
        output.extend_from_slice(header.as_bytes());
        for pixel in image.pixels() {
            output.extend_from_slice(&[pixel.red, pixel.green, pixel.blue]);
        }
        Ok(output)
    }
}

impl ImageDecoder for PpmP6Codec {
    fn decode(&self, path: &Path, spec: DecodeSpec) -> Result<Image, ImageIoError> {
        if spec.format != ImageFormat::PpmP6 {
            return Err(ImageIoError::UnsupportedFormat(spec.format));
        }
        let input = fs::read(path).map_err(|error| {
            ImageIoError::File(format!("failed to read {}: {error}", path.display()))
        })?;
        let image = self.decode_bytes(&input)?;
        if let Some(expected) = spec.dimensions
            && image.dimensions() != expected
        {
            return Err(PpmError::DimensionMismatch {
                expected,
                actual: image.dimensions(),
            }
            .into());
        }
        Ok(image)
    }
}

impl ImageEncoder for PpmP6Codec {
    fn encode(&self, path: &Path, format: ImageFormat, image: &Image) -> Result<(), ImageIoError> {
        if format != ImageFormat::PpmP6 {
            return Err(ImageIoError::UnsupportedFormat(format));
        }
        let output = self.encode_bytes(image)?;
        fs::write(path, output).map_err(|error| {
            ImageIoError::File(format!("failed to write {}: {error}", path.display()))
        })
    }
}

/// Strict PPM P6 validation errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PpmError {
    InvalidMagic,
    MissingToken(&'static str),
    InvalidDecimal(&'static str),
    NumericOverflow(&'static str),
    UnsupportedMaxValue(u32),
    MissingRasterSeparator,
    InvalidDimensions(SpecError),
    RasterSizeOverflow,
    TruncatedRaster {
        expected: usize,
        actual: usize,
    },
    UnexpectedTrailingBytes {
        expected: usize,
        actual: usize,
    },
    DimensionMismatch {
        expected: Dimensions,
        actual: Dimensions,
    },
    InvalidImage(ImageError),
}

impl fmt::Display for PpmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => formatter.write_str("magic token must be P6"),
            Self::MissingToken(field) => write!(formatter, "missing {field} token"),
            Self::InvalidDecimal(field) => {
                write!(
                    formatter,
                    "{field} token must contain only ASCII decimal digits"
                )
            }
            Self::NumericOverflow(field) => write!(formatter, "{field} value is too large"),
            Self::UnsupportedMaxValue(value) => {
                write!(formatter, "maxval must be 255, received {value}")
            }
            Self::MissingRasterSeparator => {
                formatter.write_str("maxval must be followed by one whitespace separator")
            }
            Self::InvalidDimensions(error) => write!(formatter, "invalid dimensions: {error}"),
            Self::RasterSizeOverflow => formatter.write_str("raster byte count overflow"),
            Self::TruncatedRaster { expected, actual } => write!(
                formatter,
                "truncated raster: expected {expected} bytes, received {actual}"
            ),
            Self::UnexpectedTrailingBytes { expected, actual } => write!(
                formatter,
                "unexpected trailing bytes: expected {expected} raster bytes, received {actual}"
            ),
            Self::DimensionMismatch { expected, actual } => write!(
                formatter,
                "dimension mismatch: expected {} by {}, received {} by {}",
                expected.width(),
                expected.height(),
                actual.width(),
                actual.height()
            ),
            Self::InvalidImage(error) => write!(formatter, "invalid decoded image: {error}"),
        }
    }
}

impl std::error::Error for PpmError {}

struct HeaderCursor<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> HeaderCursor<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, position: 0 }
    }

    fn next_token(&mut self, field: &'static str) -> Result<&'a [u8], PpmError> {
        self.skip_header_separators();
        let start = self.position;
        while let Some(&byte) = self.input.get(self.position) {
            if is_ascii_whitespace(byte) || byte == b'#' {
                break;
            }
            self.position += 1;
        }
        if self.position == start {
            return Err(PpmError::MissingToken(field));
        }
        Ok(&self.input[start..self.position])
    }

    fn skip_header_separators(&mut self) {
        loop {
            while self
                .input
                .get(self.position)
                .is_some_and(|byte| is_ascii_whitespace(*byte))
            {
                self.position += 1;
            }
            if self.input.get(self.position) != Some(&b'#') {
                return;
            }
            while let Some(&byte) = self.input.get(self.position) {
                self.position += 1;
                if byte == b'\n' || byte == b'\r' {
                    break;
                }
            }
        }
    }

    fn consume_raster_separator(&self) -> Result<usize, PpmError> {
        let first = *self
            .input
            .get(self.position)
            .ok_or(PpmError::MissingRasterSeparator)?;
        if !is_ascii_whitespace(first) {
            return Err(PpmError::MissingRasterSeparator);
        }
        if first == b'\r' && self.input.get(self.position + 1) == Some(&b'\n') {
            Ok(self.position + 2)
        } else {
            Ok(self.position + 1)
        }
    }
}

const fn is_ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

fn parse_decimal(token: &[u8], field: &'static str) -> Result<u32, PpmError> {
    if token.is_empty() || !token.iter().all(u8::is_ascii_digit) {
        return Err(PpmError::InvalidDecimal(field));
    }
    token.iter().try_fold(0_u32, |value, byte| {
        value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u32::from(byte - b'0')))
            .ok_or(PpmError::NumericOverflow(field))
    })
}

#[cfg(test)]
mod tests {
    use super::{PpmError, PpmP6Codec};
    use crate::image::{Image, Rgb8};
    use crate::io::{DecodeSpec, ImageDecoder, ImageEncoder, ImageFormat, ImageIoError};
    use crate::spec::{Dimensions, SpecError};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn image(width: u32, height: u32, pixels: Vec<Rgb8>) -> Image {
        Image::new(Dimensions::new(width, height).unwrap(), pixels).unwrap()
    }

    fn ppm(header: &[u8], raster: &[u8]) -> Vec<u8> {
        let mut data = header.to_vec();
        data.extend_from_slice(raster);
        data
    }

    fn temporary_path(label: &str) -> PathBuf {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "verisilicon_sr_{}_{}_{}.ppm",
            std::process::id(),
            sequence,
            label
        ))
    }

    #[test]
    fn decodes_comments_and_header_whitespace() {
        let data = ppm(
            b"P6\n# first comment\r\n2\t# dimensions\n1\n255\n",
            &[1, 2, 3, 4, 5, 6],
        );
        let decoded = PpmP6Codec::new().decode_bytes(&data).unwrap();
        assert_eq!(decoded.dimensions(), Dimensions::new(2, 1).unwrap());
        assert_eq!(decoded.pixels(), &[Rgb8::new(1, 2, 3), Rgb8::new(4, 5, 6)]);
    }

    #[test]
    fn preserves_leading_raster_whitespace_and_hash() {
        let whitespace = ppm(b"P6\n1 1\n255\n", b" \n\t");
        assert_eq!(
            PpmP6Codec::new()
                .decode_bytes(&whitespace)
                .unwrap()
                .pixels(),
            &[Rgb8::new(b' ', b'\n', b'\t')]
        );
        let hash = ppm(b"P6\n1 1\n255\n", &[b'#', 8, 9]);
        assert_eq!(
            PpmP6Codec::new().decode_bytes(&hash).unwrap().pixels(),
            &[Rgb8::new(b'#', 8, 9)]
        );
    }

    #[test]
    fn treats_crlf_as_one_raster_separator() {
        let data = ppm(b"P6\r\n1 1\r\n255\r\n", &[b' ', b'#', 7]);
        assert_eq!(
            PpmP6Codec::new().decode_bytes(&data).unwrap().pixels(),
            &[Rgb8::new(b' ', b'#', 7)]
        );
    }

    #[test]
    fn encodes_deterministically() {
        let source = image(2, 1, vec![Rgb8::new(1, 2, 3), Rgb8::new(4, 5, 6)]);
        let expected = ppm(b"P6\n2 1\n255\n", &[1, 2, 3, 4, 5, 6]);
        let codec = PpmP6Codec::new();
        assert_eq!(codec.encode_bytes(&source).unwrap(), expected);
        assert_eq!(codec.encode_bytes(&source).unwrap(), expected);
    }

    #[test]
    fn rejects_malformed_headers() {
        let codec = PpmP6Codec::new();
        assert_eq!(
            codec.decode_bytes(b"P3\n1 1\n255\n\0\0\0"),
            Err(PpmError::InvalidMagic)
        );
        assert_eq!(
            codec.decode_bytes(b"P6\n1 x\n255\n\0\0\0"),
            Err(PpmError::InvalidDecimal("height"))
        );
        assert_eq!(
            codec.decode_bytes(b"P6\n1 1\n256\n\0\0\0"),
            Err(PpmError::UnsupportedMaxValue(256))
        );
        assert_eq!(
            codec.decode_bytes(b"P6\n1 1\n255"),
            Err(PpmError::MissingRasterSeparator)
        );
    }

    #[test]
    fn rejects_zero_overflow_and_bad_lengths() {
        let codec = PpmP6Codec::new();
        assert_eq!(
            codec.decode_bytes(b"P6\n0 1\n255\n"),
            Err(PpmError::InvalidDimensions(SpecError::ZeroDimension))
        );
        assert!(matches!(
            codec.decode_bytes(b"P6\n4294967295 4294967295\n255\n"),
            Err(PpmError::InvalidDimensions(SpecError::SizeOverflow))
                | Err(PpmError::RasterSizeOverflow)
        ));
        assert_eq!(
            codec.decode_bytes(b"P6\n4294967296 1\n255\n"),
            Err(PpmError::NumericOverflow("width"))
        );
        assert_eq!(
            codec.decode_bytes(b"P6\n1 1\n255\n\x01\x02"),
            Err(PpmError::TruncatedRaster {
                expected: 3,
                actual: 2
            })
        );
        assert_eq!(
            codec.decode_bytes(b"P6\n1 1\n255\n\x01\x02\x03\x04"),
            Err(PpmError::UnexpectedTrailingBytes {
                expected: 3,
                actual: 4
            })
        );
    }

    #[test]
    fn file_adapter_round_trips_and_checks_spec() {
        let input = image(1, 2, vec![Rgb8::new(0, 1, 2), Rgb8::new(253, 254, 255)]);
        let path = temporary_path("round_trip");
        let codec = PpmP6Codec::new();
        codec.encode(&path, ImageFormat::PpmP6, &input).unwrap();
        let decoded = codec
            .decode(
                &path,
                DecodeSpec {
                    format: ImageFormat::PpmP6,
                    dimensions: Some(Dimensions::new(1, 2).unwrap()),
                },
            )
            .unwrap();
        assert_eq!(decoded, input);
        assert!(matches!(
            codec.decode(
                &path,
                DecodeSpec {
                    format: ImageFormat::PpmP6,
                    dimensions: Some(Dimensions::new(2, 1).unwrap())
                }
            ),
            Err(ImageIoError::Ppm(PpmError::DimensionMismatch { .. }))
        ));
        assert_eq!(
            codec.decode(
                &path,
                DecodeSpec {
                    format: ImageFormat::RawRgb8,
                    dimensions: None
                }
            ),
            Err(ImageIoError::UnsupportedFormat(ImageFormat::RawRgb8))
        );
        fs::remove_file(path).unwrap();
    }
}
