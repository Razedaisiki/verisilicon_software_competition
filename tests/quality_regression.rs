use verisilicon_sr::algorithm::{BicubicBaseline, QualityPipeline, SuperResolution};
use verisilicon_sr::fixtures::{HardEdge, checker_detail, constant, hard_edge, smooth_gradient};
use verisilicon_sr::image::{Image, Rgb8};
use verisilicon_sr::metrics::{Psnr, luma_psnr, luma_ssim};
use verisilicon_sr::spec::{Dimensions, ProcessingConfig};

fn dimensions(width: u32, height: u32) -> Dimensions {
    Dimensions::new(width, height).unwrap()
}

fn process<A: SuperResolution>(algorithm: A, input: &Image) -> Image {
    algorithm
        .process(input, ProcessingConfig::new(input.dimensions()))
        .unwrap()
}

#[test]
fn synthetic_pipelines_are_deterministic_with_exact_dimensions() {
    let fixtures = [
        constant(dimensions(8, 6), Rgb8::new(96, 96, 96)).unwrap(),
        smooth_gradient(dimensions(8, 6)).unwrap(),
        hard_edge(dimensions(8, 6), HardEdge::Vertical).unwrap(),
        checker_detail(dimensions(8, 6), 2).unwrap(),
    ];
    for input in fixtures {
        let baseline_first = process(BicubicBaseline::new(), &input);
        let baseline_second = process(BicubicBaseline::new(), &input);
        let quality_first = process(QualityPipeline::new(), &input);
        let quality_second = process(QualityPipeline::new(), &input);
        assert_eq!(baseline_first, baseline_second);
        assert_eq!(quality_first, quality_second);
        assert_eq!(baseline_first.dimensions(), dimensions(16, 12));
        assert_eq!(quality_first.dimensions(), dimensions(16, 12));
    }
}

#[test]
fn quality_candidate_stays_close_to_baseline_without_superiority_claim() {
    let input = hard_edge(dimensions(16, 12), HardEdge::Vertical).unwrap();
    let baseline = process(BicubicBaseline::new(), &input);
    let quality = process(QualityPipeline::new(), &input);
    let Psnr::Finite(psnr) = luma_psnr(&baseline, &quality).unwrap() else {
        panic!("quality candidate should differ from baseline on a hard edge");
    };
    let ssim = luma_ssim(&baseline, &quality).unwrap();
    assert!(psnr > 35.0, "diagnostic PSNR was {psnr}");
    assert!(ssim > 0.995, "diagnostic SSIM was {ssim}");
}

#[test]
fn smooth_fixture_metrics_meet_broad_regression_floors() {
    let input = smooth_gradient(dimensions(12, 8)).unwrap();
    let reference = smooth_gradient(dimensions(24, 16)).unwrap();
    let baseline = process(BicubicBaseline::new(), &input);
    let quality = process(QualityPipeline::new(), &input);

    for candidate in [&baseline, &quality] {
        let Psnr::Finite(psnr) = luma_psnr(&reference, candidate).unwrap() else {
            panic!("gradient diagnostic should be finite");
        };
        let ssim = luma_ssim(&reference, candidate).unwrap();
        assert!(psnr > 20.0, "diagnostic PSNR was {psnr}");
        assert!(ssim > 0.95, "diagnostic SSIM was {ssim}");
    }
}

#[test]
fn quality_hard_edge_luma_stays_inside_baseline_neighborhood() {
    let input = hard_edge(dimensions(8, 6), HardEdge::Horizontal).unwrap();
    let baseline = process(BicubicBaseline::new(), &input);
    let quality = process(QualityPipeline::new(), &input);
    let width = usize::try_from(baseline.dimensions().width()).unwrap();
    let height = usize::try_from(baseline.dimensions().height()).unwrap();
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
            let value = quality.pixels()[y * width + x].red;
            assert!(value >= minimum && value <= maximum);
        }
    }
}
