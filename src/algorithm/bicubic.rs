//! Scalar separable 2x Catmull-Rom bicubic baseline.

use super::color::{YCbCr8, rgb_to_ycbcr, ycbcr_to_rgb};
use super::{AlgorithmError, SuperResolution};
use crate::image::{Image, Rgb8};
use crate::spec::{Dimensions, ProcessingConfig, Scale};

/// Q7 weights for even output coordinates at source phase 0.75.
///
/// The taps are source offsets `[-2, -1, 0, 1]` from `output / 2`.
pub const EVEN_PHASE_WEIGHTS: [i32; 4] = [-3, 29, 111, -9];

/// Q7 weights for odd output coordinates at source phase 0.25.
///
/// The taps are source offsets `[-1, 0, 1, 2]` from `output / 2`.
pub const ODD_PHASE_WEIGHTS: [i32; 4] = [-9, 111, 29, -3];

const WEIGHT_SHIFT: u32 = 7;
const COMBINED_SHIFT: u32 = WEIGHT_SHIFT * 2;

/// Dependency-free scalar baseline using full-range YCbCr planes.
#[derive(Clone, Copy, Debug, Default)]
pub struct BicubicBaseline;

impl BicubicBaseline {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SuperResolution for BicubicBaseline {
    fn process(&self, input: &Image, config: ProcessingConfig) -> Result<Image, AlgorithmError> {
        if input.dimensions() != config.input_dimensions() {
            return Err(AlgorithmError::DimensionMismatch {
                expected: config.input_dimensions(),
                actual: input.dimensions(),
            });
        }
        if config.scale() != Scale::X2 {
            return Err(AlgorithmError::InvalidConfiguration(
                "bicubic baseline requires 2x scale",
            ));
        }

        let input_count = input
            .dimensions()
            .pixel_count()
            .map_err(AlgorithmError::InvalidDimensions)?;
        let mut y_plane = reserve_u8(input_count)?;
        let mut cb_plane = reserve_u8(input_count)?;
        let mut cr_plane = reserve_u8(input_count)?;
        for &pixel in input.pixels() {
            let converted = rgb_to_ycbcr(pixel);
            y_plane.push(converted.y);
            cb_plane.push(converted.cb);
            cr_plane.push(converted.cr);
        }

        let dimensions = input.dimensions();
        let y_scaled = scale_plane_2x(&y_plane, dimensions)?;
        let cb_scaled = scale_plane_2x(&cb_plane, dimensions)?;
        let cr_scaled = scale_plane_2x(&cr_plane, dimensions)?;
        let output_dimensions = config
            .output_dimensions()
            .map_err(AlgorithmError::InvalidDimensions)?;
        let output_count = output_dimensions
            .pixel_count()
            .map_err(AlgorithmError::InvalidDimensions)?;
        let mut pixels = reserve_rgb8(output_count)?;
        for index in 0..output_count {
            pixels.push(ycbcr_to_rgb(YCbCr8::new(
                y_scaled[index],
                cb_scaled[index],
                cr_scaled[index],
            )));
        }
        Image::new(output_dimensions, pixels)
            .map_err(|_| AlgorithmError::ProcessingFailed("invalid output image"))
    }
}

/// Scales one 8-bit plane using half-pixel mapping and Catmull-Rom `a = -0.5`.
///
/// Horizontal results remain signed Q7 values. The vertical pass applies the
/// second Q7 phase, rounds the combined Q14 value to nearest with halves away
/// from zero, then clips only the final sample to 0 through 255.
pub fn scale_plane_2x(input: &[u8], dimensions: Dimensions) -> Result<Vec<u8>, AlgorithmError> {
    let expected = dimensions
        .pixel_count()
        .map_err(AlgorithmError::InvalidDimensions)?;
    if input.len() != expected {
        return Err(AlgorithmError::InvalidPlaneLength {
            expected,
            actual: input.len(),
        });
    }

    let output_dimensions = dimensions
        .scaled(Scale::X2)
        .map_err(AlgorithmError::InvalidDimensions)?;
    let input_width = usize::try_from(dimensions.width())
        .map_err(|_| AlgorithmError::InvalidDimensions(crate::spec::SpecError::SizeOverflow))?;
    let input_height = usize::try_from(dimensions.height())
        .map_err(|_| AlgorithmError::InvalidDimensions(crate::spec::SpecError::SizeOverflow))?;
    let output_width = usize::try_from(output_dimensions.width())
        .map_err(|_| AlgorithmError::InvalidDimensions(crate::spec::SpecError::SizeOverflow))?;
    let output_height = usize::try_from(output_dimensions.height())
        .map_err(|_| AlgorithmError::InvalidDimensions(crate::spec::SpecError::SizeOverflow))?;
    let intermediate_count =
        output_width
            .checked_mul(input_height)
            .ok_or(AlgorithmError::InvalidDimensions(
                crate::spec::SpecError::SizeOverflow,
            ))?;
    let output_count = output_dimensions
        .pixel_count()
        .map_err(AlgorithmError::InvalidDimensions)?;
    let mut horizontal = zeroed_i32(intermediate_count)?;

    for y in 0..input_height {
        for x_out in 0..output_width {
            let (offsets, weights) = phase(x_out);
            let base = x_out / 2;
            let mut sum = 0_i32;
            for tap in 0..4 {
                let x = clamped_index(base, offsets[tap], input_width);
                sum += i32::from(input[y * input_width + x]) * weights[tap];
            }
            horizontal[y * output_width + x_out] = sum;
        }
    }

    let mut output = zeroed_u8(output_count)?;
    for y_out in 0..output_height {
        let (offsets, weights) = phase(y_out);
        let base = y_out / 2;
        for x in 0..output_width {
            let mut sum = 0_i64;
            for tap in 0..4 {
                let y = clamped_index(base, offsets[tap], input_height);
                sum += i64::from(horizontal[y * output_width + x]) * i64::from(weights[tap]);
            }
            output[y_out * output_width + x] = clip_u8(round_q14(sum));
        }
    }
    Ok(output)
}

fn phase(coordinate: usize) -> ([isize; 4], [i32; 4]) {
    if coordinate & 1 == 0 {
        ([-2, -1, 0, 1], EVEN_PHASE_WEIGHTS)
    } else {
        ([-1, 0, 1, 2], ODD_PHASE_WEIGHTS)
    }
}

fn clamped_index(base: usize, offset: isize, length: usize) -> usize {
    let index = base.saturating_add_signed(offset);
    index.min(length - 1)
}

fn round_q14(value: i64) -> i64 {
    let half = 1_i64 << (COMBINED_SHIFT - 1);
    if value >= 0 {
        (value + half) >> COMBINED_SHIFT
    } else {
        -((-value + half) >> COMBINED_SHIFT)
    }
}

fn clip_u8(value: i64) -> u8 {
    value.clamp(0, 255) as u8
}

fn reserve_u8(capacity: usize) -> Result<Vec<u8>, AlgorithmError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| AlgorithmError::AllocationFailed)?;
    Ok(values)
}

