use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use verisilicon_sr::algorithm::{
    BicubicBaseline, BilinearChromaQualityPipeline, ConfidenceGatedQualityPipeline,
    QualityPipeline, RecommendedBaselineV1, SelectedQualityPipeline, SuperResolution,
};
use verisilicon_sr::image::Image;
use verisilicon_sr::io::ppm::PpmP6Codec;
use verisilicon_sr::metrics::{Psnr, luma_mssim, luma_psnr};
use verisilicon_sr::spec::ProcessingConfig;

const USAGE: &str = "Usage: paired_eval <pairs.tsv> <report.csv> [bicubic|recommended] [quality|selected-ungated|confidence-gated|bilinear-chroma]";
const HEADER: &str = "record_type,pipeline,id,lr_path,hr_path,width,height,image_count,infinite_psnr_count,psnr_y_db,ssim_y\n";

#[derive(Debug)]
struct EvalError(String);

impl fmt::Display for EvalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for EvalError {}

fn error(message: impl Into<String>) -> EvalError {
    EvalError(message.into())
}

#[derive(Clone, Debug)]
struct Pair {
    id: String,
    lr_text: String,
    hr_text: String,
    lr_path: PathBuf,
    hr_path: PathBuf,
}

#[derive(Clone, Copy)]
struct Metrics {
    psnr: Psnr,
    mssim: f64,
}

struct PairScores {
    pair: Pair,
    width: u32,
    height: u32,
    baseline: Metrics,
    candidate: Metrics,
    candidate_label: &'static str,
}

#[derive(Default)]
struct CompensatedSum {
    sum: f64,
    compensation: f64,
}

impl CompensatedSum {
    fn add(&mut self, value: f64) {
        let next = self.sum + value;
        if self.sum.abs() >= value.abs() {
            self.compensation += (self.sum - next) + value;
        } else {
            self.compensation += (value - next) + self.sum;
        }
        self.sum = next;
    }

    fn total(&self) -> f64 {
        self.sum + self.compensation
    }
}

