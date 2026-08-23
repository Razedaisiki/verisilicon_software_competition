//! Deterministic bounded parameter sweeps with stratified cross-validation.
//!
//! Candidate selection sees training aggregates only. Validation metrics are
//! reported afterward, independently for PSNR and SSIM, without inventing a
//! cross-metric scalar objective.

use std::collections::{BTreeMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use verisilicon_sr::algorithm::ExecutionPolicy;
use verisilicon_sr::algorithm::quality::{
    DEFAULT_QUALITY_PARAMETERS, HISTORICAL_FINE_SWEEP_ANCHOR_PARAMETERS, QualityParameters,
    QualityPipeline,
};
use verisilicon_sr::image::Image;
use verisilicon_sr::io::ppm::PpmP6Codec;
use verisilicon_sr::metrics::{Psnr, luma_mssim, luma_psnr};
use verisilicon_sr::spec::{ProcessingConfig, Scale};

const USAGE: &str = "Usage: quality_sweep <pairs.tsv> <results.csv> [folds] [coarse|fine]";
const DEFAULT_FOLDS: usize = 5;
const COARSE_HEADER: &str = "record_type,fold,category,edge_threshold,axis_dominance_ratio,directional_refine_gain_q8,sharpen_gain_q8,image_count,mean_psnr_y_db,mean_ssim_y,delta_psnr_y_db_vs_default,delta_ssim_y_vs_default,training_psnr_rank,training_ssim_rank,training_pareto,selection\n";
const FINE_HEADER: &str = "record_type,fold,category,edge_threshold,axis_dominance_ratio,directional_refine_gain_q8,sharpen_gain_q8,image_count,mean_psnr_y_db,mean_ssim_y,delta_psnr_y_db_vs_selected,delta_ssim_y_vs_selected,training_psnr_rank,training_ssim_rank,training_pareto,selection\n";

#[derive(Debug)]
struct SweepError(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchSpace {
    Coarse,
    Fine,
}

impl SearchSpace {
    fn parse(value: &str) -> Result<Self, SweepError> {
        match value {
            "coarse" => Ok(Self::Coarse),
            "fine" => Ok(Self::Fine),
            _ => Err(error("search space must be coarse or fine")),
        }
    }
}

impl fmt::Display for SweepError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SweepError {}

fn error(message: impl Into<String>) -> SweepError {
    SweepError(message.into())
}

#[derive(Clone, Debug)]
struct Pair {
    id: String,
    category: String,
    lr_path: PathBuf,
    hr_path: PathBuf,
}

#[derive(Clone, Copy, Debug)]
struct Metrics {
    psnr: Psnr,
    mssim: f64,
}

#[derive(Clone, Copy, Debug)]
struct Aggregate {
    psnr: Psnr,
    mssim: f64,
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

fn main() -> ExitCode {
    let arguments: Vec<OsString> = env::args_os().skip(1).collect();
    if !(2..=4).contains(&arguments.len()) {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let folds = match arguments.get(2) {
        Some(value) => match value.to_str().and_then(|text| text.parse::<usize>().ok()) {
            Some(value) if value >= 2 => value,
            _ => {
                eprintln!("Error: folds must be an integer of at least 2");
                return ExitCode::from(2);
            }
        },
        None => DEFAULT_FOLDS,
    };
    let search_space = match arguments.get(3) {
        Some(value) => match value.to_str() {
            Some(value) => match SearchSpace::parse(value) {
                Ok(value) => value,
                Err(failure) => {
                    eprintln!("Error: {failure}");
                    return ExitCode::from(2);
                }
            },
            None => {
                eprintln!("Error: search space must be valid UTF-8");
                return ExitCode::from(2);
            }
        },
        None => SearchSpace::Coarse,
    };
    match run_with_space(
        Path::new(&arguments[0]),
        Path::new(&arguments[1]),
        folds,
        search_space,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("Error: {failure}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
fn run(manifest: &Path, output: &Path, folds: usize) -> Result<(), SweepError> {
    run_with_space(manifest, output, folds, SearchSpace::Coarse)
}

fn run_with_space(
    manifest: &Path,
    output: &Path,
    folds: usize,
    search_space: SearchSpace,
) -> Result<(), SweepError> {
    if output.exists() {
        return Err(error(format!(
            "refusing to overwrite sweep output: {}",
            output.display()
        )));
    }
    let pairs = load_pairs(manifest)?;
    let assignments = stratified_folds(&pairs, folds)?;
    let parameters = match search_space {
        SearchSpace::Coarse => coarse_parameters(),
        SearchSpace::Fine => fine_parameters(),
    };
    let mut metric_table = Vec::with_capacity(parameters.len());
    for &candidate in &parameters {
        let mut metrics = Vec::with_capacity(pairs.len());
        for pair in &pairs {
            metrics.push(evaluate_pair(pair, candidate)?);
        }
        metric_table.push(metrics);
    }

    let (header, comparison_parameters) = match search_space {
        SearchSpace::Coarse => (COARSE_HEADER, DEFAULT_QUALITY_PARAMETERS),
        SearchSpace::Fine => (FINE_HEADER, HISTORICAL_FINE_SWEEP_ANCHOR_PARAMETERS),
    };
    let mut output_text = String::from(header);
    let comparison_index = parameters
        .iter()
        .position(|parameters| *parameters == comparison_parameters)
        .ok_or_else(|| error("parameter space lost its comparison anchor"))?;
    let comparison_anchor = aggregate(&metric_table[comparison_index])?;
    let mut psnr_out_of_fold = vec![None; pairs.len()];
    let mut ssim_out_of_fold = vec![None; pairs.len()];
    for fold in 0..folds {
        let training: Vec<usize> = assignments
            .iter()
            .enumerate()
            .filter_map(|(index, assignment)| (*assignment != fold).then_some(index))
            .collect();
        let validation: Vec<usize> = assignments
            .iter()
            .enumerate()
            .filter_map(|(index, assignment)| (*assignment == fold).then_some(index))
            .collect();
        let training_aggregates: Vec<_> = metric_table
            .iter()
            .map(|metrics| aggregate_indices(metrics, &training))
            .collect::<Result<_, _>>()?;
        // Select before constructing validation aggregates so the data-flow
        // makes accidental validation-fold tuning difficult to introduce.
        let psnr_selected = select_psnr(&training_aggregates);
        let ssim_selected = select_ssim(&training_aggregates);
        let psnr_ranks = ranks(&training_aggregates, true);
        let ssim_ranks = ranks(&training_aggregates, false);
        let pareto = pareto_flags(&training_aggregates);
        let validation_aggregates: Vec<_> = metric_table
            .iter()
            .map(|metrics| aggregate_indices(metrics, &validation))
            .collect::<Result<_, _>>()?;
        for (candidate_index, &candidate) in parameters.iter().enumerate() {
            append_result_row(
                &mut output_text,
                "training_candidate",
                &fold.to_string(),
                "all",
                Some(candidate),
                training.len(),
                training_aggregates[candidate_index],
                training_aggregates[comparison_index],
                Some(psnr_ranks[candidate_index]),
                Some(ssim_ranks[candidate_index]),
                Some(pareto[candidate_index]),
                selection_label(candidate_index, psnr_selected, ssim_selected),
            );
            append_result_row(
                &mut output_text,
                "validation_candidate",
                &fold.to_string(),
                "all",
                Some(candidate),
                validation.len(),
                validation_aggregates[candidate_index],
                validation_aggregates[comparison_index],
                Some(psnr_ranks[candidate_index]),
                Some(ssim_ranks[candidate_index]),
                Some(pareto[candidate_index]),
                selection_label(candidate_index, psnr_selected, ssim_selected),
            );
            let mut categories = BTreeMap::<&str, Vec<usize>>::new();
            for &index in &validation {
                categories
                    .entry(&pairs[index].category)
                    .or_default()
                    .push(index);
            }
            for (category, indices) in categories {
                let category_anchor = aggregate_indices(&metric_table[comparison_index], &indices)?;
                append_result_row(
                    &mut output_text,
                    "validation_category",
                    &fold.to_string(),
                    category,
                    Some(candidate),
                    indices.len(),
                    aggregate_indices(&metric_table[candidate_index], &indices)?,
                    category_anchor,
                    Some(psnr_ranks[candidate_index]),
                    Some(ssim_ranks[candidate_index]),
                    Some(pareto[candidate_index]),
                    selection_label(candidate_index, psnr_selected, ssim_selected),
                );
            }
        }
        for &index in &validation {
            psnr_out_of_fold[index] = Some(metric_table[psnr_selected][index]);
            ssim_out_of_fold[index] = Some(metric_table[ssim_selected][index]);
        }
    }
    let psnr_out_of_fold: Vec<_> = psnr_out_of_fold
        .into_iter()
        .collect::<Option<_>>()
        .ok_or_else(|| error("PSNR cross-validation left an image unassigned"))?;
    let ssim_out_of_fold: Vec<_> = ssim_out_of_fold
        .into_iter()
        .collect::<Option<_>>()
        .ok_or_else(|| error("SSIM cross-validation left an image unassigned"))?;
    let psnr_cross_validation = aggregate(&psnr_out_of_fold)?;
    let ssim_cross_validation = aggregate(&ssim_out_of_fold)?;
    append_result_row(
        &mut output_text,
        "cross_validation",
        "all",
        "all",
        None,
        psnr_out_of_fold.len(),
        psnr_cross_validation,
        comparison_anchor,
        None,
        None,
        None,
        "psnr",
    );
    append_result_row(
        &mut output_text,
        "cross_validation",
        "all",
        "all",
        None,
        ssim_out_of_fold.len(),
        ssim_cross_validation,
        comparison_anchor,
        None,
        None,
        None,
        "ssim",
    );
    write_atomic(output, output_text.as_bytes())
}

fn coarse_parameters() -> Vec<QualityParameters> {
    let mut values = Vec::new();
    for edge_threshold in [32, 64] {
        for directional_refine_gain_q8 in [32, 64] {
            for sharpen_gain_q8 in [32, 64] {
                values.push(QualityParameters {
                    edge_threshold,
                    axis_dominance_ratio: 2,
                    directional_refine_gain_q8,
                    sharpen_gain_q8,
                });
            }
        }
    }
    values.push(DEFAULT_QUALITY_PARAMETERS);
    values.sort();
    values.dedup();
    values
}

fn fine_parameters() -> Vec<QualityParameters> {
    const SELECTED_NEIGHBORHOOD: [(i32, i32, i32, i32); 31] = [
        (64, 2, 32, 64),
        (48, 2, 32, 64),
        (56, 2, 32, 64),
        (72, 2, 32, 64),
        (80, 2, 32, 64),
        (64, 1, 32, 64),
        (64, 3, 32, 64),
        (64, 2, 16, 64),
        (64, 2, 24, 64),
        (64, 2, 40, 64),
        (64, 2, 48, 64),
        (64, 2, 32, 48),
        (64, 2, 32, 56),
        (64, 2, 32, 72),
        (64, 2, 32, 80),
        (48, 2, 32, 56),
        (48, 2, 32, 72),
        (56, 2, 32, 56),
        (56, 2, 32, 72),
        (72, 2, 32, 56),
        (72, 2, 32, 72),
        (80, 2, 32, 56),
        (80, 2, 32, 72),
        (64, 2, 24, 48),
        (64, 2, 24, 56),
        (64, 2, 24, 72),
        (64, 2, 24, 80),
        (64, 2, 40, 48),
        (64, 2, 40, 56),
        (64, 2, 40, 72),
        (64, 2, 40, 80),
    ];
    let mut values = SELECTED_NEIGHBORHOOD
        .into_iter()
        .map(|(edge, axis, directional, sharpen)| QualityParameters {
            edge_threshold: edge,
            axis_dominance_ratio: axis,
            directional_refine_gain_q8: directional,
            sharpen_gain_q8: sharpen,
        })
        .collect::<Vec<_>>();
    values.push(DEFAULT_QUALITY_PARAMETERS);
    values.sort();
    values.dedup();
    debug_assert!(values.contains(&HISTORICAL_FINE_SWEEP_ANCHOR_PARAMETERS));
    values
}

fn load_pairs(manifest: &Path) -> Result<Vec<Pair>, SweepError> {
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
    let parent = fs::canonicalize(manifest.parent().unwrap_or_else(|| Path::new(".")))
        .map_err(|failure| error(format!("failed to resolve manifest directory: {failure}")))?;
    let mut ids = HashSet::new();
    let mut files = HashSet::new();
    let mut pairs = Vec::new();
    for (line_index, line) in lines.enumerate() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 3 {
            return Err(error(format!("malformed manifest row {}", line_index + 2)));
        }
        let id = fields[0];
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || !ids.insert(id.to_owned())
        {
            return Err(error(format!("invalid or duplicate pair ID: {id:?}")));
        }
        let category = category_from_id(id)?;
        let lr_path = resolve_path(&parent, fields[1])?;
        let hr_path = resolve_path(&parent, fields[2])?;
        if !files.insert(lr_path.clone()) || !files.insert(hr_path.clone()) {
            return Err(error("pairs manifest contains a duplicate image file"));
        }
        let lr = decode_ppm(&lr_path)?;
        let hr = decode_ppm(&hr_path)?;
        let expected = lr
            .dimensions()
            .scaled(Scale::X2)
            .map_err(|failure| error(failure.to_string()))?;
        if hr.dimensions() != expected || expected.width() < 11 || expected.height() < 11 {
            return Err(error(format!("pair {id} has invalid LR/HR dimensions")));
        }
        pairs.push(Pair {
            id: id.to_owned(),
            category: category.to_owned(),
            lr_path,
            hr_path,
        });
    }
    if pairs.is_empty() {
        return Err(error("pairs manifest contains no pairs"));
    }
    pairs.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(pairs)
}

fn category_from_id(id: &str) -> Result<&str, SweepError> {
    let (category, _) = split_numeric_suffix(id).ok_or_else(|| {
        error(format!(
            "pair ID has no final numeric category suffix: {id}"
        ))
    })?;
    if category.is_empty()
        || category.ends_with(['_', '-'])
        || split_numeric_suffix(category).is_some()
    {
        return Err(error(format!(
            "pair ID has an ambiguous or missing category: {id}"
        )));
    }
    Ok(category)
}

fn split_numeric_suffix(value: &str) -> Option<(&str, &str)> {
    let delimiter = value.rfind(['_', '-'])?;
    let suffix = &value[delimiter + 1..];
    (!suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some((&value[..delimiter], suffix))
}

fn resolve_path(root: &Path, value: &str) -> Result<PathBuf, SweepError> {
    let relative = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || !relative
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ppm"))
    {
        return Err(error(format!("unsafe or unsupported pair path: {value:?}")));
    }
    let resolved = fs::canonicalize(root.join(relative))
        .map_err(|failure| error(format!("failed to resolve pair path {value}: {failure}")))?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err(error(format!(
            "pair path escapes dataset or is not a file: {value}"
        )));
    }
    Ok(resolved)
}

fn decode_ppm(path: &Path) -> Result<Image, SweepError> {
    let bytes = fs::read(path)
        .map_err(|failure| error(format!("failed to read {}: {failure}", path.display())))?;
    PpmP6Codec::new()
        .decode_bytes(&bytes)
        .map_err(|failure| error(format!("failed to decode {}: {failure}", path.display())))
}

fn stratified_folds(pairs: &[Pair], folds: usize) -> Result<Vec<usize>, SweepError> {
    let mut categories = BTreeMap::<&str, Vec<usize>>::new();
    for (index, pair) in pairs.iter().enumerate() {
        categories.entry(&pair.category).or_default().push(index);
    }
    if categories.values().any(|indices| indices.len() < folds) {
        return Err(error(
            "every category must contain at least as many pairs as folds",
        ));
    }
    let mut assignments = vec![0; pairs.len()];
    for indices in categories.values() {
        for (position, &index) in indices.iter().enumerate() {
            assignments[index] = position % folds;
        }
    }
    Ok(assignments)
}

fn evaluate_pair(pair: &Pair, parameters: QualityParameters) -> Result<Metrics, SweepError> {
    let lr = decode_ppm(&pair.lr_path)?;
    let hr = decode_ppm(&pair.hr_path)?;
    let output = QualityPipeline::new()
        .process_with_parameters(
            &lr,
            ProcessingConfig::new(lr.dimensions()),
            ExecutionPolicy::Auto,
            parameters,
        )
        .map_err(|failure| {
            error(format!(
                "quality processing failed for {}: {failure}",
                pair.id
            ))
        })?;
    Ok(Metrics {
        psnr: luma_psnr(&hr, &output).map_err(|failure| error(failure.to_string()))?,
        mssim: luma_mssim(&hr, &output).map_err(|failure| error(failure.to_string()))?,
    })
}

fn select_psnr(values: &[Aggregate]) -> usize {
    let mut best = 0;
    for index in 1..values.len() {
        if compare_psnr(values[index].psnr, values[best].psnr).is_gt() {
            best = index;
        }
    }
    best
}

fn select_ssim(values: &[Aggregate]) -> usize {
    let mut best = 0;
    for index in 1..values.len() {
        if values[index].mssim.total_cmp(&values[best].mssim).is_gt() {
            best = index;
        }
    }
    best
}

fn compare_psnr(left: Psnr, right: Psnr) -> std::cmp::Ordering {
    match (left, right) {
        (Psnr::Infinite, Psnr::Infinite) => std::cmp::Ordering::Equal,
        (Psnr::Infinite, Psnr::Finite(_)) => std::cmp::Ordering::Greater,
        (Psnr::Finite(_), Psnr::Infinite) => std::cmp::Ordering::Less,
        (Psnr::Finite(left), Psnr::Finite(right)) => left.total_cmp(&right),
    }
}

fn ranks(values: &[Aggregate], psnr: bool) -> Vec<usize> {
    values
        .iter()
        .map(|value| {
            1 + values
                .iter()
                .filter(|other| {
                    if psnr {
                        compare_psnr(other.psnr, value.psnr).is_gt()
                    } else {
                        other.mssim > value.mssim
                    }
                })
                .count()
        })
        .collect()
}

fn pareto_flags(values: &[Aggregate]) -> Vec<bool> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            !values.iter().enumerate().any(|(other_index, other)| {
                other_index != index
                    && !compare_psnr(other.psnr, value.psnr).is_lt()
                    && other.mssim >= value.mssim
                    && (compare_psnr(other.psnr, value.psnr).is_gt() || other.mssim > value.mssim)
            })
        })
        .collect()
}

fn selection_label(index: usize, psnr: usize, ssim: usize) -> &'static str {
    match (index == psnr, index == ssim) {
        (true, true) => "both",
        (true, false) => "psnr",
        (false, true) => "ssim",
        (false, false) => "none",
    }
}

fn aggregate_indices(metrics: &[Metrics], indices: &[usize]) -> Result<Aggregate, SweepError> {
    let selected: Vec<_> = indices.iter().map(|&index| metrics[index]).collect();
    aggregate(&selected)
}

fn aggregate(metrics: &[Metrics]) -> Result<Aggregate, SweepError> {
    if metrics.is_empty() {
        return Err(error("cannot aggregate an empty metric set"));
    }
    let mut psnr = CompensatedSum::default();
    let mut mssim = CompensatedSum::default();
    let mut infinite = false;
    for metric in metrics {
        match metric.psnr {
            Psnr::Infinite => infinite = true,
            Psnr::Finite(value) => psnr.add(value),
        }
        mssim.add(metric.mssim);
    }
    Ok(Aggregate {
        psnr: if infinite {
            Psnr::Infinite
        } else {
            Psnr::Finite(psnr.total() / metrics.len() as f64)
        },
        mssim: mssim.total() / metrics.len() as f64,
    })
}

#[allow(clippy::too_many_arguments)]
fn append_result_row(
    output: &mut String,
    record_type: &str,
    fold: &str,
    category: &str,
    parameters: Option<QualityParameters>,
    image_count: usize,
    metrics: Aggregate,
    comparison_metrics: Aggregate,
    psnr_rank: Option<usize>,
    ssim_rank: Option<usize>,
    pareto: Option<bool>,
    selection: &str,
) {
    let [edge, axis, directional, sharpen] = parameters.map_or_else(
        || [String::new(), String::new(), String::new(), String::new()],
        |parameters| {
            [
                parameters.edge_threshold.to_string(),
                parameters.axis_dominance_ratio.to_string(),
                parameters.directional_refine_gain_q8.to_string(),
                parameters.sharpen_gain_q8.to_string(),
            ]
        },
    );
    let delta_psnr = match (metrics.psnr, comparison_metrics.psnr) {
        (Psnr::Infinite, Psnr::Infinite) => "0.000000".to_owned(),
        (Psnr::Infinite, Psnr::Finite(_)) => "inf".to_owned(),
        (Psnr::Finite(_), Psnr::Infinite) => "-inf".to_owned(),
        (Psnr::Finite(value), Psnr::Finite(default)) => format!("{:.6}", value - default),
    };
    let delta_ssim = format!("{:.9}", metrics.mssim - comparison_metrics.mssim);
    let psnr_rank = psnr_rank.map(|value| value.to_string()).unwrap_or_default();
    let ssim_rank = ssim_rank.map(|value| value.to_string()).unwrap_or_default();
    let pareto = pareto.map(|value| value.to_string()).unwrap_or_default();
    output.push_str(&format!(
        "{record_type},{fold},{category},{edge},{axis},{directional},{sharpen},{image_count},{},{:.9},{delta_psnr},{delta_ssim},{psnr_rank},{ssim_rank},{pareto},{selection}\n",
        metrics.psnr,
        metrics.mssim,
    ));
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SweepError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(error("sweep output parent is not a directory"));
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| error("sweep output has no valid filename"))?;
    let temporary = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|failure| error(format!("failed to create sweep staging file: {failure}")))?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|failure| error(format!("failed to write sweep output: {failure}")))?;
        file.sync_all()
            .map_err(|failure| error(format!("failed to flush sweep output: {failure}")))?;
        drop(file);
        if path.exists() {
            return Err(error("refusing to overwrite sweep output"));
        }
        fs::rename(&temporary, path)
            .map_err(|failure| error(format!("failed to publish sweep output: {failure}")))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{
        Aggregate, Metrics, Pair, SearchSpace, USAGE, aggregate_indices, category_from_id,
        coarse_parameters, fine_parameters, pareto_flags, ranks, run, run_with_space, select_psnr,
        select_ssim, stratified_folds,
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

    fn temporary_directory() -> PathBuf {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "verisilicon-quality-sweep-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn write_pair(root: &Path, id: &str, seed: u8) -> String {
        let dimensions = Dimensions::new(6, 6).unwrap();
        let pixels = (0..36)
            .map(|index| {
                let value = seed.wrapping_add((index * 29) as u8);
                Rgb8::new(value, value.wrapping_add(17), value.wrapping_add(43))
            })
            .collect();
        let lr = Image::new(dimensions, pixels).unwrap();
        let hr = BicubicBaseline::new()
            .process(&lr, ProcessingConfig::new(dimensions))
            .unwrap();
        let lr_name = format!("{id}-lr.ppm");
        let hr_name = format!("{id}-hr.ppm");
        fs::write(
            root.join(&lr_name),
            PpmP6Codec::new().encode_bytes(&lr).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join(&hr_name),
            PpmP6Codec::new().encode_bytes(&hr).unwrap(),
        )
        .unwrap();
        format!("{id}\t{lr_name}\t{hr_name}\n")
    }

    #[test]
    fn coarse_space_is_small_stable_and_contains_default() {
        let values = coarse_parameters();
        assert_eq!(values.len(), 9);
        assert!(values.contains(&verisilicon_sr::algorithm::quality::DEFAULT_QUALITY_PARAMETERS));
        assert!(values.windows(2).all(|window| window[0] < window[1]));
    }

    #[test]
    fn fine_space_is_bounded_stable_and_centered_on_historical_anchor() {
        let values = fine_parameters();
        let selected = verisilicon_sr::algorithm::quality::HISTORICAL_FINE_SWEEP_ANCHOR_PARAMETERS;
        let frozen_default = verisilicon_sr::algorithm::quality::DEFAULT_QUALITY_PARAMETERS;
        assert_eq!(values.len(), 32);
        assert!(values.contains(&selected));
        assert!(values.contains(&frozen_default));
        assert!(values.contains(&verisilicon_sr::algorithm::quality::SELECTED_UNGATED_PARAMETERS));
        assert_ne!(
            selected,
            verisilicon_sr::algorithm::quality::SELECTED_UNGATED_PARAMETERS
        );
        assert!(values.windows(2).all(|window| window[0] < window[1]));

        let difference_count =
            |parameters: &verisilicon_sr::algorithm::quality::QualityParameters| {
                usize::from(parameters.edge_threshold != selected.edge_threshold)
                    + usize::from(parameters.axis_dominance_ratio != selected.axis_dominance_ratio)
                    + usize::from(
                        parameters.directional_refine_gain_q8
                            != selected.directional_refine_gain_q8,
                    )
                    + usize::from(parameters.sharpen_gain_q8 != selected.sharpen_gain_q8)
            };
        assert_eq!(
            values
                .iter()
                .filter(|parameters| difference_count(parameters) == 1)
                .count(),
            14
        );
        assert_eq!(
            values
                .iter()
                .filter(|parameters| difference_count(parameters) == 2)
                .count(),
            16
        );
        assert_eq!(SearchSpace::parse("coarse").unwrap(), SearchSpace::Coarse);
        assert_eq!(SearchSpace::parse("fine").unwrap(), SearchSpace::Fine);
        assert!(SearchSpace::parse("other").is_err());
        assert!(USAGE.contains("[coarse|fine]"));
    }

    #[test]
    fn eval30_categories_accept_underscore_or_hyphen_without_losing_text_ui() {
        assert_eq!(category_from_id("nature_01").unwrap(), "nature");
        assert_eq!(category_from_id("render-01").unwrap(), "render");
        assert_eq!(category_from_id("text_ui_01").unwrap(), "text_ui");
        assert!(category_from_id("text_ui").is_err());
        assert!(category_from_id("_01").is_err());
        assert!(category_from_id("nature_01-02").is_err());
        assert!(category_from_id("nature__01").is_err());
    }

    #[test]
    fn folds_are_stratified_and_selection_cannot_see_validation_values() {
        let pairs: Vec<_> = ["nature", "render", "text_ui"]
            .into_iter()
            .flat_map(|category| {
                (0..4).map(move |index| Pair {
                    id: format!("{category}-{index}"),
                    category: category.to_owned(),
                    lr_path: PathBuf::new(),
                    hr_path: PathBuf::new(),
                })
            })
            .collect();
        let assignments = stratified_folds(&pairs, 2).unwrap();
        for category in ["nature", "render", "text_ui"] {
            let counts = (0..2)
                .map(|fold| {
                    pairs
                        .iter()
                        .zip(&assignments)
                        .filter(|(pair, assignment)| {
                            pair.category == category && **assignment == fold
                        })
                        .count()
                })
                .collect::<Vec<_>>();
            assert_eq!(counts, [2, 2]);
        }
        let training = [0, 1];
        let mut table = vec![
            vec![
                Metrics {
                    psnr: Psnr::Finite(30.0),
                    mssim: 0.9
                };
                3
            ],
            vec![
                Metrics {
                    psnr: Psnr::Finite(20.0),
                    mssim: 0.8
                };
                3
            ],
        ];
        let training_aggregates = table
            .iter()
            .map(|metrics| aggregate_indices(metrics, &training).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(select_psnr(&training_aggregates), 0);
        assert_eq!(select_ssim(&training_aggregates), 0);
        table[1][2] = Metrics {
            psnr: Psnr::Infinite,
            mssim: 1.0,
        };
        let training_aggregates = table
            .iter()
            .map(|metrics| aggregate_indices(metrics, &training).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(select_psnr(&training_aggregates), 0);
        assert_eq!(select_ssim(&training_aggregates), 0);
    }

    #[test]
    fn end_to_end_output_is_deterministic_machine_readable_and_non_overwriting() {
        let root = temporary_directory();
        let mut manifest = String::from("id\tlr_path\thr_path\n");
        for category in ["nature", "render", "text_ui"] {
            for index in 0..2 {
                manifest.push_str(&write_pair(
                    &root,
                    &format!("{category}_{index:02}"),
                    (index * 41 + category.len()) as u8,
                ));
            }
        }
        let manifest_path = root.join("pairs.tsv");
        fs::write(&manifest_path, manifest).unwrap();
        let first = root.join("first.csv");
        let second = root.join("second.csv");
        let fine_first = root.join("fine-first.csv");
        let fine_second = root.join("fine-second.csv");
        run(&manifest_path, &first, 2).unwrap();
        run(&manifest_path, &second, 2).unwrap();
        let bytes = fs::read(&first).unwrap();
        assert_eq!(bytes, fs::read(&second).unwrap());
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.starts_with("record_type,fold,category,"));
        assert_eq!(text.matches("training_candidate").count(), 18);
        assert_eq!(text.matches("validation_candidate").count(), 18);
        assert_eq!(text.matches("validation_category").count(), 54);
        assert_eq!(text.matches("cross_validation").count(), 2);
        assert!(text.contains("delta_psnr_y_db_vs_default"));
        assert!(text.contains(",psnr\n"));
        assert!(text.contains(",ssim\n"));
        let cross_validation_rows = text
            .lines()
            .filter(|line| line.starts_with("cross_validation,"))
            .collect::<Vec<_>>();
        assert_eq!(cross_validation_rows.len(), 2);
        for row in cross_validation_rows {
            let fields = row.split(',').collect::<Vec<_>>();
            assert_eq!(fields.len(), 16);
            assert!(fields[3..7].iter().all(|field| field.is_empty()));
            assert!(fields[10].parse::<f64>().unwrap().is_finite());
            assert!(fields[11].parse::<f64>().unwrap().is_finite());
        }
        assert!(run(&manifest_path, &first, 2).is_err());
        assert_eq!(fs::read(first).unwrap(), bytes);

        run_with_space(&manifest_path, &fine_first, 2, SearchSpace::Fine).unwrap();
        run_with_space(&manifest_path, &fine_second, 2, SearchSpace::Fine).unwrap();
        let fine_bytes = fs::read(&fine_first).unwrap();
        assert_eq!(fine_bytes, fs::read(&fine_second).unwrap());
        let fine_text = String::from_utf8(fine_bytes).unwrap();
        assert!(fine_text.starts_with("record_type,fold,category,"));
        assert!(fine_text.contains("delta_psnr_y_db_vs_selected"));
        assert!(fine_text.contains("delta_ssim_y_vs_selected"));
        assert_eq!(fine_text.matches("training_candidate").count(), 64);
        assert_eq!(fine_text.matches("validation_candidate").count(), 64);
        assert_eq!(fine_text.matches("validation_category").count(), 192);
        assert_eq!(fine_text.matches("cross_validation").count(), 2);
        let selected_rows = fine_text
            .lines()
            .filter(|line| !line.starts_with("cross_validation,") && line.contains(",64,2,32,64,"))
            .collect::<Vec<_>>();
        assert_eq!(selected_rows.len(), 10);
        for row in selected_rows {
            let fields = row.split(',').collect::<Vec<_>>();
            assert_eq!(fields[10], "0.000000");
            assert_eq!(fields[11], "0.000000000");
        }
        assert!(run_with_space(&manifest_path, &fine_first, 2, SearchSpace::Fine).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rankings_and_pareto_front_keep_metrics_separate() {
        let values = [
            Aggregate {
                psnr: Psnr::Finite(30.0),
                mssim: 0.8,
            },
            Aggregate {
                psnr: Psnr::Finite(29.0),
                mssim: 0.9,
            },
            Aggregate {
                psnr: Psnr::Finite(28.0),
                mssim: 0.7,
            },
        ];
        assert_eq!(ranks(&values, true), [1, 2, 3]);
        assert_eq!(ranks(&values, false), [2, 1, 3]);
        assert_eq!(pareto_flags(&values), [true, true, false]);
        assert_eq!(select_psnr(&values), 0);
        assert_eq!(select_ssim(&values), 1);
    }
}
