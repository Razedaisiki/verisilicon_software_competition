//! Provisional diagnostic luma image-quality metrics.
//!
//! These metrics are self-written diagnostics and are not asserted to match
//! the missing official scoring implementation. Luma is the deterministic Y
//! sample produced by the project's fixed-point BT.601 conversion.
//!
//! PSNR uses `10 * log10(255^2 / MSE)`, where MSE is the population mean of
//! squared luma error. Identical luma planes return [`Psnr::Infinite`].
//!
//! [`luma_ssim`] is the legacy single global population-statistics diagnostic.
//! [`luma_mssim`] is the separately versioned local 11x11 Gaussian development
//! metric defined by `docs/EVALUATION.md`. Both use
//! `((2 mx my + C1) (2 covariance + C2)) /
//!  ((mx^2 + my^2 + C1) (variance_x + variance_y + C2))`,
//! with population variance and covariance, `L = 255`, `K1 = 0.01`,
//! `K2 = 0.03`, `C1 = (K1 L)^2 = 6.5025`, and
//! `C2 = (K2 L)^2 = 58.5225`.

use crate::algorithm::color::rgb_to_ycbcr;
use crate::image::Image;
use crate::spec::Dimensions;
use std::fmt;

pub const SSIM_K1: f64 = 0.01;
pub const SSIM_K2: f64 = 0.03;
pub const SAMPLE_MAX: f64 = 255.0;
pub const SSIM_C1: f64 = 6.5025;
pub const SSIM_C2: f64 = 58.5225;
pub const MSSIM_WINDOW_SIZE: u32 = 11;
pub const MSSIM_Q20_KERNEL: [u64; 11] = [
    1_078, 7_968, 37_750, 114_673, 223_352, 278_934, 223_352, 114_673, 37_750, 7_968, 1_078,
];
const Q40_SCALE: f64 = 1_099_511_627_776.0;

/// Luma PSNR result with an explicit identical-image state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Psnr {
    Infinite,
    Finite(f64),
}

impl fmt::Display for Psnr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Infinite => formatter.write_str("inf"),
            Self::Finite(value) => write!(formatter, "{value:.6}"),
        }
    }
}

/// Errors returned by same-sized image metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetricError {
    DimensionMismatch {
        reference: Dimensions,
        candidate: Dimensions,
    },
    ImageTooSmallForMssim {
        dimensions: Dimensions,
    },
}

impl fmt::Display for MetricError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionMismatch {
                reference,
                candidate,
            } => write!(
                formatter,
                "metric dimension mismatch: reference is {} by {}, candidate is {} by {}",
                reference.width(),
                reference.height(),
                candidate.width(),
                candidate.height()
            ),
            Self::ImageTooSmallForMssim { dimensions } => write!(
                formatter,
                "MSSIM requires dimensions of at least {MSSIM_WINDOW_SIZE} by {MSSIM_WINDOW_SIZE}, received {} by {}",
                dimensions.width(),
                dimensions.height()
            ),
        }
    }
}

impl std::error::Error for MetricError {}

/// Computes provisional diagnostic luma PSNR.
pub fn luma_psnr(reference: &Image, candidate: &Image) -> Result<Psnr, MetricError> {
    require_same_dimensions(reference, candidate)?;
    let squared_error = reference
        .pixels()
        .iter()
        .zip(candidate.pixels())
        .map(|(&reference_pixel, &candidate_pixel)| {
            let reference_y = i32::from(rgb_to_ycbcr(reference_pixel).y);
            let candidate_y = i32::from(rgb_to_ycbcr(candidate_pixel).y);
            let difference = reference_y - candidate_y;
            let difference = i64::from(difference);
            (difference * difference) as u128
        })
        .sum::<u128>();
    if squared_error == 0 {
        return Ok(Psnr::Infinite);
    }
    let count = reference.pixels().len() as f64;
    let mean_squared_error = squared_error as f64 / count;
    Ok(Psnr::Finite(
        10.0 * ((SAMPLE_MAX * SAMPLE_MAX) / mean_squared_error).log10(),
    ))
}