#[derive(Clone, Copy)]
struct DatasetMetrics {
    psnr: Psnr,
    mssim: f64,
    infinite_psnr_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BaselineSelection {
    Bicubic,
    Recommended,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CandidateSelection {
    Quality,
    SelectedUngated,
    ConfidenceGated,
    BilinearChroma,
}

impl CandidateSelection {
    const fn report_label(self) -> &'static str {
        match self {
            Self::Quality => "candidate",
            Self::SelectedUngated => "selected-ungated",
            Self::ConfidenceGated => "confidence-gated",
            Self::BilinearChroma => "bilinear-chroma",
        }
    }
}

fn parse_baseline(value: &str) -> Result<BaselineSelection, EvalError> {
    match value {
        "bicubic" => Ok(BaselineSelection::Bicubic),
        "recommended" => Ok(BaselineSelection::Recommended),
        _ => Err(error(format!(
            "invalid baseline selector {value:?}; expected bicubic or recommended"
        ))),
    }
}

fn parse_candidate(value: &str) -> Result<CandidateSelection, EvalError> {
    match value {
        "quality" => Ok(CandidateSelection::Quality),
        "selected-ungated" => Ok(CandidateSelection::SelectedUngated),
        "confidence-gated" => Ok(CandidateSelection::ConfidenceGated),
        "bilinear-chroma" => Ok(CandidateSelection::BilinearChroma),
        _ => Err(error(format!(
            "invalid candidate selector {value:?}; expected quality, selected-ungated, confidence-gated, or bilinear-chroma"
        ))),
    }
}

fn main() -> ExitCode {
    let arguments: Vec<OsString> = env::args_os().skip(1).collect();
    if !(2..=4).contains(&arguments.len()) {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let result = if let Some(value) = arguments.get(2) {
        let Some(value) = value.to_str() else {
            eprintln!("Error: baseline selector must be valid UTF-8");
            return ExitCode::from(2);
        };
        let baseline = match parse_baseline(value) {
            Ok(selection) => selection,
            Err(failure) => {
                eprintln!("Error: {failure}");
                return ExitCode::from(2);
            }
        };
        let candidate = match arguments.get(3) {
            Some(value) => {
                let Some(value) = value.to_str() else {
                    eprintln!("Error: candidate selector must be valid UTF-8");
                    return ExitCode::from(2);
                };
                match parse_candidate(value) {
                    Ok(selection) => selection,
                    Err(failure) => {
                        eprintln!("Error: {failure}");
                        return ExitCode::from(2);
                    }
                }
            }
            None => CandidateSelection::Quality,
        };
        run_with_selections(
            Path::new(&arguments[0]),
            Path::new(&arguments[1]),
            baseline,
            candidate,
        )
    } else {
        run(Path::new(&arguments[0]), Path::new(&arguments[1]))
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("Error: {failure}");
            ExitCode::from(1)
        }
    }
}

fn run(manifest: &Path, report: &Path) -> Result<(), EvalError> {
    run_with_baseline(manifest, report, BaselineSelection::Bicubic)
}

fn run_with_baseline(
    manifest: &Path,
    report: &Path,
    baseline: BaselineSelection,
) -> Result<(), EvalError> {
    run_with_selections(manifest, report, baseline, CandidateSelection::Quality)
}

fn run_with_selections(
    manifest: &Path,
    report: &Path,
    baseline: BaselineSelection,
    candidate: CandidateSelection,
) -> Result<(), EvalError> {
    if report.exists() {
        return Err(error(format!(
            "refusing to overwrite existing report: {}",
            report.display()
        )));
    }
    let pairs = load_and_validate_pairs(manifest)?;
    let mut scores = Vec::with_capacity(pairs.len());
    for pair in pairs {
        scores.push(score_pair(pair, baseline, candidate)?);
    }
    let report_bytes = render_report(&scores)?;
    write_atomic(report, &report_bytes)
}

fn load_and_validate_pairs(manifest: &Path) -> Result<Vec<Pair>, EvalError> {
    let bytes = fs::read(manifest)
        .map_err(|failure| error(format!("failed to read {}: {failure}", manifest.display())))?;
    if bytes.iter().any(|byte| *byte > 0x7f || *byte == 0) {
        return Err(error("pairs manifest must contain only ASCII text"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| error("pairs manifest is not ASCII"))?;
    let mut lines = text.lines();
    if lines.next() != Some("id\tlr_path\thr_path") {
        return Err(error("pairs manifest has an invalid header"));
    }
    let manifest_parent = manifest.parent().unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(manifest_parent).map_err(|failure| {
        error(format!(
            "failed to resolve manifest directory {}: {failure}",
            manifest_parent.display()
        ))
    })?;
    let mut ids = HashSet::new();
    let mut files = HashSet::new();
    let mut pairs = Vec::new();
    for (line_index, line) in lines.enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 3 {
            return Err(error(format!(
                "malformed pairs manifest row {}",
                line_index + 2
            )));
        }
        let id = fields[0];
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(error(format!("invalid pair ID at row {}", line_index + 2)));
        }
        if !ids.insert(id.to_owned()) {
            return Err(error(format!("duplicate pair ID: {id}")));
        }
        let lr_path = resolve_pair_path(&root, fields[1])?;
        let hr_path = resolve_pair_path(&root, fields[2])?;
        if !files.insert(lr_path.clone()) || !files.insert(hr_path.clone()) {
            return Err(error("pair manifest contains a duplicate image file"));
        }
        pairs.push(Pair {
            id: id.to_owned(),
            lr_text: fields[1].to_owned(),
            hr_text: fields[2].to_owned(),
            lr_path,
            hr_path,
        });
    }
    if pairs.is_empty() {
        return Err(error("pairs manifest contains no image pairs"));
    }
    pairs.sort_by(|left, right| left.id.cmp(&right.id));
    for pair in &pairs {
        let lr = decode_ppm(&pair.lr_path)?;
        let hr = decode_ppm(&pair.hr_path)?;
        let expected = lr
            .dimensions()
            .scaled(verisilicon_sr::spec::Scale::X2)
            .map_err(|failure| error(failure.to_string()))?;
        if hr.dimensions() != expected {
            return Err(error(format!(
                "pair {} dimension mismatch: LR 2x is {} by {}, HR is {} by {}",
                pair.id,
                expected.width(),
                expected.height(),
                hr.dimensions().width(),
                hr.dimensions().height()
            )));
        }
        if hr.dimensions().width() < 11 || hr.dimensions().height() < 11 {
            return Err(error(format!(
                "pair {} is too small for 11x11 MSSIM",
                pair.id
            )));
        }
    }
    Ok(pairs)
}

fn resolve_pair_path(root: &Path, text: &str) -> Result<PathBuf, EvalError> {
    if text.is_empty() || text.contains('\\') {
        return Err(error(format!("unsafe pair path: {text:?}")));
    }
    let relative = Path::new(text);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(error(format!("unsafe pair path: {text:?}")));
    }
    if !relative
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ppm"))
    {
        return Err(error(format!("unsupported pair image format: {text}")));
    }
    let resolved = fs::canonicalize(root.join(relative))
        .map_err(|failure| error(format!("failed to resolve pair path {text}: {failure}")))?;
    if !resolved.starts_with(root) {
        return Err(error(format!(
            "pair path escapes manifest directory: {text}"
        )));
    }
    let metadata = fs::metadata(&resolved)
        .map_err(|failure| error(format!("failed to inspect pair path {text}: {failure}")))?;
    if !metadata.is_file() {
        return Err(error(format!("pair path is not a regular file: {text}")));
    }
    Ok(resolved)
}

