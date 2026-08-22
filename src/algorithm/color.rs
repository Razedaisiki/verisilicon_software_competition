//! Deterministic fixed-point BT.601 full-range color conversion.

use crate::image::Rgb8;

/// One full-range BT.601 luma and chroma sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YCbCr8 {
    pub y: u8,
    pub cb: u8,
    pub cr: u8,
}

impl YCbCr8 {
    #[must_use]
    pub const fn new(y: u8, cb: u8, cr: u8) -> Self {
        Self { y, cb, cr }
    }
}

/// Converts RGB8 to full-range YCbCr8 using Q8 coefficients.
///
/// The equations are:
/// `Y = (77 R + 150 G + 29 B) / 256`,
/// `Cb = 128 + (-43 R - 85 G + 128 B) / 256`, and
/// `Cr = 128 + (128 R - 107 G - 21 B) / 256`.
/// Signed values are rounded to nearest with halves away from zero, then
/// clipped to the inclusive 8-bit range.
#[must_use]
pub fn rgb_to_ycbcr(pixel: Rgb8) -> YCbCr8 {
    let red = i32::from(pixel.red);
    let green = i32::from(pixel.green);
    let blue = i32::from(pixel.blue);
    let y = round_q8(77 * red + 150 * green + 29 * blue);
    let cb = 128 + round_q8(-43 * red - 85 * green + 128 * blue);
    let cr = 128 + round_q8(128 * red - 107 * green - 21 * blue);
    YCbCr8::new(clip_u8(y), clip_u8(cb), clip_u8(cr))
}

/// Converts full-range YCbCr8 to RGB8 using Q8 coefficients.
///
/// The equations are:
/// `R = Y + 359 (Cr - 128) / 256`,
/// `G = Y - (88 (Cb - 128) + 183 (Cr - 128)) / 256`, and
/// `B = Y + 454 (Cb - 128) / 256`.
/// Signed values are rounded to nearest with halves away from zero, then
/// clipped to the inclusive 8-bit range.
#[must_use]
pub fn ycbcr_to_rgb(pixel: YCbCr8) -> Rgb8 {
    let y = i32::from(pixel.y);
    let cb = i32::from(pixel.cb) - 128;
    let cr = i32::from(pixel.cr) - 128;
    let red = y + round_q8(359 * cr);
    let green = y - round_q8(88 * cb + 183 * cr);
    let blue = y + round_q8(454 * cb);
    Rgb8::new(clip_u8(red), clip_u8(green), clip_u8(blue))
}

fn round_q8(value: i32) -> i32 {
    if value >= 0 {
        (value + 128) >> 8
    } else {
        -((-value + 128) >> 8)
    }
}

fn clip_u8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::{YCbCr8, rgb_to_ycbcr, ycbcr_to_rgb};
    use crate::image::Rgb8;

    #[test]
    fn fixed_primary_vectors_match_documented_coefficients() {
        assert_eq!(rgb_to_ycbcr(Rgb8::new(0, 0, 0)), YCbCr8::new(0, 128, 128));
        assert_eq!(
            rgb_to_ycbcr(Rgb8::new(255, 255, 255)),
            YCbCr8::new(255, 128, 128)
        );
        assert_eq!(rgb_to_ycbcr(Rgb8::new(255, 0, 0)), YCbCr8::new(77, 85, 255));
        assert_eq!(rgb_to_ycbcr(Rgb8::new(0, 255, 0)), YCbCr8::new(149, 43, 21));
        assert_eq!(
            rgb_to_ycbcr(Rgb8::new(0, 0, 255)),
            YCbCr8::new(29, 255, 107)
        );
    }

    #[test]
    fn inverse_fixed_vectors_are_stable() {
        assert_eq!(ycbcr_to_rgb(YCbCr8::new(0, 128, 128)), Rgb8::new(0, 0, 0));
        assert_eq!(
            ycbcr_to_rgb(YCbCr8::new(255, 128, 128)),
            Rgb8::new(255, 255, 255)
        );
        assert_eq!(ycbcr_to_rgb(YCbCr8::new(77, 85, 255)), Rgb8::new(255, 1, 1));
    }

    #[test]
    fn color_round_trip_error_is_bounded_for_fixed_vectors() {
        let samples = [
            Rgb8::new(0, 0, 0),
            Rgb8::new(255, 255, 255),
            Rgb8::new(255, 0, 0),
            Rgb8::new(0, 255, 0),
            Rgb8::new(0, 0, 255),
            Rgb8::new(12, 34, 56),
            Rgb8::new(231, 117, 9),
        ];
        for sample in samples {
            let output = ycbcr_to_rgb(rgb_to_ycbcr(sample));
            assert!(sample.red.abs_diff(output.red) <= 2);
            assert!(sample.green.abs_diff(output.green) <= 2);
            assert!(sample.blue.abs_diff(output.blue) <= 2);
        }
    }
}