/// Computes provisional diagnostic global luma SSIM.
pub fn luma_ssim(reference: &Image, candidate: &Image) -> Result<f64, MetricError> {
    require_same_dimensions(reference, candidate)?;
    let mut sum_reference = 0_u128;
    let mut sum_candidate = 0_u128;
    let mut sum_reference_squared = 0_u128;
    let mut sum_candidate_squared = 0_u128;
    let mut sum_product = 0_u128;
    for (&reference_pixel, &candidate_pixel) in reference.pixels().iter().zip(candidate.pixels()) {
        let reference_y = u128::from(rgb_to_ycbcr(reference_pixel).y);
        let candidate_y = u128::from(rgb_to_ycbcr(candidate_pixel).y);
        sum_reference += reference_y;
        sum_candidate += candidate_y;
        sum_reference_squared += reference_y * reference_y;
        sum_candidate_squared += candidate_y * candidate_y;
        sum_product += reference_y * candidate_y;
    }

    let count = reference.pixels().len() as f64;
    let mean_reference = sum_reference as f64 / count;
    let mean_candidate = sum_candidate as f64 / count;
    let variance_reference = sum_reference_squared as f64 / count - mean_reference * mean_reference;
    let variance_candidate = sum_candidate_squared as f64 / count - mean_candidate * mean_candidate;
    let covariance = sum_product as f64 / count - mean_reference * mean_candidate;
    let luminance = 2.0 * mean_reference * mean_candidate + SSIM_C1;
    let structure = 2.0 * covariance + SSIM_C2;
    let mean_energy = mean_reference * mean_reference + mean_candidate * mean_candidate + SSIM_C1;
    let variance_energy = variance_reference + variance_candidate + SSIM_C2;
    Ok((luminance * structure) / (mean_energy * variance_energy))
}

#[derive(Clone, Copy, Default)]
struct WeightedMoments {
    reference: u64,
    candidate: u64,
    reference_squared: u64,
    candidate_squared: u64,
    product: u64,
}

/// Computes the local 11x11 Gaussian mean luma SSIM development metric.
pub fn luma_mssim(reference: &Image, candidate: &Image) -> Result<f64, MetricError> {
    require_same_dimensions(reference, candidate)?;
    let dimensions = reference.dimensions();
    if dimensions.width() < MSSIM_WINDOW_SIZE || dimensions.height() < MSSIM_WINDOW_SIZE {
        return Err(MetricError::ImageTooSmallForMssim { dimensions });
    }
    let width = dimensions.width() as usize;
    let height = dimensions.height() as usize;
    let valid_width = width - MSSIM_WINDOW_SIZE as usize + 1;
    let valid_height = height - MSSIM_WINDOW_SIZE as usize + 1;
    let reference_y: Vec<u8> = reference
        .pixels()
        .iter()
        .map(|&pixel| rgb_to_ycbcr(pixel).y)
        .collect();
    let candidate_y: Vec<u8> = candidate
        .pixels()
        .iter()
        .map(|&pixel| rgb_to_ycbcr(pixel).y)
        .collect();
    let mut row_cache = vec![WeightedMoments::default(); valid_width * MSSIM_WINDOW_SIZE as usize];
    for source_y in 0..MSSIM_WINDOW_SIZE as usize {
        horizontal_moments(
            &reference_y,
            &candidate_y,
            width,
            source_y,
            valid_width,
            &mut row_cache[source_y * valid_width..(source_y + 1) * valid_width],
        );
    }
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for window_y in 0..valid_height {
        for window_x in 0..valid_width {
            let mut moments = WeightedMoments::default();
            for (tap, &weight) in MSSIM_Q20_KERNEL.iter().enumerate() {
                let cached = row_cache
                    [((window_y + tap) % MSSIM_WINDOW_SIZE as usize) * valid_width + window_x];
                moments.reference += cached.reference * weight;
                moments.candidate += cached.candidate * weight;
                moments.reference_squared += cached.reference_squared * weight;
                moments.candidate_squared += cached.candidate_squared * weight;
                moments.product += cached.product * weight;
            }
            neumaier_add(local_ssim(moments), &mut sum, &mut compensation);
        }
        let next_source_y = window_y + MSSIM_WINDOW_SIZE as usize;
        if next_source_y < height {
            let slot = next_source_y % MSSIM_WINDOW_SIZE as usize;
            horizontal_moments(
                &reference_y,
                &candidate_y,
                width,
                next_source_y,
                valid_width,
                &mut row_cache[slot * valid_width..(slot + 1) * valid_width],
            );
        }
    }
    Ok((sum + compensation) / (valid_width * valid_height) as f64)
}

