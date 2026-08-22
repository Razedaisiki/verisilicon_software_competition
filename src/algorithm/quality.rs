//! Deterministic scalar luma enhancement candidate.

use super::bicubic::scale_plane_2x;
#[cfg(test)]
use super::bicubic::scale_plane_2x_reference;
use super::color::{YCbCr8, rgb_to_ycbcr, ycbcr_to_rgb};
use super::{AlgorithmError, SuperResolution};
use crate::image::{Image, Rgb8};
use crate::spec::{Dimensions, ProcessingConfig, Scale, SpecError};

/// Minimum dominant Sobel component used to classify a sample as an edge.
pub const EDGE_THRESHOLD: i32 = 48;

/// Dominant gradient ratio used for horizontal or vertical classification.
pub const AXIS_DOMINANCE_RATIO: i32 = 2;

/// Q8 blend gain for refinement along the detected edge orientation.
pub const DIRECTIONAL_REFINE_GAIN_Q8: i32 = 64;

/// Q8 gain for the four-neighbor luma detail term.
pub const SHARPEN_GAIN_Q8: i32 = 48;

/// Radius of the unenhanced bicubic luma envelope used for anti-ringing.
pub const ENVELOPE_RADIUS: usize = 1;

/// Quantized orientation of an edge through one luma sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdgeOrientation {
    Flat,
    Horizontal,
    Vertical,
    DiagonalDown,
    DiagonalUp,
}

/// Opt-in scalar quality candidate. It is not a measured quality claim.
#[derive(Clone, Copy, Debug, Default)]
pub struct QualityPipeline;

impl QualityPipeline {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Runs the retained full-intermediate scalar scaler as an exact oracle.
    #[cfg(test)]
    pub(crate) fn process_reference(
        &self,
        input: &Image,
        config: ProcessingConfig,
    ) -> Result<Image, AlgorithmError> {
        process_impl(input, config, scale_plane_2x_reference)
    }
}

impl SuperResolution for QualityPipeline {
    fn process(&self, input: &Image, config: ProcessingConfig) -> Result<Image, AlgorithmError> {
        process_impl(input, config, scale_plane_2x)
    }
}

