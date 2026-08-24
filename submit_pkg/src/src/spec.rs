//! Checked image dimensions and processing configuration.

use std::fmt;

/// Organizer-confirmed width of official packed RGB888 input.
pub const OFFICIAL_RAW_INPUT_WIDTH: u32 = 1920;
/// Organizer-confirmed height of official packed RGB888 input.
pub const OFFICIAL_RAW_INPUT_HEIGHT: u32 = 1080;
/// Exact byte count of official packed RGB888 input.
pub const OFFICIAL_RAW_INPUT_BYTE_COUNT: usize = 6_220_800;
/// Exact byte count of 2x official packed RGB888 output.
pub const OFFICIAL_RAW_OUTPUT_BYTE_COUNT: usize = 24_883_200;

/// Returns the fixed official packed RGB888 input dimensions.
#[must_use]
pub const fn official_raw_input_dimensions() -> Dimensions {
    Dimensions {
        width: OFFICIAL_RAW_INPUT_WIDTH,
        height: OFFICIAL_RAW_INPUT_HEIGHT,
    }
}

/// The public scale supported by the software contest scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scale {
    X2,
}

impl Scale {
    /// Returns the integer scale factor.
    #[must_use]
    pub const fn factor(self) -> u32 {
        match self {
            Self::X2 => 2,
        }
    }
}

/// Positive image dimensions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dimensions {
    width: u32,
    height: u32,
}

impl Dimensions {
    /// Creates dimensions after checking that both values are positive.
    pub const fn new(width: u32, height: u32) -> Result<Self, SpecError> {
        if width == 0 || height == 0 {
            return Err(SpecError::ZeroDimension);
        }
        Ok(Self { width, height })
    }

    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }

    /// Returns the number of pixels after checking the host `usize` range.
    pub fn pixel_count(self) -> Result<usize, SpecError> {
        let width = usize::try_from(self.width).map_err(|_| SpecError::SizeOverflow)?;
        let height = usize::try_from(self.height).map_err(|_| SpecError::SizeOverflow)?;
        width.checked_mul(height).ok_or(SpecError::SizeOverflow)
    }

    /// Returns dimensions scaled by the selected factor.
    pub fn scaled(self, scale: Scale) -> Result<Self, SpecError> {
        let factor = scale.factor();
        let width = self
            .width
            .checked_mul(factor)
            .ok_or(SpecError::SizeOverflow)?;
        let height = self
            .height
            .checked_mul(factor)
            .ok_or(SpecError::SizeOverflow)?;
        Self::new(width, height)
    }
}

/// Validated processing configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessingConfig {
    input_dimensions: Dimensions,
    scale: Scale,
}

impl ProcessingConfig {
    #[must_use]
    pub const fn new(input_dimensions: Dimensions) -> Self {
        Self {
            input_dimensions,
            scale: Scale::X2,
        }
    }

    #[must_use]
    pub const fn input_dimensions(self) -> Dimensions {
        self.input_dimensions
    }

    #[must_use]
    pub const fn scale(self) -> Scale {
        self.scale
    }

    pub fn output_dimensions(self) -> Result<Dimensions, SpecError> {
        self.input_dimensions.scaled(self.scale)
    }
}

/// Errors produced while validating image specifications.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpecError {
    ZeroDimension,
    SizeOverflow,
}

impl fmt::Display for SpecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDimension => formatter.write_str("image dimensions must be positive"),
            Self::SizeOverflow => formatter.write_str("image dimensions exceed supported limits"),
        }
    }
}

impl std::error::Error for SpecError {}

#[cfg(test)]
mod tests {
    use super::{
        Dimensions, OFFICIAL_RAW_INPUT_BYTE_COUNT, OFFICIAL_RAW_OUTPUT_BYTE_COUNT,
        ProcessingConfig, Scale, SpecError, official_raw_input_dimensions,
    };

    #[test]
    fn dimensions_reject_zero_values() {
        assert_eq!(Dimensions::new(0, 1), Err(SpecError::ZeroDimension));
        assert_eq!(Dimensions::new(1, 0), Err(SpecError::ZeroDimension));
    }

    #[test]
    fn dimensions_report_pixel_count() {
        let dimensions = Dimensions::new(1920, 1080).expect("valid dimensions");
        assert_eq!(dimensions.pixel_count(), Ok(2_073_600));
    }

    #[test]
    fn scale_x2_checks_output_dimensions() {
        let input = Dimensions::new(1920, 1080).expect("valid dimensions");
        let output = input.scaled(Scale::X2).expect("valid scaled dimensions");
        assert_eq!(output, Dimensions::new(3840, 2160).unwrap());
    }

    #[test]
    fn scale_x2_rejects_overflow() {
        let input = Dimensions::new(u32::MAX, 1).expect("valid dimensions");
        assert_eq!(input.scaled(Scale::X2), Err(SpecError::SizeOverflow));
    }

    #[test]
    fn processing_config_uses_x2() {
        let input = Dimensions::new(4, 3).expect("valid dimensions");
        let config = ProcessingConfig::new(input);
        assert_eq!(config.input_dimensions(), input);
        assert_eq!(config.scale(), Scale::X2);
        assert_eq!(config.output_dimensions(), Dimensions::new(8, 6));
    }

    #[test]
    fn official_raw_geometry_and_byte_counts_are_exact() {
        let input = official_raw_input_dimensions();
        assert_eq!(input, Dimensions::new(1920, 1080).unwrap());
        assert_eq!(
            input.pixel_count().unwrap() * 3,
            OFFICIAL_RAW_INPUT_BYTE_COUNT
        );
        assert_eq!(
            input.scaled(Scale::X2).unwrap().pixel_count().unwrap() * 3,
            OFFICIAL_RAW_OUTPUT_BYTE_COUNT
        );
    }
}
