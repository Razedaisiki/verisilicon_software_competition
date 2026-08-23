//! Deterministic scalar luma enhancement candidate.

use super::bicubic::scale_plane_2x;
#[cfg(test)]
use super::bicubic::scale_plane_2x_reference;
use super::color::{YCbCr8, rgb_to_ycbcr, ycbcr_to_rgb};
use super::{
    AlgorithmError, ExecutionPolicy, SuperResolution, resolve_execution_policy, run_channel_jobs,
};
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

/// Explicit evaluation-only parameters for the existing quality arithmetic.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct QualityParameters {
    pub edge_threshold: i32,
    pub axis_dominance_ratio: i32,
    pub directional_refine_gain_q8: i32,
    pub sharpen_gain_q8: i32,
}

pub const DEFAULT_QUALITY_PARAMETERS: QualityParameters = QualityParameters {
    edge_threshold: EDGE_THRESHOLD,
    axis_dominance_ratio: AXIS_DOMINANCE_RATIO,
    directional_refine_gain_q8: DIRECTIONAL_REFINE_GAIN_Q8,
    sharpen_gain_q8: SHARPEN_GAIN_Q8,
};

/// Ungated parameters selected by the milestone-one cross-validation sweep.
pub const SELECTED_UNGATED_PARAMETERS: QualityParameters = QualityParameters {
    edge_threshold: 64,
    axis_dominance_ratio: 2,
    directional_refine_gain_q8: 32,
    sharpen_gain_q8: 64,
};

impl QualityParameters {
    fn validate(self) -> Result<Self, AlgorithmError> {
        if self.edge_threshold < 0
            || !(1..=256).contains(&self.axis_dominance_ratio)
            || !(0..=256).contains(&self.directional_refine_gain_q8)
            || !(0..=256).contains(&self.sharpen_gain_q8)
        {
            return Err(AlgorithmError::InvalidConfiguration(
                "quality evaluation parameters are outside supported bounds",
            ));
        }
        Ok(self)
    }
}

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

    /// Processes with an explicit execution policy.
    pub fn process_with_policy(
        &self,
        input: &Image,
        config: ProcessingConfig,
        policy: ExecutionPolicy,
    ) -> Result<Image, AlgorithmError> {
        self.process_with_parameters(input, config, policy, DEFAULT_QUALITY_PARAMETERS)
    }

    /// Processes with explicit evaluation-only parameters.
    pub fn process_with_parameters(
        &self,
        input: &Image,
        config: ProcessingConfig,
        policy: ExecutionPolicy,
        parameters: QualityParameters,
    ) -> Result<Image, AlgorithmError> {
        process_impl(
            input,
            config,
            scale_plane_2x,
            policy,
            parameters.validate()?,
            false,
        )
    }

    /// Runs the retained full-intermediate scalar scaler as an exact oracle.
    #[cfg(test)]
    pub(crate) fn process_reference(
        &self,
        input: &Image,
        config: ProcessingConfig,
    ) -> Result<Image, AlgorithmError> {
        process_impl(
            input,
            config,
            scale_plane_2x_reference,
            ExecutionPolicy::Serial,
            DEFAULT_QUALITY_PARAMETERS,
            false,
        )
    }
}

impl SuperResolution for QualityPipeline {
    fn process(&self, input: &Image, config: ProcessingConfig) -> Result<Image, AlgorithmError> {
        self.process_with_policy(input, config, ExecutionPolicy::Auto)
    }
}

/// Validated ungated quality candidate selected for public processing.
///
/// The frozen [`QualityPipeline`] remains available as a separate library and
/// evaluator path.
#[derive(Clone, Copy, Debug, Default)]
pub struct SelectedQualityPipeline;

impl SelectedQualityPipeline {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn process_with_policy(
        &self,
        input: &Image,
        config: ProcessingConfig,
        policy: ExecutionPolicy,
    ) -> Result<Image, AlgorithmError> {
        QualityPipeline::new().process_with_parameters(
            input,
            config,
            policy,
            SELECTED_UNGATED_PARAMETERS,
        )
    }
}

impl SuperResolution for SelectedQualityPipeline {
    fn process(&self, input: &Image, config: ProcessingConfig) -> Result<Image, AlgorithmError> {
        self.process_with_policy(input, config, ExecutionPolicy::Auto)
    }
}

/// Isolated confidence-gated evaluation candidate.
///
/// This does not replace [`QualityPipeline`] or the default command-line path.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConfidenceGatedQualityPipeline;

impl ConfidenceGatedQualityPipeline {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub fn process_with_policy(
        &self,
        input: &Image,
        config: ProcessingConfig,
        policy: ExecutionPolicy,
    ) -> Result<Image, AlgorithmError> {
        process_impl(
            input,
            config,
            scale_plane_2x,
            policy,
            SELECTED_UNGATED_PARAMETERS,
            true,
        )
    }
}