fn reserve_rgb8(capacity: usize) -> Result<Vec<Rgb8>, AlgorithmError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| AlgorithmError::AllocationFailed)?;
    Ok(values)
}

fn zeroed_i32(length: usize) -> Result<Vec<i32>, AlgorithmError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .map_err(|_| AlgorithmError::AllocationFailed)?;
    values.resize(length, 0);
    Ok(values)
}

fn zeroed_u8(length: usize) -> Result<Vec<u8>, AlgorithmError> {
    let mut values = reserve_u8(length)?;
    values.resize(length, 0);
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::{BicubicBaseline, EVEN_PHASE_WEIGHTS, ODD_PHASE_WEIGHTS, scale_plane_2x};
    use crate::algorithm::{AlgorithmError, SuperResolution};
    use crate::image::{Image, Rgb8};
    use crate::spec::{Dimensions, ProcessingConfig};

    fn dimensions(width: u32, height: u32) -> Dimensions {
        Dimensions::new(width, height).unwrap()
    }

    #[test]
    fn phase_weights_are_exact_and_normalized() {
        assert_eq!(EVEN_PHASE_WEIGHTS, [-3, 29, 111, -9]);
        assert_eq!(ODD_PHASE_WEIGHTS, [-9, 111, 29, -3]);
        assert_eq!(EVEN_PHASE_WEIGHTS.iter().sum::<i32>(), 128);
        assert_eq!(ODD_PHASE_WEIGHTS.iter().sum::<i32>(), 128);
    }

    #[test]
    fn constant_plane_and_one_by_one_borders_are_preserved() {
        assert_eq!(
            scale_plane_2x(&[73], dimensions(1, 1)).unwrap(),
            vec![73; 4]
        );
        assert_eq!(
            scale_plane_2x(&[19, 19, 19, 19], dimensions(2, 2)).unwrap(),
            vec![19; 16]
        );
    }

    #[test]
    fn gradient_and_impulse_vectors_are_fixed() {
        assert_eq!(
            scale_plane_2x(&[0, 255], dimensions(2, 1)).unwrap(),
            vec![0, 52, 203, 255, 0, 52, 203, 255]
        );
        assert_eq!(
            scale_plane_2x(&[0, 255, 0], dimensions(3, 1)).unwrap(),
            vec![0, 58, 221, 221, 58, 0, 0, 58, 221, 221, 58, 0]
        );
    }

    #[test]
    fn plane_length_is_checked() {
        assert_eq!(
            scale_plane_2x(&[1], dimensions(2, 1)),
            Err(AlgorithmError::InvalidPlaneLength {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn pipeline_scales_one_by_one_and_extreme_colors() {
        let input = Image::new(dimensions(1, 1), vec![Rgb8::new(255, 0, 0)]).unwrap();
        let output = BicubicBaseline::new()
            .process(&input, ProcessingConfig::new(input.dimensions()))
            .unwrap();
        assert_eq!(output.dimensions(), dimensions(2, 2));
        assert_eq!(output.pixels(), vec![Rgb8::new(255, 1, 1); 4]);
    }

    #[test]
    fn pipeline_is_deterministic() {
        let input = Image::new(
            dimensions(2, 2),
            vec![
                Rgb8::new(0, 0, 0),
                Rgb8::new(255, 0, 0),
                Rgb8::new(0, 255, 0),
                Rgb8::new(0, 0, 255),
            ],
        )
        .unwrap();
        let config = ProcessingConfig::new(input.dimensions());
        let first = BicubicBaseline::new().process(&input, config).unwrap();
        let second = BicubicBaseline::new().process(&input, config).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.dimensions(), dimensions(4, 4));
    }

    #[test]
    fn pipeline_rejects_configuration_dimension_mismatch() {
        let input = Image::new(dimensions(1, 1), vec![Rgb8::new(1, 2, 3)]).unwrap();
        assert_eq!(
            BicubicBaseline::new().process(&input, ProcessingConfig::new(dimensions(2, 1))),
            Err(AlgorithmError::DimensionMismatch {
                expected: dimensions(2, 1),
                actual: dimensions(1, 1)
            })
        );
    }
}