fn decode_ppm(path: &Path) -> Result<Image, EvalError> {
    let bytes = fs::read(path)
        .map_err(|failure| error(format!("failed to read {}: {failure}", path.display())))?;
    PpmP6Codec::new()
        .decode_bytes(&bytes)
        .map_err(|failure| error(format!("failed to decode {}: {failure}", path.display())))
}

fn score_pair(
    pair: Pair,
    baseline_selection: BaselineSelection,
    candidate_selection: CandidateSelection,
) -> Result<PairScores, EvalError> {
    let lr = decode_ppm(&pair.lr_path)?;
    let hr = decode_ppm(&pair.hr_path)?;
    let config = ProcessingConfig::new(lr.dimensions());
    let baseline = match baseline_selection {
        BaselineSelection::Bicubic => BicubicBaseline::new().process(&lr, config),
        BaselineSelection::Recommended => RecommendedBaselineV1::new().process(&lr, config),
    }
    .map_err(|failure| error(format!("baseline failed for {}: {failure}", pair.id)))?;
    let candidate = match candidate_selection {
        CandidateSelection::Quality => QualityPipeline::new().process(&lr, config),
        CandidateSelection::SelectedUngated => SelectedQualityPipeline::new().process(&lr, config),
        CandidateSelection::ConfidenceGated => {
            ConfidenceGatedQualityPipeline::new().process(&lr, config)
        }
        CandidateSelection::BilinearChroma => {
            BilinearChromaQualityPipeline::new().process(&lr, config)
        }
    }
    .map_err(|failure| error(format!("candidate failed for {}: {failure}", pair.id)))?;
    if baseline.dimensions() != hr.dimensions() || candidate.dimensions() != hr.dimensions() {
        return Err(error(format!(
            "generated dimensions differ from HR for {}",
            pair.id
        )));
    }
    let baseline_metrics = calculate_metrics(&hr, &baseline)?;
    let candidate_metrics = calculate_metrics(&hr, &candidate)?;
    Ok(PairScores {
        width: hr.dimensions().width(),
        height: hr.dimensions().height(),
        pair,
        baseline: baseline_metrics,
        candidate: candidate_metrics,
        candidate_label: candidate_selection.report_label(),
    })
}

