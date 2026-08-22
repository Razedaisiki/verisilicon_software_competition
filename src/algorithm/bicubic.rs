//! Deterministic separable 2x Catmull-Rom bicubic baseline.

use super::color::{YCbCr8, rgb_to_ycbcr, ycbcr_to_rgb};
use super::{AlgorithmError, SuperResolution};
use crate::image::{Image, Rgb8};
use crate::spec::{Dimensions, ProcessingConfig, Scale, SpecError};

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
const CACHE_ROWS: usize = 4;

/// Dependency-free baseline using full-range YCbCr planes.
#[derive(Clone, Copy, Debug, Default)]
pub struct BicubicBaseline;

impl BicubicBaseline {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Runs the retained single-thread full-intermediate implementation.
    ///
    /// This path exists as an exact regression oracle for the cached default.
    #[cfg(test)]
    pub(crate) fn process_reference(
        &self,
        input: &Image,
        config: ProcessingConfig,
    ) -> Result<Image, AlgorithmError> {
        process_impl(input, config, scale_plane_2x_reference)
    }
}

impl SuperResolution for BicubicBaseline {
    fn process(&self, input: &Image, config: ProcessingConfig) -> Result<Image, AlgorithmError> {
        process_impl(input, config, scale_plane_2x)
    }
}

