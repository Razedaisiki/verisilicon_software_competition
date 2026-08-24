//! Frozen dependency-free recommended evaluation baseline.

use super::color::{YCbCr8, rgb_to_ycbcr, ycbcr_to_rgb};
use super::{
    AlgorithmError, ExecutionPolicy, SuperResolution, resolve_execution_policy, run_channel_jobs,
};
use crate::image::{Image, Rgb8};
use crate::spec::{Dimensions, ProcessingConfig, Scale, SpecError};

const REFINE_GAIN_Q8: i32 = 32;
const LOW_PHASE_WEIGHT_Q8: i64 = 64;
const HIGH_PHASE_WEIGHT_Q8: i64 = 192;

/// Frozen local baseline used only when explicitly selected for evaluation.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecommendedBaselineV1;

impl RecommendedBaselineV1 {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Processes with an explicit bounded execution policy.
    pub fn process_with_policy(
        &self,
        input: &Image,
        config: ProcessingConfig,
        policy: ExecutionPolicy,
    ) -> Result<Image, AlgorithmError> {
        process_impl(input, config, policy)
    }
}

impl SuperResolution for RecommendedBaselineV1 {
    fn process(&self, input: &Image, config: ProcessingConfig) -> Result<Image, AlgorithmError> {
        self.process_with_policy(input, config, ExecutionPolicy::Auto)
    }
}

