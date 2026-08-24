//! Deterministic fixed-point BT.601 full-range color conversion.

use crate::image::Rgb8;

const Q8_SHIFT: u32 = 8;
const Q8_ROUNDING_OFFSET: i32 = 1 << (Q8_SHIFT - 1);
const CHROMA_OFFSET: i32 = 128;

const RGB_TO_Y_RED_Q8: i32 = 77;
const RGB_TO_Y_GREEN_Q8: i32 = 150;
const RGB_TO_Y_BLUE_Q8: i32 = 29;
const RGB_TO_CB_RED_Q8: i32 = -43;
const RGB_TO_CB_GREEN_Q8: i32 = -85;
const RGB_TO_CB_BLUE_Q8: i32 = 128;
const RGB_TO_CR_RED_Q8: i32 = 128;
const RGB_TO_CR_GREEN_Q8: i32 = -107;
const RGB_TO_CR_BLUE_Q8: i32 = -21;

const YCBCR_TO_RED_CR_Q8: i32 = 359;
const YCBCR_TO_GREEN_CB_Q8: i32 = 88;
const YCBCR_TO_GREEN_CR_Q8: i32 = 183;
const YCBCR_TO_BLUE_CB_Q8: i32 = 454;

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
    let y = round_q8(RGB_TO_Y_RED_Q8 * red + RGB_TO_Y_GREEN_Q8 * green + RGB_TO_Y_BLUE_Q8 * blue);
    let cb = CHROMA_OFFSET
        + round_q8(RGB_TO_CB_RED_Q8 * red + RGB_TO_CB_GREEN_Q8 * green + RGB_TO_CB_BLUE_Q8 * blue);
    let cr = CHROMA_OFFSET
        + round_q8(RGB_TO_CR_RED_Q8 * red + RGB_TO_CR_GREEN_Q8 * green + RGB_TO_CR_BLUE_Q8 * blue);
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
    let cb = i32::from(pixel.cb) - CHROMA_OFFSET;
    let cr = i32::from(pixel.cr) - CHROMA_OFFSET;
    let red = y + round_q8(YCBCR_TO_RED_CR_Q8 * cr);
    let green = y - round_q8(YCBCR_TO_GREEN_CB_Q8 * cb + YCBCR_TO_GREEN_CR_Q8 * cr);
    let blue = y + round_q8(YCBCR_TO_BLUE_CB_Q8 * cb);
    Rgb8::new(clip_u8(red), clip_u8(green), clip_u8(blue))
}

fn round_q8(value: i32) -> i32 {
    if value >= 0 {
        (value + Q8_ROUNDING_OFFSET) >> Q8_SHIFT
    } else {
        -((-value + Q8_ROUNDING_OFFSET) >> Q8_SHIFT)
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