impl SuperResolution for ConfidenceGatedQualityPipeline {
    fn process(&self, input: &Image, config: ProcessingConfig) -> Result<Image, AlgorithmError> {
        self.process_with_policy(input, config, ExecutionPolicy::Auto)
    }
}

fn process_impl(
    input: &Image,
    config: ProcessingConfig,
    scaler: fn(&[u8], Dimensions) -> Result<Vec<u8>, AlgorithmError>,
    policy: ExecutionPolicy,
    parameters: QualityParameters,
    confidence_gated: bool,
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
    let selected = resolve_execution_policy(policy, input_dimensions);
    let [y_enhanced, cb_scaled, cr_scaled] = if selected == ExecutionPolicy::Serial {
        let y_bicubic = scaler(&y_plane, input_dimensions)?;
        drop(y_plane);
        let y_enhanced =
            enhance_luma_candidate(&y_bicubic, output_dimensions, parameters, confidence_gated)?;
        drop(y_bicubic);
        let cb_scaled = scaler(&cb_plane, input_dimensions)?;
        drop(cb_plane);
        let cr_scaled = scaler(&cr_plane, input_dimensions)?;
        drop(cr_plane);
        [y_enhanced, cb_scaled, cr_scaled]
    } else {
        let scaled = run_channel_jobs(selected, |channel| match channel {
            0 => {
                let y_bicubic = scaler(&y_plane, input_dimensions)?;
                enhance_luma_candidate(&y_bicubic, output_dimensions, parameters, confidence_gated)
            }
            1 => scaler(&cb_plane, input_dimensions),
            2 => scaler(&cr_plane, input_dimensions),
            _ => unreachable!("channel jobs are limited to three"),
        })?;
        drop(y_plane);
        drop(cb_plane);
        drop(cr_plane);
        scaled
    };

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
    enhance_luma_with_parameters(input, dimensions, DEFAULT_QUALITY_PARAMETERS)
}

fn enhance_luma_with_parameters(
    input: &[u8],
    dimensions: Dimensions,
    parameters: QualityParameters,
) -> Result<Vec<u8>, AlgorithmError> {
    enhance_luma_candidate(input, dimensions, parameters, false)
}

fn enhance_luma_candidate(
    input: &[u8],
    dimensions: Dimensions,
    parameters: QualityParameters,
    confidence_gated: bool,
) -> Result<Vec<u8>, AlgorithmError> {
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
            let orientation =
                detect_edge_orientation_with_parameters(input, width, height, x, y, parameters);
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
            let refined =
                center + round_q8((directed - center) * parameters.directional_refine_gain_q8);
            let axial_sum = i32::from(sample(input, width, height, x, y, -1, 0))
                + i32::from(sample(input, width, height, x, y, 1, 0))
                + i32::from(sample(input, width, height, x, y, 0, -1))
                + i32::from(sample(input, width, height, x, y, 0, 1));
            let low_pass = (axial_sum + 2) / 4;
            let sharpened = refined + round_q8((center - low_pass) * parameters.sharpen_gain_q8);
            let (minimum, maximum) = local_envelope(input, width, height, x, y);
            let enhanced = sharpened.clamp(i32::from(minimum), i32::from(maximum));
            let gated = if confidence_gated {
                let alpha = confidence_alpha_q8(input, width, height, x, y, orientation);
                center + round_q8((enhanced - center) * alpha)
            } else {
                enhanced
            };
            output[y * width + x] = gated.clamp(i32::from(minimum), i32::from(maximum)) as u8;
        }
    }
    Ok(output)
}