fn process_impl(
    input: &Image,
    config: ProcessingConfig,
    policy: ExecutionPolicy,
) -> Result<Image, AlgorithmError> {
    if input.dimensions() != config.input_dimensions() {
        return Err(AlgorithmError::DimensionMismatch {
            expected: config.input_dimensions(),
            actual: input.dimensions(),
        });
    }
    if config.scale() != Scale::X2 {
        return Err(AlgorithmError::InvalidConfiguration(
            "recommended baseline requires 2x scale",
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
    let selected = resolve_execution_policy(policy, dimensions);
    let [y_scaled, cb_scaled, cr_scaled] = if selected == ExecutionPolicy::Serial {
        [
            scale_luma_nearest_refined_2x(&y_plane, dimensions)?,
            scale_chroma_bilinear_2x(&cb_plane, dimensions)?,
            scale_chroma_bilinear_2x(&cr_plane, dimensions)?,
        ]
    } else {
        run_channel_jobs(selected, |channel| match channel {
            0 => scale_luma_nearest_refined_2x(&y_plane, dimensions),
            1 => scale_chroma_bilinear_2x(&cb_plane, dimensions),
            2 => scale_chroma_bilinear_2x(&cr_plane, dimensions),
            _ => unreachable!("channel jobs are limited to three"),
        })?
    };
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
        .map_err(|_| AlgorithmError::ProcessingFailed("invalid recommended baseline output"))
}

/// Scales luma by nearest-neighbor 2x and applies the frozen refinement.
pub fn scale_luma_nearest_refined_2x(
    input: &[u8],
    dimensions: Dimensions,
) -> Result<Vec<u8>, AlgorithmError> {
    let geometry = Geometry::new(input, dimensions)?;
    let mut nearest = zeroed_u8(geometry.output_count)?;
    for y in 0..geometry.output_height {
        for x in 0..geometry.output_width {
            nearest[y * geometry.output_width + x] = input[(y / 2) * geometry.input_width + x / 2];
        }
    }
    let mut output = zeroed_u8(geometry.output_count)?;
    for y in 0..geometry.output_height {
        for x in 0..geometry.output_width {
            let center = i32::from(sample_clamped(
                &nearest,
                geometry.output_width,
                geometry.output_height,
                x,
                y,
            ));
            let north = i32::from(sample_offset(
                &nearest,
                geometry.output_width,
                geometry.output_height,
                x,
                y,
                0,
                -1,
            ));
            let south = i32::from(sample_offset(
                &nearest,
                geometry.output_width,
                geometry.output_height,
                x,
                y,
                0,
                1,
            ));
            let east = i32::from(sample_offset(
                &nearest,
                geometry.output_width,
                geometry.output_height,
                x,
                y,
                1,
                0,
            ));
            let west = i32::from(sample_offset(
                &nearest,
                geometry.output_width,
                geometry.output_height,
                x,
                y,
                -1,
                0,
            ));
            let neighbor_average = (north + south + east + west + 2) / 4;
            let refined = center + round_q8((center - neighbor_average) * REFINE_GAIN_Q8);
            let mut minimum = u8::MAX;
            let mut maximum = u8::MIN;
            for offset_y in -1..=1 {
                for offset_x in -1..=1 {
                    let value = sample_offset(
                        &nearest,
                        geometry.output_width,
                        geometry.output_height,
                        x,
                        y,
                        offset_x,
                        offset_y,
                    );
                    minimum = minimum.min(value);
                    maximum = maximum.max(value);
                }
            }
            output[y * geometry.output_width + x] =
                refined.clamp(i32::from(minimum), i32::from(maximum)) as u8;
        }
    }
    Ok(output)
}

/// Scales one chroma plane by frozen separable half-pixel Q8 bilinear 2x.
pub fn scale_chroma_bilinear_2x(
    input: &[u8],
    dimensions: Dimensions,
) -> Result<Vec<u8>, AlgorithmError> {
    let geometry = Geometry::new(input, dimensions)?;
    let mut output = zeroed_u8(geometry.output_count)?;
    for y in 0..geometry.output_height {
        let (y0, y1, wy0, wy1) = bilinear_phase(y, geometry.input_height);
        for x in 0..geometry.output_width {
            let (x0, x1, wx0, wx1) = bilinear_phase(x, geometry.input_width);
            let top = i64::from(input[y0 * geometry.input_width + x0]) * wx0
                + i64::from(input[y0 * geometry.input_width + x1]) * wx1;
            let bottom = i64::from(input[y1 * geometry.input_width + x0]) * wx0
                + i64::from(input[y1 * geometry.input_width + x1]) * wx1;
            output[y * geometry.output_width + x] =
                ((top * wy0 + bottom * wy1 + (1 << 15)) >> 16) as u8;
        }
    }
    Ok(output)
}

fn bilinear_phase(coordinate: usize, length: usize) -> (usize, usize, i64, i64) {
    let base = coordinate / 2;
    if coordinate & 1 == 0 {
        (
            base.saturating_sub(1),
            base.min(length - 1),
            LOW_PHASE_WEIGHT_Q8,
            HIGH_PHASE_WEIGHT_Q8,
        )
    } else {
        (
            base.min(length - 1),
            base.saturating_add(1).min(length - 1),
            HIGH_PHASE_WEIGHT_Q8,
            LOW_PHASE_WEIGHT_Q8,
        )
    }
}

fn sample_clamped(values: &[u8], width: usize, height: usize, x: usize, y: usize) -> u8 {
    values[y.min(height - 1) * width + x.min(width - 1)]
}

fn sample_offset(
    values: &[u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    offset_x: isize,
    offset_y: isize,
) -> u8 {
    sample_clamped(
        values,
        width,
        height,
        x.saturating_add_signed(offset_x),
        y.saturating_add_signed(offset_y),
    )
}

fn round_q8(value: i32) -> i32 {
    if value >= 0 {
        (value + 128) >> 8
    } else {
        -((-value + 128) >> 8)
    }
}

#[derive(Clone, Copy)]
struct Geometry {
    input_width: usize,
    input_height: usize,
    output_width: usize,
    output_height: usize,
    output_count: usize,
}

impl Geometry {
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
            output_height: to_usize(output_dimensions.height())?,
            output_count: output_dimensions
                .pixel_count()
                .map_err(AlgorithmError::InvalidDimensions)?,
        })
    }
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

fn zeroed_u8(length: usize) -> Result<Vec<u8>, AlgorithmError> {
    let mut values = reserve_u8(length)?;
    values.resize(length, 0);
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::{RecommendedBaselineV1, scale_chroma_bilinear_2x, scale_luma_nearest_refined_2x};
    use crate::algorithm::{AlgorithmError, ExecutionPolicy, SuperResolution};
    use crate::image::{Image, Rgb8};
    use crate::spec::{Dimensions, ProcessingConfig};

    fn dimensions(width: u32, height: u32) -> Dimensions {
        Dimensions::new(width, height).unwrap()
    }

    fn direct_bilinear(input: &[u8], size: Dimensions) -> Vec<u8> {
        let width = size.width() as usize;
        let height = size.height() as usize;
        let mut output = vec![0; width * height * 4];
        for y in 0..height * 2 {
            for x in 0..width * 2 {
                let source_x = x as f64 / 2.0 - 0.25;
                let source_y = y as f64 / 2.0 - 0.25;
                let x0 = source_x.floor() as isize;
                let y0 = source_y.floor() as isize;
                let fx = if x & 1 == 0 { 0.75 } else { 0.25 };
                let fy = if y & 1 == 0 { 0.75 } else { 0.25 };
                let sample = |sx: isize, sy: isize| {
                    let sx = sx.clamp(0, width as isize - 1) as usize;
                    let sy = sy.clamp(0, height as isize - 1) as usize;
                    f64::from(input[sy * width + sx])
                };
                let value = sample(x0, y0) * (1.0 - fx) * (1.0 - fy)
                    + sample(x0 + 1, y0) * fx * (1.0 - fy)
                    + sample(x0, y0 + 1) * (1.0 - fx) * fy
                    + sample(x0 + 1, y0 + 1) * fx * fy;
                output[y * width * 2 + x] = value.round() as u8;
            }
        }
        output
    }

    fn direct_luma(input: &[u8], size: Dimensions) -> Vec<u8> {
        let width = size.width() as usize * 2;
        let height = size.height() as usize * 2;
        let mut nearest = vec![0; width * height];
        for y in 0..height {
            for x in 0..width {
                nearest[y * width + x] = input[(y / 2) * size.width() as usize + x / 2];
            }
        }
        let get = |x: isize, y: isize| {
            let x = x.clamp(0, width as isize - 1) as usize;
            let y = y.clamp(0, height as isize - 1) as usize;
            nearest[y * width + x]
        };
        let mut output = Vec::with_capacity(nearest.len());
        for y in 0..height as isize {
            for x in 0..width as isize {
                let center = i32::from(get(x, y));
                let average = (i32::from(get(x, y - 1))
                    + i32::from(get(x, y + 1))
                    + i32::from(get(x - 1, y))
                    + i32::from(get(x + 1, y))
                    + 2)
                    / 4;
                let detail_q8 = (center - average) * 32;
                let delta = if detail_q8 >= 0 {
                    (detail_q8 + 128) >> 8
                } else {
                    -((-detail_q8 + 128) >> 8)
                };
                let mut minimum = u8::MAX;
                let mut maximum = u8::MIN;
                for oy in -1..=1 {
                    for ox in -1..=1 {
                        let value = get(x + ox, y + oy);
                        minimum = minimum.min(value);
                        maximum = maximum.max(value);
                    }
                }
                output.push((center + delta).clamp(i32::from(minimum), i32::from(maximum)) as u8);
            }
        }
        output
    }

    #[test]
    fn plane_vectors_phases_and_borders_match_direct_oracles() {
        let size = dimensions(3, 2);
        let plane = [0, 64, 255, 17, 129, 231];
        assert_eq!(
            scale_chroma_bilinear_2x(&plane, size).unwrap(),
            direct_bilinear(&plane, size)
        );
        assert_eq!(
            scale_luma_nearest_refined_2x(&plane, size).unwrap(),
            direct_luma(&plane, size)
        );
        assert_eq!(
            scale_luma_nearest_refined_2x(&plane, size).unwrap(),
            vec![
                0, 0, 64, 64, 255, 255, 0, 0, 64, 64, 255, 255, 17, 14, 129, 128, 233, 231, 17, 17,
                129, 129, 231, 231,
            ]
        );
        assert_eq!(
            scale_chroma_bilinear_2x(&[0, 255], dimensions(2, 1)).unwrap(),
            vec![0, 64, 191, 255, 0, 64, 191, 255]
        );
    }

    #[test]
    fn constants_dimensions_and_configuration_errors_are_exact() {
        assert_eq!(
            scale_luma_nearest_refined_2x(&[91], dimensions(1, 1)).unwrap(),
            vec![91; 4]
        );
        assert_eq!(
            scale_chroma_bilinear_2x(&[37], dimensions(1, 1)).unwrap(),
            vec![37; 4]
        );
        assert_eq!(
            scale_chroma_bilinear_2x(&[1], dimensions(2, 1)),
            Err(AlgorithmError::InvalidPlaneLength {
                expected: 2,
                actual: 1
            })
        );
        let input = Image::new(dimensions(1, 1), vec![Rgb8::new(10, 20, 30)]).unwrap();
        let output = RecommendedBaselineV1::new()
            .process(&input, ProcessingConfig::new(input.dimensions()))
            .unwrap();
        assert_eq!(output.dimensions(), dimensions(2, 2));
        assert_eq!(
            RecommendedBaselineV1::new().process(&input, ProcessingConfig::new(dimensions(2, 1))),
            Err(AlgorithmError::DimensionMismatch {
                expected: dimensions(2, 1),
                actual: dimensions(1, 1)
            })
        );
    }

    #[test]
    fn luma_never_leaves_original_nearest_three_by_three_envelope() {
        let size = dimensions(5, 4);
        let input: Vec<u8> = (0..20)
            .map(|index| ((index * 73 + 19) & 255) as u8)
            .collect();
        let output = scale_luma_nearest_refined_2x(&input, size).unwrap();
        let width = size.width() as usize * 2;
        let height = size.height() as usize * 2;
        for y in 0..height {
            for x in 0..width {
                let mut minimum = u8::MAX;
                let mut maximum = u8::MIN;
                for oy in -1_isize..=1 {
                    for ox in -1_isize..=1 {
                        let nx = x.saturating_add_signed(ox).min(width - 1);
                        let ny = y.saturating_add_signed(oy).min(height - 1);
                        let value = input[(ny / 2) * size.width() as usize + nx / 2];
                        minimum = minimum.min(value);
                        maximum = maximum.max(value);
                    }
                }
                assert!((minimum..=maximum).contains(&output[y * width + x]));
            }
        }
    }

    #[test]
    fn serial_parallel_and_repeated_outputs_match() {
        let size = dimensions(9, 7);
        let pixels = (0..63)
            .map(|index| Rgb8::new((index * 31) as u8, (index * 17) as u8, (index * 7) as u8))
            .collect();
        let input = Image::new(size, pixels).unwrap();
        let config = ProcessingConfig::new(size);
        let serial = RecommendedBaselineV1::new()
            .process_with_policy(&input, config, ExecutionPolicy::Serial)
            .unwrap();
        let parallel = RecommendedBaselineV1::new()
            .process_with_policy(&input, config, ExecutionPolicy::Parallel)
            .unwrap();
        assert_eq!(serial, parallel);
        assert_eq!(
            serial,
            RecommendedBaselineV1::new()
                .process(&input, config)
                .unwrap()
        );
    }
}