fn process_impl(
    input: &Image,
    config: ProcessingConfig,
    scaler: fn(&[u8], Dimensions) -> Result<Vec<u8>, AlgorithmError>,
) -> Result<Image, AlgorithmError> {
    if input.dimensions() != config.input_dimensions() {
        return Err(AlgorithmError::DimensionMismatch {
            expected: config.input_dimensions(),
            actual: input.dimensions(),
        });
    }
    if config.scale() != Scale::X2 {
        return Err(AlgorithmError::InvalidConfiguration(
            "quality pipeline requires 2x scale",
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

    let input_dimensions = input.dimensions();
    let output_dimensions = config
        .output_dimensions()
        .map_err(AlgorithmError::InvalidDimensions)?;
    let y_bicubic = scaler(&y_plane, input_dimensions)?;
    drop(y_plane);
    let y_enhanced = enhance_luma(&y_bicubic, output_dimensions)?;
    drop(y_bicubic);
    let cb_scaled = scaler(&cb_plane, input_dimensions)?;
    drop(cb_plane);
    let cr_scaled = scaler(&cr_plane, input_dimensions)?;
    drop(cr_plane);

    let output_count = output_dimensions
        .pixel_count()
        .map_err(AlgorithmError::InvalidDimensions)?;
    let mut pixels = reserve_rgb8(output_count)?;
    for index in 0..output_count {
        pixels.push(ycbcr_to_rgb(YCbCr8::new(
            y_enhanced[index],
            cb_scaled[index],
            cr_scaled[index],
        )));
    }
    Image::new(output_dimensions, pixels)
        .map_err(|_| AlgorithmError::ProcessingFailed("invalid quality output image"))
}

/// Enhances an already bicubic-scaled luma plane.
///
/// All neighborhood reads come from the unmodified input plane. Directional
/// refinement averages only the two samples along the detected edge, blended
/// with Q8 gain 64. A four-neighbor detail term is added with Q8 gain 48.
/// Every result is finally clamped to the minimum and maximum in the original
/// 3x3 bicubic neighborhood, preventing new local extrema.
pub fn enhance_luma(input: &[u8], dimensions: Dimensions) -> Result<Vec<u8>, AlgorithmError> {
    let expected = dimensions
        .pixel_count()
        .map_err(AlgorithmError::InvalidDimensions)?;
    if input.len() != expected {
        return Err(AlgorithmError::InvalidPlaneLength {
            expected,
            actual: input.len(),
        });
    }
    let width = to_usize(dimensions.width())?;
    let height = to_usize(dimensions.height())?;
    let mut output = zeroed_u8(expected)?;

    for y in 0..height {
        for x in 0..width {
            let center = i32::from(sample(input, width, height, x, y, 0, 0));
            let orientation = detect_edge_orientation_unchecked(input, width, height, x, y);
            let directed = match orientation {
                EdgeOrientation::Flat => center,
                EdgeOrientation::Horizontal => average_pair(
                    sample(input, width, height, x, y, -1, 0),
                    sample(input, width, height, x, y, 1, 0),
                ),
                EdgeOrientation::Vertical => average_pair(
                    sample(input, width, height, x, y, 0, -1),
                    sample(input, width, height, x, y, 0, 1),
                ),
                EdgeOrientation::DiagonalDown => average_pair(
                    sample(input, width, height, x, y, -1, -1),
                    sample(input, width, height, x, y, 1, 1),
                ),
                EdgeOrientation::DiagonalUp => average_pair(
                    sample(input, width, height, x, y, 1, -1),
                    sample(input, width, height, x, y, -1, 1),
                ),
            };
            let refined = center + round_q8((directed - center) * DIRECTIONAL_REFINE_GAIN_Q8);
            let axial_sum = i32::from(sample(input, width, height, x, y, -1, 0))
                + i32::from(sample(input, width, height, x, y, 1, 0))
                + i32::from(sample(input, width, height, x, y, 0, -1))
                + i32::from(sample(input, width, height, x, y, 0, 1));
            let low_pass = (axial_sum + 2) / 4;
            let sharpened = refined + round_q8((center - low_pass) * SHARPEN_GAIN_Q8);
            let (minimum, maximum) = local_envelope(input, width, height, x, y);
            output[y * width + x] = sharpened.clamp(i32::from(minimum), i32::from(maximum)) as u8;
        }
    }
    Ok(output)
}

fn detect_edge_orientation_unchecked(
    input: &[u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
) -> EdgeOrientation {
    let north_west = i32::from(sample(input, width, height, x, y, -1, -1));
    let north = i32::from(sample(input, width, height, x, y, 0, -1));
    let north_east = i32::from(sample(input, width, height, x, y, 1, -1));
    let west = i32::from(sample(input, width, height, x, y, -1, 0));
    let east = i32::from(sample(input, width, height, x, y, 1, 0));
    let south_west = i32::from(sample(input, width, height, x, y, -1, 1));
    let south = i32::from(sample(input, width, height, x, y, 0, 1));
    let south_east = i32::from(sample(input, width, height, x, y, 1, 1));

    let gradient_x = (north_east + 2 * east + south_east) - (north_west + 2 * west + south_west);
    let gradient_y = (south_west + 2 * south + south_east) - (north_west + 2 * north + north_east);
    let magnitude_x = gradient_x.abs();
    let magnitude_y = gradient_y.abs();
    if magnitude_x.max(magnitude_y) < EDGE_THRESHOLD {
        EdgeOrientation::Flat
    } else if magnitude_y >= AXIS_DOMINANCE_RATIO * magnitude_x {
        EdgeOrientation::Horizontal
    } else if magnitude_x >= AXIS_DOMINANCE_RATIO * magnitude_y {
        EdgeOrientation::Vertical
    } else if gradient_x.signum() == gradient_y.signum() {
        EdgeOrientation::DiagonalUp
    } else {
        EdgeOrientation::DiagonalDown
    }
}

fn local_envelope(input: &[u8], width: usize, height: usize, x: usize, y: usize) -> (u8, u8) {
    let mut minimum = u8::MAX;
    let mut maximum = u8::MIN;
    for y_offset in -(ENVELOPE_RADIUS as isize)..=ENVELOPE_RADIUS as isize {
        for x_offset in -(ENVELOPE_RADIUS as isize)..=ENVELOPE_RADIUS as isize {
            let value = sample(input, width, height, x, y, x_offset, y_offset);
            minimum = minimum.min(value);
            maximum = maximum.max(value);
        }
    }
    (minimum, maximum)
}

fn sample(
    input: &[u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    x_offset: isize,
    y_offset: isize,
) -> u8 {
    let sample_x = x.saturating_add_signed(x_offset).min(width - 1);
    let sample_y = y.saturating_add_signed(y_offset).min(height - 1);
    input[sample_y * width + sample_x]
}

fn average_pair(first: u8, second: u8) -> i32 {
    (i32::from(first) + i32::from(second) + 1) / 2
}

fn round_q8(value: i32) -> i32 {
    if value >= 0 {
        (value + 128) >> 8
    } else {
        -((-value + 128) >> 8)
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
    use super::{
        EdgeOrientation, QualityPipeline, detect_edge_orientation_unchecked, enhance_luma,
        local_envelope,
    };
    use crate::algorithm::{BicubicBaseline, SuperResolution};
    use crate::fixtures::{HardEdge, checker_detail, hard_edge, smooth_gradient};
    use crate::image::{Image, Rgb8};
    use crate::spec::{Dimensions, ProcessingConfig};

    fn dimensions(width: u32, height: u32) -> Dimensions {
        Dimensions::new(width, height).unwrap()
    }

    fn orientation(plane: &[u8]) -> EdgeOrientation {
        detect_edge_orientation_unchecked(plane, 3, 3, 1, 1)
    }

    #[test]
    fn constant_luma_and_borders_are_preserved() {
        assert_eq!(enhance_luma(&[91], dimensions(1, 1)).unwrap(), vec![91]);
        assert_eq!(
            enhance_luma(&[37, 37, 37, 37], dimensions(2, 2)).unwrap(),
            vec![37; 4]
        );
    }

    #[test]
    fn detects_horizontal_vertical_and_diagonal_orientations() {
        assert_eq!(
            orientation(&[0, 0, 0, 128, 128, 128, 255, 255, 255]),
            EdgeOrientation::Horizontal
        );
        assert_eq!(
            orientation(&[0, 128, 255, 0, 128, 255, 0, 128, 255]),
            EdgeOrientation::Vertical
        );
        assert_eq!(
            orientation(&[0, 0, 255, 0, 255, 255, 255, 255, 255]),
            EdgeOrientation::DiagonalUp
        );
        assert_eq!(
            orientation(&[0, 255, 255, 0, 0, 255, 0, 0, 0]),
            EdgeOrientation::DiagonalDown
        );
    }

    #[test]
    fn anti_ringing_respects_every_local_envelope() {
        let plane = [0, 8, 32, 64, 128, 192, 224, 248, 255];
        let enhanced = enhance_luma(&plane, dimensions(3, 3)).unwrap();
        for y in 0..3 {
            for x in 0..3 {
                let (minimum, maximum) = local_envelope(&plane, 3, 3, x, y);
                let value = enhanced[y * 3 + x];
                assert!(value >= minimum && value <= maximum);
            }
        }
    }

    #[test]
    fn quality_pipeline_is_deterministic_and_has_exact_dimensions() {
        let input = Image::new(
            dimensions(2, 2),
            vec![
                Rgb8::new(0, 0, 0),
                Rgb8::new(255, 255, 255),
                Rgb8::new(0, 0, 0),
                Rgb8::new(255, 255, 255),
            ],
        )
        .unwrap();
        let config = ProcessingConfig::new(input.dimensions());
        let first = QualityPipeline::new().process(&input, config).unwrap();
        let second = QualityPipeline::new().process(&input, config).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.dimensions(), dimensions(4, 4));
    }

    #[test]
    fn quality_pipeline_rejects_dimension_mismatch() {
        let input = Image::new(dimensions(1, 1), vec![Rgb8::new(1, 2, 3)]).unwrap();
        assert!(
            QualityPipeline::new()
                .process(&input, ProcessingConfig::new(dimensions(2, 1)))
                .is_err()
        );
    }

    #[test]
    fn optimized_quality_matches_reference_across_patterns_and_sizes() {
        let inputs = [
            Image::new(dimensions(1, 1), vec![Rgb8::new(19, 37, 83)]).unwrap(),
            smooth_gradient(dimensions(7, 5)).unwrap(),
            hard_edge(dimensions(1, 7), HardEdge::Horizontal).unwrap(),
            checker_detail(dimensions(9, 3), 2).unwrap(),
        ];
        for input in inputs {
            let config = ProcessingConfig::new(input.dimensions());
            let reference = QualityPipeline::new()
                .process_reference(&input, config)
                .unwrap();
            for _ in 0..3 {
                assert_eq!(
                    QualityPipeline::new().process(&input, config).unwrap(),
                    reference
                );
            }
        }
    }

    #[test]
    fn synthetic_edge_differs_from_baseline_without_luma_overshoot() {
        let input = Image::new(
            dimensions(4, 2),
            vec![
                Rgb8::new(0, 0, 0),
                Rgb8::new(0, 0, 0),
                Rgb8::new(255, 255, 255),
                Rgb8::new(255, 255, 255),
                Rgb8::new(0, 0, 0),
                Rgb8::new(0, 0, 0),
                Rgb8::new(255, 255, 255),
                Rgb8::new(255, 255, 255),
            ],
        )
        .unwrap();
        let config = ProcessingConfig::new(input.dimensions());
        let baseline = BicubicBaseline::new().process(&input, config).unwrap();
        let quality = QualityPipeline::new().process(&input, config).unwrap();
        assert_ne!(quality, baseline);
        let width = 8_usize;
        let height = 4_usize;
        for y in 0..height {
            for x in 0..width {
                let mut minimum = u8::MAX;
                let mut maximum = u8::MIN;
                for y_offset in -1_isize..=1 {
                    for x_offset in -1_isize..=1 {
                        let sample_x = x.saturating_add_signed(x_offset).min(width - 1);
                        let sample_y = y.saturating_add_signed(y_offset).min(height - 1);
                        let value = baseline.pixels()[sample_y * width + sample_x].red;
                        minimum = minimum.min(value);
                        maximum = maximum.max(value);
                    }
                }
                let pixel = quality.pixels()[y * width + x];
                assert!(pixel.red >= minimum && pixel.red <= maximum);
                assert_eq!(pixel.red, pixel.green);
                assert_eq!(pixel.green, pixel.blue);
            }
        }
    }
}
