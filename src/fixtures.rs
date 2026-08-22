//! Deterministic synthetic RGB8 fixtures for diagnostics and regression tests.

use crate::image::{Image, Rgb8};
use crate::spec::{Dimensions, SpecError};
use std::fmt;

/// Orientation of a synthetic hard edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HardEdge {
    Horizontal,
    Vertical,
}

/// Errors produced while generating synthetic fixtures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureError {
    InvalidDimensions(SpecError),
    InvalidCellSize,
    AllocationFailed,
}

impl fmt::Display for FixtureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions(error) => {
                write!(formatter, "invalid fixture dimensions: {error}")
            }
            Self::InvalidCellSize => formatter.write_str("checker cell size must be positive"),
            Self::AllocationFailed => formatter.write_str("fixture allocation failed"),
        }
    }
}

impl std::error::Error for FixtureError {}

pub fn constant(dimensions: Dimensions, value: Rgb8) -> Result<Image, FixtureError> {
    generate(dimensions, |_, _| value)
}

pub fn smooth_gradient(dimensions: Dimensions) -> Result<Image, FixtureError> {
    let width = dimensions.width();
    let height = dimensions.height();
    generate(dimensions, |x, y| {
        let red = ramp(x, width);
        let green = ramp(y, height);
        let blue = ((u16::from(red) + u16::from(green) + 1) / 2) as u8;
        Rgb8::new(red, green, blue)
    })
}

pub fn hard_edge(dimensions: Dimensions, edge: HardEdge) -> Result<Image, FixtureError> {
    let width = dimensions.width();
    let height = dimensions.height();
    generate(dimensions, |x, y| {
        let bright = match edge {
            HardEdge::Horizontal => y >= height / 2,
            HardEdge::Vertical => x >= width / 2,
        };
        if bright {
            Rgb8::new(255, 255, 255)
        } else {
            Rgb8::new(0, 0, 0)
        }
    })
}

pub fn checker_detail(dimensions: Dimensions, cell_size: u32) -> Result<Image, FixtureError> {
    if cell_size == 0 {
        return Err(FixtureError::InvalidCellSize);
    }
    generate(dimensions, |x, y| {
        if ((x / cell_size) + (y / cell_size)) & 1 == 0 {
            Rgb8::new(24, 48, 72)
        } else {
            Rgb8::new(232, 208, 184)
        }
    })
}

fn generate<F>(dimensions: Dimensions, mut pixel: F) -> Result<Image, FixtureError>
where
    F: FnMut(u32, u32) -> Rgb8,
{
    let count = dimensions
        .pixel_count()
        .map_err(FixtureError::InvalidDimensions)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| FixtureError::AllocationFailed)?;
    for y in 0..dimensions.height() {
        for x in 0..dimensions.width() {
            pixels.push(pixel(x, y));
        }
    }
    Image::new(dimensions, pixels).map_err(|_| FixtureError::AllocationFailed)
}

fn ramp(position: u32, length: u32) -> u8 {
    if length == 1 {
        return 128;
    }
    let denominator = u64::from(length - 1);
    let numerator = u64::from(position) * 255 + denominator / 2;
    (numerator / denominator) as u8
}

#[cfg(test)]
mod tests {
    use super::{FixtureError, HardEdge, checker_detail, constant, hard_edge, smooth_gradient};
    use crate::image::Rgb8;
    use crate::spec::Dimensions;

    fn dimensions(width: u32, height: u32) -> Dimensions {
        Dimensions::new(width, height).unwrap()
    }

    #[test]
    fn constant_fixture_is_exact() {
        let fixture = constant(dimensions(2, 2), Rgb8::new(7, 8, 9)).unwrap();
        assert_eq!(fixture.pixels(), vec![Rgb8::new(7, 8, 9); 4]);
    }

    #[test]
    fn smooth_gradient_has_fixed_corner_vectors() {
        let fixture = smooth_gradient(dimensions(3, 3)).unwrap();
        assert_eq!(fixture.pixels()[0], Rgb8::new(0, 0, 0));
        assert_eq!(fixture.pixels()[2], Rgb8::new(255, 0, 128));
        assert_eq!(fixture.pixels()[6], Rgb8::new(0, 255, 128));
        assert_eq!(fixture.pixels()[8], Rgb8::new(255, 255, 255));
    }

    #[test]
    fn hard_edges_have_fixed_halves() {
        let vertical = hard_edge(dimensions(4, 2), HardEdge::Vertical).unwrap();
        assert_eq!(vertical.pixels()[1], Rgb8::new(0, 0, 0));
        assert_eq!(vertical.pixels()[2], Rgb8::new(255, 255, 255));
        let horizontal = hard_edge(dimensions(2, 4), HardEdge::Horizontal).unwrap();
        assert_eq!(horizontal.pixels()[2], Rgb8::new(0, 0, 0));
        assert_eq!(horizontal.pixels()[4], Rgb8::new(255, 255, 255));
    }

    #[test]
    fn checker_is_deterministic_and_rejects_zero_cell_size() {
        let first = checker_detail(dimensions(4, 2), 1).unwrap();
        let second = checker_detail(dimensions(4, 2), 1).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            checker_detail(dimensions(2, 2), 0),
            Err(FixtureError::InvalidCellSize)
        );
    }
}