fn calculate_metrics(reference: &Image, candidate: &Image) -> Result<Metrics, EvalError> {
    Ok(Metrics {
        psnr: luma_psnr(reference, candidate).map_err(|failure| error(failure.to_string()))?,
        mssim: luma_mssim(reference, candidate).map_err(|failure| error(failure.to_string()))?,
    })
}

fn dataset_metrics<'a>(metrics: impl Iterator<Item = &'a Metrics>, count: usize) -> DatasetMetrics {
    let mut psnr = CompensatedSum::default();
    let mut mssim = CompensatedSum::default();
    let mut infinite_psnr_count = 0;
    for value in metrics {
        match value.psnr {
            Psnr::Infinite => infinite_psnr_count += 1,
            Psnr::Finite(finite) => psnr.add(finite),
        }
        mssim.add(value.mssim);
    }
    DatasetMetrics {
        psnr: if infinite_psnr_count == 0 {
            Psnr::Finite(psnr.total() / count as f64)
        } else {
            Psnr::Infinite
        },
        mssim: mssim.total() / count as f64,
        infinite_psnr_count,
    }
}

fn render_report(scores: &[PairScores]) -> Result<Vec<u8>, EvalError> {
    let baseline = dataset_metrics(scores.iter().map(|score| &score.baseline), scores.len());
    let candidate = dataset_metrics(scores.iter().map(|score| &score.candidate), scores.len());
    let candidate_label = scores[0].candidate_label;
    let mut output = String::from(HEADER);
    for score in scores {
        append_image_row(&mut output, score, "baseline", score.baseline);
        append_image_row(&mut output, score, candidate_label, score.candidate);
    }
    append_dataset_row(&mut output, "baseline", scores.len(), baseline);
    append_dataset_row(&mut output, candidate_label, scores.len(), candidate);
    let psnr_delta = match (baseline.psnr, candidate.psnr) {
        (Psnr::Finite(baseline), Psnr::Finite(candidate)) => format!("{:.6}", candidate - baseline),
        _ => "undefined".to_owned(),
    };
    append_csv_row(
        &mut output,
        &[
            "dataset_delta".to_owned(),
            format!("{candidate_label}-minus-baseline"),
            "__dataset_delta__".to_owned(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            scores.len().to_string(),
            String::new(),
            psnr_delta,
            format!("{:.9}", candidate.mssim - baseline.mssim),
        ],
    );
    if !output.is_ascii() {
        return Err(error("report contains non-ASCII output"));
    }
    Ok(output.into_bytes())
}

fn append_image_row(output: &mut String, score: &PairScores, pipeline: &str, metrics: Metrics) {
    append_csv_row(
        output,
        &[
            "image".to_owned(),
            pipeline.to_owned(),
            score.pair.id.clone(),
            score.pair.lr_text.clone(),
            score.pair.hr_text.clone(),
            score.width.to_string(),
            score.height.to_string(),
            String::new(),
            String::new(),
            metrics.psnr.to_string(),
            format!("{:.9}", metrics.mssim),
        ],
    );
}

fn append_dataset_row(output: &mut String, pipeline: &str, count: usize, metrics: DatasetMetrics) {
    append_csv_row(
        output,
        &[
            "dataset_average".to_owned(),
            pipeline.to_owned(),
            "__dataset_average__".to_owned(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            count.to_string(),
            metrics.infinite_psnr_count.to_string(),
            metrics.psnr.to_string(),
            format!("{:.9}", metrics.mssim),
        ],
    );
}

fn append_csv_row(output: &mut String, fields: &[String]) {
    for (index, field) in fields.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        if field
            .bytes()
            .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'))
        {
            output.push('"');
            for character in field.chars() {
                if character == '"' {
                    output.push('"');
                }
                output.push(character);
            }
            output.push('"');
        } else {
            output.push_str(field);
        }
    }
    output.push('\n');
}

fn write_atomic(report: &Path, bytes: &[u8]) -> Result<(), EvalError> {
    let parent = report.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(error(format!(
            "report parent is not a directory: {}",
            parent.display()
        )));
    }
    let file_name = report
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| error("report path has no valid filename"))?;
    let mut temporary = None;
    for attempt in 0..100_u32 {
        let path = parent.join(format!(".{file_name}.tmp-{}-{attempt}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                temporary = Some((path, file));
                break;
            }
            Err(failure) if failure.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(failure) => {
                return Err(error(format!(
                    "failed to create report staging file: {failure}"
                )));
            }
        }
    }
    let (temporary_path, mut file) =
        temporary.ok_or_else(|| error("no unused report staging filename"))?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|failure| error(format!("failed to write report: {failure}")))?;
        file.sync_all()
            .map_err(|failure| error(format!("failed to flush report: {failure}")))?;
        drop(file);
        if report.exists() {
            return Err(error(format!(
                "refusing to overwrite existing report: {}",
                report.display()
            )));
        }
        fs::rename(&temporary_path, report)
            .map_err(|failure| error(format!("failed to publish report atomically: {failure}")))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        BaselineSelection, CandidateSelection, HEADER, Metrics, USAGE, dataset_metrics,
        load_and_validate_pairs, parse_baseline, parse_candidate, run, run_with_baseline,
        run_with_selections, write_atomic,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use verisilicon_sr::algorithm::{BicubicBaseline, SuperResolution};
    use verisilicon_sr::image::{Image, Rgb8};
    use verisilicon_sr::io::ppm::PpmP6Codec;
    use verisilicon_sr::metrics::Psnr;
    use verisilicon_sr::spec::{Dimensions, ProcessingConfig};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory(label: &str) -> PathBuf {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "verisilicon-paired-eval-{label}-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn lr(seed: u8) -> Image {
        let dimensions = Dimensions::new(6, 6).unwrap();
        let pixels = (0..36)
            .map(|index| {
                let value = seed.wrapping_add((index * 17) as u8);
                Rgb8::new(value, value.wrapping_add(11), value.wrapping_add(29))
            })
            .collect();
        Image::new(dimensions, pixels).unwrap()
    }

    fn write_ppm(path: &Path, image: &Image) {
        fs::write(path, PpmP6Codec::new().encode_bytes(image).unwrap()).unwrap();
    }

    fn add_pair(root: &Path, id: &str, seed: u8) -> (String, String) {
        let lr_path = root.join(format!("{id},lr.ppm"));
        let hr_path = root.join(format!("{id},hr.ppm"));
        let input = lr(seed);
        let reference = BicubicBaseline::new()
            .process(&input, ProcessingConfig::new(input.dimensions()))
            .unwrap();
        write_ppm(&lr_path, &input);
        write_ppm(&hr_path, &reference);
        (
            lr_path.file_name().unwrap().to_str().unwrap().to_owned(),
            hr_path.file_name().unwrap().to_str().unwrap().to_owned(),
        )
    }

    #[test]
    fn report_is_sorted_exact_non_overwriting_and_repeatable() {
        let root = temporary_directory("report");
        let (b_lr, b_hr) = add_pair(&root, "b", 7);
        let (a_lr, a_hr) = add_pair(&root, "a", 29);
        let manifest = root.join("pairs.tsv");
        fs::write(
            &manifest,
            format!("id\tlr_path\thr_path\nb\t{b_lr}\t{b_hr}\na\t{a_lr}\t{a_hr}\n"),
        )
        .unwrap();
        let first = root.join("first.csv");
        let second = root.join("second.csv");
        run(&manifest, &first).unwrap();
        run(&manifest, &second).unwrap();
        let bytes = fs::read(&first).unwrap();
        assert_eq!(bytes, fs::read(&second).unwrap());
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with(HEADER));
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 8);
        assert_eq!(
            lines[1],
            "image,baseline,a,\"a,lr.ppm\",\"a,hr.ppm\",12,12,,,inf,1.000000000"
        );
        assert!(lines[2].starts_with("image,candidate,a,"));
        assert_eq!(
            lines[3],
            "image,baseline,b,\"b,lr.ppm\",\"b,hr.ppm\",12,12,,,inf,1.000000000"
        );
        assert_eq!(
            lines[5],
            "dataset_average,baseline,__dataset_average__,,,,,2,2,inf,1.000000000"
        );
        assert!(lines[7].contains(",undefined,"));
        let sentinel = fs::read(&first).unwrap();
        assert!(
            run(&manifest, &first)
                .unwrap_err()
                .to_string()
                .contains("refusing to overwrite")
        );
        assert_eq!(fs::read(&first).unwrap(), sentinel);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manifest_rejects_malformed_duplicate_traversal_and_dimension_mismatch() {
        let root = temporary_directory("invalid");
        let (lr_path, hr_path) = add_pair(&root, "case", 3);
        let manifest = root.join("pairs.tsv");
        fs::write(&manifest, "wrong\n").unwrap();
        assert!(load_and_validate_pairs(&manifest).is_err());
        fs::write(
            &manifest,
            format!(
                "id\tlr_path\thr_path\ncase\t{lr_path}\t{hr_path}\ncase\t{lr_path}\t{hr_path}\n"
            ),
        )
        .unwrap();
        assert!(
            load_and_validate_pairs(&manifest)
                .unwrap_err()
                .to_string()
                .contains("duplicate pair ID")
        );
        fs::write(
            &manifest,
            format!(
                "id\tlr_path\thr_path\nfirst\t{lr_path}\t{hr_path}\nsecond\t{lr_path}\t{hr_path}\n"
            ),
        )
        .unwrap();
        assert!(
            load_and_validate_pairs(&manifest)
                .unwrap_err()
                .to_string()
                .contains("duplicate image file")
        );
        fs::write(
            &manifest,
            format!("id\tlr_path\thr_path\ncase\t../{lr_path}\t{hr_path}\n"),
        )
        .unwrap();
        assert!(
            load_and_validate_pairs(&manifest)
                .unwrap_err()
                .to_string()
                .contains("unsafe pair path")
        );
        let wrong_hr = Image::new(
            Dimensions::new(11, 12).unwrap(),
            vec![Rgb8::new(1, 2, 3); 132],
        )
        .unwrap();
        write_ppm(&root.join(&hr_path), &wrong_hr);
        fs::write(
            &manifest,
            format!("id\tlr_path\thr_path\ncase\t{lr_path}\t{hr_path}\n"),
        )
        .unwrap();
        assert!(
            load_and_validate_pairs(&manifest)
                .unwrap_err()
                .to_string()
                .contains("dimension mismatch")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validation_failure_publishes_no_report_or_staging_file() {
        let root = temporary_directory("cleanup");
        let lr_path = root.join("input.ppm");
        let hr_path = root.join("broken.ppm");
        write_ppm(&lr_path, &lr(1));
        fs::write(&hr_path, b"not ppm").unwrap();
        let manifest = root.join("pairs.tsv");
        fs::write(
            &manifest,
            "id\tlr_path\thr_path\ncase\tinput.ppm\tbroken.ppm\n",
        )
        .unwrap();
        let report = root.join("report.csv");
        assert!(run(&manifest, &report).is_err());
        assert!(!report.exists());
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")
        }));
        let occupied = root.join("occupied");
        fs::create_dir(&occupied).unwrap();
        assert!(write_atomic(&occupied, b"not published").is_err());
        assert!(!fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("occupied.tmp-")
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dataset_reduction_weights_images_equally_and_preserves_infinity() {
        let values = [
            Metrics {
                psnr: Psnr::Finite(10.0),
                mssim: 0.25,
            },
            Metrics {
                psnr: Psnr::Finite(30.0),
                mssim: 0.75,
            },
        ];
        let aggregate = dataset_metrics(values.iter(), values.len());
        assert_eq!(aggregate.psnr, Psnr::Finite(20.0));
        assert_eq!(aggregate.mssim, 0.5);
        assert_eq!(aggregate.infinite_psnr_count, 0);
        let infinite = [
            values[0],
            Metrics {
                psnr: Psnr::Infinite,
                mssim: 1.0,
            },
        ];
        let aggregate = dataset_metrics(infinite.iter(), infinite.len());
        assert_eq!(aggregate.psnr, Psnr::Infinite);
        assert_eq!(aggregate.infinite_psnr_count, 1);
    }

    #[test]
    fn selector_is_strict_default_is_compatible_and_recommended_is_used() {
        assert_eq!(
            parse_baseline("bicubic").unwrap(),
            BaselineSelection::Bicubic
        );
        assert_eq!(
            parse_baseline("recommended").unwrap(),
            BaselineSelection::Recommended
        );
        assert!(parse_baseline("other").is_err());
        assert_eq!(
            parse_candidate("quality").unwrap(),
            CandidateSelection::Quality
        );
        assert_eq!(
            parse_candidate("selected-ungated").unwrap(),
            CandidateSelection::SelectedUngated
        );
        assert_eq!(
            parse_candidate("confidence-gated").unwrap(),
            CandidateSelection::ConfidenceGated
        );
        assert_eq!(
            parse_candidate("bilinear-chroma").unwrap(),
            CandidateSelection::BilinearChroma
        );
        assert!(parse_candidate("other").is_err());
        assert!(USAGE.contains("bilinear-chroma"));
        let root = temporary_directory("recommended");
        let (lr_path, hr_path) = add_pair(&root, "case", 41);
        let manifest = root.join("pairs.tsv");
        fs::write(
            &manifest,
            format!("id\tlr_path\thr_path\ncase\t{lr_path}\t{hr_path}\n"),
        )
        .unwrap();
        let default_report = root.join("default.csv");
        let bicubic_report = root.join("bicubic.csv");
        let recommended_report = root.join("recommended.csv");
        let selected_report = root.join("selected.csv");
        let gated_report = root.join("gated.csv");
        let bilinear_chroma_report = root.join("bilinear-chroma.csv");
        run(&manifest, &default_report).unwrap();
        run_with_baseline(&manifest, &bicubic_report, BaselineSelection::Bicubic).unwrap();
        run_with_baseline(
            &manifest,
            &recommended_report,
            BaselineSelection::Recommended,
        )
        .unwrap();
        run_with_selections(
            &manifest,
            &selected_report,
            BaselineSelection::Bicubic,
            CandidateSelection::SelectedUngated,
        )
        .unwrap();
        run_with_selections(
            &manifest,
            &bilinear_chroma_report,
            BaselineSelection::Bicubic,
            CandidateSelection::BilinearChroma,
        )
        .unwrap();
        run_with_selections(
            &manifest,
            &gated_report,
            BaselineSelection::Bicubic,
            CandidateSelection::ConfidenceGated,
        )
        .unwrap();
        assert_eq!(
            fs::read(&default_report).unwrap(),
            fs::read(&bicubic_report).unwrap()
        );
        let recommended = fs::read_to_string(recommended_report).unwrap();
        let baseline_row = recommended.lines().nth(1).unwrap();
        assert!(baseline_row.starts_with("image,baseline,case,"));
        assert!(!baseline_row.contains(",inf,1.000000000"));
        for (report, label) in [
            (selected_report, "selected-ungated"),
            (gated_report, "confidence-gated"),
            (bilinear_chroma_report, "bilinear-chroma"),
        ] {
            let text = fs::read_to_string(report).unwrap();
            assert!(
                text.lines()
                    .any(|line| line.starts_with(&format!("image,{label},case,")))
            );
            assert!(text.lines().any(|line| line
                .starts_with(&format!("dataset_delta,{label}-minus-baseline,"))));
        }
        fs::remove_dir_all(root).unwrap();
    }
}
