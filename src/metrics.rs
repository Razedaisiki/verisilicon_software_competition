//! Provisional diagnostic luma image-quality metrics.
//!
//! These metrics are self-written diagnostics and are not asserted to match
//! the missing official scoring implementation. Luma is the deterministic Y
//! sample produced by the project's fixed-point BT.601 conversion.
//!
//! PSNR uses `10 * log10(255^2 / MSE)`, where MSE is the population mean of
//! squared luma error. Identical luma planes return [`Psnr::Infinite`].
//!
//! SSIM is one global population-statistics window over the complete luma
//! image, not a sliding-window or multiscale variant. It uses
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
    use super::{MetricError, Psnr, luma_psnr, luma_ssim};
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
    }
}