fn confidence_alpha_q8(
    input: &[u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    orientation: EdgeOrientation,
) -> i32 {
    let ((tangent_x, tangent_y), (normal_x, normal_y)) = match orientation {
        EdgeOrientation::Flat => return 0,
        EdgeOrientation::Horizontal => ((1, 0), (0, 1)),
        EdgeOrientation::Vertical => ((0, 1), (1, 0)),
        EdgeOrientation::DiagonalDown => ((1, 1), (1, -1)),
        EdgeOrientation::DiagonalUp => ((1, -1), (1, 1)),
    };
    let normal_contrast = |tangent_offset: isize| {
        i32::from(sample(
            input,
            width,
            height,
            x,
            y,
            tangent_x * tangent_offset + normal_x,
            tangent_y * tangent_offset + normal_y,
        )) - i32::from(sample(
            input,
            width,
            height,
            x,
            y,
            tangent_x * tangent_offset - normal_x,
            tangent_y * tangent_offset - normal_y,
        ))
    };
    let before = normal_contrast(-1);
    let center = normal_contrast(0);
    let after = normal_contrast(1);
    if center == 0 || before.signum() != center.signum() || after.signum() != center.signum() {
        return 0;
    }
    let tangent_disagreement =
        (i32::from(sample(input, width, height, x, y, tangent_x, tangent_y))
            - i32::from(sample(input, width, height, x, y, -tangent_x, -tangent_y)))
        .abs();
    let contrast_disagreement = (before - center).abs() + (after - center).abs();
    let evidence = center.abs() - tangent_disagreement - (contrast_disagreement + 1) / 2;
    confidence_ramp_q8(evidence)
}

fn confidence_ramp_q8(evidence: i32) -> i32 {
    const ZERO_EVIDENCE: i32 = 8;
    const FULL_EVIDENCE: i32 = 48;
    if evidence <= ZERO_EVIDENCE {
        0
    } else if evidence >= FULL_EVIDENCE {
        256
    } else {
        ((evidence - ZERO_EVIDENCE) * 256 + (FULL_EVIDENCE - ZERO_EVIDENCE) / 2)
            / (FULL_EVIDENCE - ZERO_EVIDENCE)
    }
}

#[cfg(test)]
fn detect_edge_orientation_unchecked(
    input: &[u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
) -> EdgeOrientation {
    detect_edge_orientation_with_parameters(input, width, height, x, y, DEFAULT_QUALITY_PARAMETERS)
}

fn detect_edge_orientation_with_parameters(
    input: &[u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    parameters: QualityParameters,
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
    if magnitude_x.max(magnitude_y) < parameters.edge_threshold {
        EdgeOrientation::Flat
    } else if magnitude_y >= parameters.axis_dominance_ratio * magnitude_x {
        EdgeOrientation::Horizontal
    } else if magnitude_x >= parameters.axis_dominance_ratio * magnitude_y {
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
        ConfidenceGatedQualityPipeline, DEFAULT_QUALITY_PARAMETERS, EdgeOrientation,
        QualityParameters, QualityPipeline, SELECTED_UNGATED_PARAMETERS, SelectedQualityPipeline,
        confidence_alpha_q8, confidence_ramp_q8, detect_edge_orientation_unchecked,
        detect_edge_orientation_with_parameters, enhance_luma, enhance_luma_candidate,
        local_envelope,
    };
    use crate::algorithm::{BicubicBaseline, ExecutionPolicy, SuperResolution};
    use crate::fixtures::{HardEdge, checker_detail, constant, hard_edge, smooth_gradient};
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
    fn confidence_ramp_is_bounded_and_monotonic() {
        let values = (-32..=96).map(confidence_ramp_q8).collect::<Vec<_>>();
        assert!(values.iter().all(|&value| (0..=256).contains(&value)));
        assert!(values.windows(2).all(|window| window[0] <= window[1]));
        assert_eq!(confidence_ramp_q8(8), 0);
        assert_eq!(confidence_ramp_q8(48), 256);
    }

    #[test]
    fn confidence_vetoes_flat_and_irregular_texture() {
        let flat = [91; 25];
        assert_eq!(
            confidence_alpha_q8(&flat, 5, 5, 2, 2, EdgeOrientation::Flat),
            0
        );
        let checker = [
            0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255,
            0, 255, 0,
        ];
        assert_eq!(
            confidence_alpha_q8(&checker, 5, 5, 2, 2, EdgeOrientation::Vertical),
            0
        );
    }

    #[test]
    fn coherent_edge_keeps_the_selected_ungated_residual() {
        let plane = [
            0, 10, 120, 240, 255, 0, 10, 120, 240, 255, 0, 10, 120, 240, 255, 0, 10, 120, 240, 255,
            0, 10, 120, 240, 255,
        ];
        let orientation = detect_edge_orientation_with_parameters(
            &plane,
            5,
            5,
            1,
            2,
            SELECTED_UNGATED_PARAMETERS,
        );
        assert_eq!(orientation, EdgeOrientation::Vertical);
        assert_eq!(confidence_alpha_q8(&plane, 5, 5, 1, 2, orientation), 256);
        let ungated =
            enhance_luma_candidate(&plane, dimensions(5, 5), SELECTED_UNGATED_PARAMETERS, false)
                .unwrap();
        let gated =
            enhance_luma_candidate(&plane, dimensions(5, 5), SELECTED_UNGATED_PARAMETERS, true)
                .unwrap();
        assert_ne!(ungated[11], plane[11]);
        assert_eq!(gated[11], ungated[11]);
    }

    #[test]
    fn gated_candidate_respects_envelopes_and_execution_policies() {
        let plane = [0, 8, 32, 64, 128, 192, 224, 248, 255];
        let gated =
            enhance_luma_candidate(&plane, dimensions(3, 3), SELECTED_UNGATED_PARAMETERS, true)
                .unwrap();
        for y in 0..3 {
            for x in 0..3 {
                let (minimum, maximum) = local_envelope(&plane, 3, 3, x, y);
                assert!((minimum..=maximum).contains(&gated[y * 3 + x]));
            }
        }

        let input = checker_detail(dimensions(9, 7), 2).unwrap();
        let config = ProcessingConfig::new(input.dimensions());
        let pipeline = ConfidenceGatedQualityPipeline::new();
        let serial = pipeline
            .process_with_policy(&input, config, ExecutionPolicy::Serial)
            .unwrap();
        let parallel = pipeline
            .process_with_policy(&input, config, ExecutionPolicy::Parallel)
            .unwrap();
        assert_eq!(serial, parallel);
        assert_eq!(
            serial,
            pipeline
                .process_with_policy(&input, config, ExecutionPolicy::Serial)
                .unwrap()
        );
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
    fn explicit_default_parameters_preserve_the_public_pipeline_exactly() {
        let input = checker_detail(dimensions(7, 5), 2).unwrap();
        let config = ProcessingConfig::new(input.dimensions());
        let pipeline = QualityPipeline::new();
        let public = pipeline
            .process_with_policy(&input, config, ExecutionPolicy::Serial)
            .unwrap();
        let explicit = pipeline
            .process_with_parameters(
                &input,
                config,
                ExecutionPolicy::Serial,
                DEFAULT_QUALITY_PARAMETERS,
            )
            .unwrap();
        assert_eq!(explicit, public);
    }

    #[test]
    fn selected_pipeline_equals_the_explicit_validated_parameters() {
        let input = checker_detail(dimensions(9, 7), 2).unwrap();
        let config = ProcessingConfig::new(input.dimensions());
        let mut selected_outputs = Vec::new();
        for policy in [ExecutionPolicy::Serial, ExecutionPolicy::Parallel] {
            let selected = SelectedQualityPipeline::new()
                .process_with_policy(&input, config, policy)
                .unwrap();
            let explicit = QualityPipeline::new()
                .process_with_parameters(&input, config, policy, SELECTED_UNGATED_PARAMETERS)
                .unwrap();
            assert_eq!(selected, explicit);
            selected_outputs.push(selected);
        }
        assert_eq!(selected_outputs[0], selected_outputs[1]);
    }

    #[test]
    fn evaluation_parameters_are_validated_and_deterministic() {
        let input = smooth_gradient(dimensions(7, 5)).unwrap();
        let config = ProcessingConfig::new(input.dimensions());
        let pipeline = QualityPipeline::new();
        let invalid = QualityParameters {
            edge_threshold: -1,
            ..DEFAULT_QUALITY_PARAMETERS
        };
        assert!(
            pipeline
                .process_with_parameters(&input, config, ExecutionPolicy::Serial, invalid)
                .is_err()
        );

        let candidate = QualityParameters {
            edge_threshold: 32,
            directional_refine_gain_q8: 32,
            sharpen_gain_q8: 64,
            ..DEFAULT_QUALITY_PARAMETERS
        };
        let first = pipeline
            .process_with_parameters(&input, config, ExecutionPolicy::Serial, candidate)
            .unwrap();
        let second = pipeline
            .process_with_parameters(&input, config, ExecutionPolicy::Serial, candidate)
            .unwrap();
        assert_eq!(first, second);
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
    fn serial_and_parallel_quality_match_reference_exactly() {
        let inputs = [
            Image::new(dimensions(1, 1), vec![Rgb8::new(19, 37, 83)]).unwrap(),
            constant(dimensions(3, 5), Rgb8::new(91, 17, 203)).unwrap(),
            smooth_gradient(dimensions(7, 5)).unwrap(),
            hard_edge(dimensions(9, 7), HardEdge::Vertical).unwrap(),
            hard_edge(dimensions(7, 9), HardEdge::Horizontal).unwrap(),
            hard_edge(dimensions(1, 7), HardEdge::Horizontal).unwrap(),
            hard_edge(dimensions(7, 1), HardEdge::Vertical).unwrap(),
            checker_detail(dimensions(9, 3), 2).unwrap(),
        ];
        for input in inputs {
            let config = ProcessingConfig::new(input.dimensions());
            let reference = QualityPipeline::new()
                .process_reference(&input, config)
                .unwrap();
            for _ in 0..3 {
                assert_eq!(
                    QualityPipeline::new()
                        .process_with_policy(&input, config, ExecutionPolicy::Serial)
                        .unwrap(),
                    reference
                );
                assert_eq!(
                    QualityPipeline::new()
                        .process_with_policy(&input, config, ExecutionPolicy::Parallel)
                        .unwrap(),
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