fn horizontal_moments(
    reference: &[u8],
    candidate: &[u8],
    width: usize,
    source_y: usize,
    valid_width: usize,
    output: &mut [WeightedMoments],
) {
    let row_start = source_y * width;
    for window_x in 0..valid_width {
        let mut moments = WeightedMoments::default();
        for (tap, &weight) in MSSIM_Q20_KERNEL.iter().enumerate() {
            let reference_value = u64::from(reference[row_start + window_x + tap]);
            let candidate_value = u64::from(candidate[row_start + window_x + tap]);
            moments.reference += reference_value * weight;
            moments.candidate += candidate_value * weight;
            moments.reference_squared += reference_value * reference_value * weight;
            moments.candidate_squared += candidate_value * candidate_value * weight;
            moments.product += reference_value * candidate_value * weight;
        }
        output[window_x] = moments;
    }
}

fn local_ssim(moments: WeightedMoments) -> f64 {
    let mean_reference = moments.reference as f64 / Q40_SCALE;
    let mean_candidate = moments.candidate as f64 / Q40_SCALE;
    let variance_reference =
        (moments.reference_squared as f64 / Q40_SCALE - mean_reference * mean_reference).max(0.0);
    let variance_candidate =
        (moments.candidate_squared as f64 / Q40_SCALE - mean_candidate * mean_candidate).max(0.0);
    let covariance = moments.product as f64 / Q40_SCALE - mean_reference * mean_candidate;
    ((2.0 * mean_reference * mean_candidate + SSIM_C1) * (2.0 * covariance + SSIM_C2))
        / ((mean_reference * mean_reference + mean_candidate * mean_candidate + SSIM_C1)
            * (variance_reference + variance_candidate + SSIM_C2))
}

fn neumaier_add(value: f64, sum: &mut f64, compensation: &mut f64) {
    let next = *sum + value;
    if sum.abs() >= value.abs() {
        *compensation += (*sum - next) + value;
    } else {
        *compensation += (value - next) + *sum;
    }
    *sum = next;
}

