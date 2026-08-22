//! Owned RGB8 image types.

use crate::spec::{Dimensions, SpecError};
use std::fmt;

/// One packed RGB8 pixel.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct Rgb8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb8 {
    #[must_use]
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

/// An owned row-major RGB8 image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Image {
    dimensions: Dimensions,
    pixels: Vec<Rgb8>,
}

impl Image {
    /// Creates an image when the pixel count exactly matches its dimensions.
    pub fn new(dimensions: Dimensions, pixels: Vec<Rgb8>) -> Result<Self, ImageError> {
        let expected = dimensions
            .pixel_count()
            .map_err(ImageError::InvalidDimensions)?;
        let actual = pixels.len();
        if actual != expected {
            return Err(ImageError::PixelCountMismatch { expected, actual });
        }
        Ok(Self { dimensions, pixels })
    }

    #[must_use]
    pub const fn dimensions(&self) -> Dimensions {
        self.dimensions
    }

    #[must_use]
    pub fn pixels(&self) -> &[Rgb8] {
        &self.pixels
    }

    #[must_use]
    pub fn into_pixels(self) -> Vec<Rgb8> {
        self.pixels
    }
}

/// Errors produced while constructing an image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImageError {
    InvalidDimensions(SpecError),
    PixelCountMismatch { expected: usize, actual: usize },
}

impl fmt::Display for ImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions(error) => {
                write!(formatter, "invalid image dimensions: {error}")
            }
            Self::PixelCountMismatch { expected, actual } => write!(
                formatter,
                "pixel count mismatch: expected {expected}, received {actual}"
            ),
        }
    }
}

impl std::error::Error for ImageError {}

#[cfg(test)]
mod tests {
    use super::{Image, ImageError, Rgb8};
    use crate::spec::Dimensions;

    #[test]
    fn image_accepts_exact_pixel_count() {
        let dimensions = Dimensions::new(2, 1).expect("valid dimensions");
        let pixels = vec![Rgb8::new(1, 2, 3), Rgb8::new(4, 5, 6)];
        let image = Image::new(dimensions, pixels.clone()).expect("valid image");
        assert_eq!(image.dimensions(), dimensions);
        assert_eq!(image.pixels(), pixels);
    }

    #[test]
    fn image_rejects_incorrect_pixel_count() {
        let dimensions = Dimensions::new(2, 2).expect("valid dimensions");
        let error = Image::new(dimensions, vec![Rgb8::default()]).unwrap_err();
        assert_eq!(
            error,
            ImageError::PixelCountMismatch {
                expected: 4,
                actual: 1
            }
        );
    }
}