fn process_impl(
    input: &Image,
    config: ProcessingConfig,
    scaler: fn(&[u8], Dimensions) -> Result<Vec<u8>, AlgorithmError>,
) -> Result<Image, AlgorithmError> {
    validate_pipeline(input, config, "bicubic baseline requires 2x scale")?;
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
    let y_scaled = scaler(&y_plane, dimensions)?;
    drop(y_plane);
    let cb_scaled = scaler(&cb_plane, dimensions)?;
    drop(cb_plane);
    let cr_scaled = scaler(&cr_plane, dimensions)?;
    drop(cr_plane);
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

fn validate_pipeline(
    input: &Image,
    config: ProcessingConfig,
    scale_message: &'static str,
) -> Result<(), AlgorithmError> {
    if input.dimensions() != config.input_dimensions() {
        return Err(AlgorithmError::DimensionMismatch {
            expected: config.input_dimensions(),
            actual: input.dimensions(),
        });
    }
    if config.scale() != Scale::X2 {
        return Err(AlgorithmError::InvalidConfiguration(scale_message));
    }
    Ok(())
}

/// Scales one plane with a four-row signed cache.
///
/// The arithmetic is identical to [`scale_plane_2x_reference`]. The scaler
/// owns at most four signed Q7 horizontal rows. The vertical pass rounds the
/// combined Q14 value to nearest with halves away from zero and clips only the
/// final sample.
pub fn scale_plane_2x(input: &[u8], dimensions: Dimensions) -> Result<Vec<u8>, AlgorithmError> {
    let geometry = ScaleGeometry::new(input, dimensions)?;
    let mut output = zeroed_u8(geometry.output_count)?;
    scale_cached_rows(input, geometry, 0, &mut output)?;
    Ok(output)
}

/// Retained full-intermediate scalar test oracle.
#[cfg(test)]
pub(crate) fn scale_plane_2x_reference(
    input: &[u8],
    dimensions: Dimensions,
) -> Result<Vec<u8>, AlgorithmError> {
    let geometry = ScaleGeometry::new(input, dimensions)?;
    let intermediate_count = geometry
        .output_width
        .checked_mul(geometry.input_height)
        .ok_or(AlgorithmError::InvalidDimensions(SpecError::SizeOverflow))?;
    let mut horizontal = zeroed_i32(intermediate_count)?;
    for y in 0..geometry.input_height {
        compute_horizontal_row(
            input,
            geometry.input_width,
            geometry.output_width,
            y,
            &mut horizontal[y * geometry.output_width..(y + 1) * geometry.output_width],
        );
    }
    let mut output = zeroed_u8(geometry.output_count)?;
    for y_out in 0..geometry.output_count / geometry.output_width {
        let (offsets, weights) = phase(y_out);
        let base = y_out / 2;
        for x in 0..geometry.output_width {
            let mut sum = 0_i64;
            for tap in 0..4 {
                let y = clamped_index(base, offsets[tap], geometry.input_height);
                sum +=
                    i64::from(horizontal[y * geometry.output_width + x]) * i64::from(weights[tap]);
            }
            output[y_out * geometry.output_width + x] = clip_u8(round_q14(sum));
        }
    }
    Ok(output)
}

#[derive(Clone, Copy)]
struct ScaleGeometry {
    input_width: usize,
    input_height: usize,
    output_width: usize,
    output_count: usize,
}

impl ScaleGeometry {
    fn new(input: &[u8], dimensions: Dimensions) -> Result<Self, AlgorithmError> {
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
        Ok(Self {
            input_width: to_usize(dimensions.width())?,
            input_height: to_usize(dimensions.height())?,
            output_width: to_usize(output_dimensions.width())?,
            output_count: output_dimensions
                .pixel_count()
                .map_err(AlgorithmError::InvalidDimensions)?,
        })
    }
}

struct HorizontalRowCache {
    values: Vec<i32>,
    row_ids: [Option<usize>; CACHE_ROWS],
    output_width: usize,
}

impl HorizontalRowCache {
    fn new(output_width: usize) -> Result<Self, AlgorithmError> {
        let length = output_width
            .checked_mul(CACHE_ROWS)
            .ok_or(AlgorithmError::InvalidDimensions(SpecError::SizeOverflow))?;
        Ok(Self {
            values: zeroed_i32(length)?,
            row_ids: [None; CACHE_ROWS],
            output_width,
        })
    }

    fn prepare(
        &mut self,
        required: [usize; 4],
        input: &[u8],
        input_width: usize,
    ) -> Result<[usize; 4], AlgorithmError> {
        for source_y in required {
            if self.row_ids.contains(&Some(source_y)) {
                continue;
            }
            let slot = self
                .row_ids
                .iter()
                .position(|row_id| row_id.is_none_or(|row_id| !required.contains(&row_id)))
                .ok_or(AlgorithmError::ProcessingFailed(
                    "horizontal row cache has no replaceable slot",
                ))?;
            let start = slot * self.output_width;
            compute_horizontal_row(
                input,
                input_width,
                self.output_width,
                source_y,
                &mut self.values[start..start + self.output_width],
            );
            self.row_ids[slot] = Some(source_y);
        }
        let mut positions = [0_usize; 4];
        for (tap, source_y) in required.into_iter().enumerate() {
            positions[tap] = self
                .row_ids
                .iter()
                .position(|row_id| *row_id == Some(source_y))
                .ok_or(AlgorithmError::ProcessingFailed(
                    "horizontal row cache lost a required row",
                ))?;
        }
        Ok(positions)
    }

    fn row(&self, slot: usize) -> &[i32] {
        let start = slot * self.output_width;
        &self.values[start..start + self.output_width]
    }
}

fn scale_cached_rows(
    input: &[u8],
    geometry: ScaleGeometry,
    start_y: usize,
    output: &mut [u8],
) -> Result<(), AlgorithmError> {
    let mut cache = HorizontalRowCache::new(geometry.output_width)?;
    for (local_y, output_row) in output.chunks_mut(geometry.output_width).enumerate() {
        let y_out = start_y + local_y;
        let (offsets, weights) = phase(y_out);
        let base = y_out / 2;
        let required = offsets.map(|offset| clamped_index(base, offset, geometry.input_height));
        let positions = cache.prepare(required, input, geometry.input_width)?;
        for (x, sample) in output_row.iter_mut().enumerate() {
            let mut sum = 0_i64;
            for tap in 0..4 {
                sum += i64::from(cache.row(positions[tap])[x]) * i64::from(weights[tap]);
            }
            *sample = clip_u8(round_q14(sum));
        }
    }
    Ok(())
}

fn compute_horizontal_row(
    input: &[u8],
    input_width: usize,
    output_width: usize,
    source_y: usize,
    output: &mut [i32],
) {
    for (x_out, result) in output.iter_mut().enumerate().take(output_width) {
        let (offsets, weights) = phase(x_out);
        let base = x_out / 2;
        let mut sum = 0_i32;
        for tap in 0..4 {
            let x = clamped_index(base, offsets[tap], input_width);
            sum += i32::from(input[source_y * input_width + x]) * weights[tap];
        }
        *result = sum;
    }
}

fn phase(coordinate: usize) -> ([isize; 4], [i32; 4]) {
    if coordinate & 1 == 0 {
        ([-2, -1, 0, 1], EVEN_PHASE_WEIGHTS)
    } else {
        ([-1, 0, 1, 2], ODD_PHASE_WEIGHTS)
    }
}

fn clamped_index(base: usize, offset: isize, length: usize) -> usize {
    base.saturating_add_signed(offset).min(length - 1)
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

fn to_usize(value: u32) -> Result<usize, AlgorithmError> {
    usize::try_from(value).map_err(|_| AlgorithmError::InvalidDimensions(SpecError::SizeOverflow))
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
    use super::{
        BicubicBaseline, EVEN_PHASE_WEIGHTS, ODD_PHASE_WEIGHTS, scale_plane_2x,
        scale_plane_2x_reference,
    };
    use crate::algorithm::{AlgorithmError, SuperResolution};
    use crate::fixtures::{HardEdge, checker_detail, hard_edge, smooth_gradient};
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
    fn fixed_vectors_and_borders_are_unchanged() {
        assert_eq!(
            scale_plane_2x(&[73], dimensions(1, 1)).unwrap(),
            vec![73; 4]
        );
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
    fn cached_scaler_matches_reference_for_odd_and_thin_planes() {
        let cases = [
            (dimensions(1, 1), vec![17]),
            (dimensions(1, 5), vec![0, 64, 128, 192, 255]),
            (dimensions(5, 1), vec![0, 64, 128, 192, 255]),
            (
                dimensions(3, 5),
                (0_u8..15).map(|value| value * 17).collect(),
            ),
        ];
        for (size, plane) in cases {
            let reference = scale_plane_2x_reference(&plane, size).unwrap();
            for _ in 0..3 {
                assert_eq!(scale_plane_2x(&plane, size).unwrap(), reference);
            }
        }
    }

    #[test]
    fn optimized_pipeline_matches_reference_across_patterns() {
        let inputs = [
            smooth_gradient(dimensions(7, 5)).unwrap(),
            hard_edge(dimensions(9, 7), HardEdge::Vertical).unwrap(),
            checker_detail(dimensions(5, 9), 2).unwrap(),
        ];
        for input in inputs {
            let config = ProcessingConfig::new(input.dimensions());
            let expected = BicubicBaseline::new()
                .process_reference(&input, config)
                .unwrap();
            for _ in 0..3 {
                assert_eq!(
                    BicubicBaseline::new().process(&input, config).unwrap(),
                    expected
                );
            }
        }
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