fn require_same_dimensions(reference: &Image, candidate: &Image) -> Result<(), MetricError> {
    if reference.dimensions() != candidate.dimensions() {
        return Err(MetricError::DimensionMismatch {
            reference: reference.dimensions(),
            candidate: candidate.dimensions(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        MSSIM_Q20_KERNEL, MetricError, Psnr, SSIM_C1, SSIM_C2, luma_mssim, luma_psnr, luma_ssim,
    };
    use crate::algorithm::color::rgb_to_ycbcr;
    use crate::image::{Image, Rgb8};
    use crate::spec::Dimensions;

    fn image(width: u32, height: u32, values: &[u8]) -> Image {
        let pixels = values
            .iter()
            .map(|&value| Rgb8::new(value, value, value))
            .collect();
        Image::new(Dimensions::new(width, height).unwrap(), pixels).unwrap()
    }

    #[test]
    fn identical_images_have_infinite_psnr_and_unit_ssim() {
        let source = image(2, 1, &[0, 255]);
        assert_eq!(luma_psnr(&source, &source), Ok(Psnr::Infinite));
        assert!((luma_ssim(&source, &source).unwrap() - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn one_level_error_has_fixed_psnr_and_ssim() {
        let reference = image(1, 1, &[100]);
        let candidate = image(1, 1, &[101]);
        let Psnr::Finite(psnr) = luma_psnr(&reference, &candidate).unwrap() else {
            panic!("expected finite PSNR");
        };
        assert!((psnr - 48.130_803_608_679_1).abs() < 1.0e-10);
        assert!(
            (luma_ssim(&reference, &candidate).unwrap() - 0.999_950_513_429_356_3).abs() < 1.0e-12
        );
    }

    #[test]
    fn metrics_order_small_error_above_large_error() {
        let reference = image(2, 1, &[64, 192]);
        let small_error = image(2, 1, &[65, 191]);
        let large_error = image(2, 1, &[0, 255]);
        let Psnr::Finite(small_psnr) = luma_psnr(&reference, &small_error).unwrap() else {
            panic!("expected finite PSNR");
        };
        let Psnr::Finite(large_psnr) = luma_psnr(&reference, &large_error).unwrap() else {
            panic!("expected finite PSNR");
        };
        assert!(small_psnr > large_psnr);
        assert!(
            luma_ssim(&reference, &small_error).unwrap()
                > luma_ssim(&reference, &large_error).unwrap()
        );
    }

    #[test]
    fn dimension_mismatch_is_explicit() {
        let reference = image(1, 1, &[0]);
        let candidate = image(2, 1, &[0, 0]);
        let expected = MetricError::DimensionMismatch {
            reference: reference.dimensions(),
            candidate: candidate.dimensions(),
        };
        assert_eq!(luma_psnr(&reference, &candidate), Err(expected));
        assert_eq!(luma_ssim(&reference, &candidate), Err(expected));
        assert_eq!(luma_mssim(&reference, &candidate), Err(expected));
    }

    fn direct_mssim(reference: &Image, candidate: &Image) -> f64 {
        let width = reference.dimensions().width() as usize;
        let height = reference.dimensions().height() as usize;
        let mut sum = 0.0;
        let mut count = 0_usize;
        let scale = (1_u64 << 40) as f64;
        for y in 0..=height - 11 {
            for x in 0..=width - 11 {
                let mut sx = 0_u64;
                let mut sy = 0_u64;
                let mut sxx = 0_u64;
                let mut syy = 0_u64;
                let mut sxy = 0_u64;
                for (wy, &weight_y) in MSSIM_Q20_KERNEL.iter().enumerate() {
                    for (wx, &weight_x) in MSSIM_Q20_KERNEL.iter().enumerate() {
                        let weight = weight_y * weight_x;
                        let index = (y + wy) * width + x + wx;
                        let x_value = u64::from(rgb_to_ycbcr(reference.pixels()[index]).y);
                        let y_value = u64::from(rgb_to_ycbcr(candidate.pixels()[index]).y);
                        sx += x_value * weight;
                        sy += y_value * weight;
                        sxx += x_value * x_value * weight;
                        syy += y_value * y_value * weight;
                        sxy += x_value * y_value * weight;
                    }
                }
                let mx = sx as f64 / scale;
                let my = sy as f64 / scale;
                let vx = (sxx as f64 / scale - mx * mx).max(0.0);
                let vy = (syy as f64 / scale - my * my).max(0.0);
                let covariance = sxy as f64 / scale - mx * my;
                sum += ((2.0 * mx * my + SSIM_C1) * (2.0 * covariance + SSIM_C2))
                    / ((mx * mx + my * my + SSIM_C1) * (vx + vy + SSIM_C2));
                count += 1;
            }
        }
        sum / count as f64
    }

    #[test]
    fn mssim_kernel_and_minimum_dimensions_are_exact() {
        assert_eq!(MSSIM_Q20_KERNEL.iter().sum::<u64>(), 1_u64 << 20);
        assert_eq!(MSSIM_Q20_KERNEL, {
            let mut reversed = MSSIM_Q20_KERNEL;
            reversed.reverse();
            reversed
        });
        let too_small = image(10, 11, &[0; 110]);
        assert_eq!(
            luma_mssim(&too_small, &too_small),
            Err(MetricError::ImageTooSmallForMssim {
                dimensions: too_small.dimensions()
            })
        );
    }

    #[test]
    fn optimized_mssim_matches_independent_direct_oracle() {
        let values: Vec<u8> = (0..13 * 23)
            .map(|index| ((index * 37 + index / 7 * 19) & 255) as u8)
            .collect();
        let changed: Vec<u8> = values
            .iter()
            .enumerate()
            .map(|(index, value)| value.saturating_add((index % 5) as u8))
            .collect();
        let reference = image(13, 23, &values);
        let candidate = image(13, 23, &changed);
        let expected = direct_mssim(&reference, &candidate);
        let actual = luma_mssim(&reference, &candidate).unwrap();
        assert!(
            (actual - expected).abs() < 1.0e-15,
            "{actual} != {expected}"
        );
        assert_eq!(actual, luma_mssim(&reference, &candidate).unwrap());
    }

    #[test]
    fn constant_one_level_difference_has_analytic_mssim() {
        let reference = image(11, 11, &[100; 121]);
        let candidate = image(11, 11, &[101; 121]);
        let expected = (2.0 * 100.0 * 101.0 + SSIM_C1) / (100.0 * 100.0 + 101.0 * 101.0 + SSIM_C1);
        assert!((luma_mssim(&reference, &candidate).unwrap() - expected).abs() < 1.0e-15);
    }

    #[test]
    fn mssim_uses_only_valid_unpadded_windows() {
        let reference = image(12, 12, &[100; 144]);
        let mut values = vec![100; 144];
        for y in 0..12 {
            for x in 0..12 {
                if x < 5 || y < 5 || x >= 7 || y >= 7 {
                    values[y * 12 + x] = 0;
                }
            }
        }
        let candidate = image(12, 12, &values);
        assert!(luma_mssim(&reference, &candidate).unwrap() < 1.0);
        assert_eq!(luma_mssim(&reference, &reference).unwrap(), 1.0);
    }
}
