use std::path::Path;

use image::open;
use imageproc::filter::laplacian_filter;

pub fn blur_score(path: &Path) -> Result<f64, String> {
    let img = open(path)
        .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?
        .to_luma8();

    let laplacian = laplacian_filter(&img);

    let pixels: Vec<f64> = laplacian.pixels().map(|p| p.0[0] as f64).collect();

    Ok(variance(&pixels))
}

pub fn auto_threshold(scores: &[f64]) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }

    let mut sorted = scores.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let q1_idx = sorted.len() / 4;
    sorted[q1_idx]
}

fn variance(data: &[f64]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let mean = data.iter().sum::<f64>() / data.len() as f64;
    let sum_sq_diff: f64 = data.iter().map(|x| (x - mean).powi(2)).sum();
    sum_sq_diff / data.len() as f64
}
