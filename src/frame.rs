use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Frame {
    pub path: PathBuf,
    pub blur_score: f64,
    pub timestamp: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameManifestEntry {
    pub index: usize,
    pub filename: String,
    pub blur_score: f64,
    pub phash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<BoundingBox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BoundingBox {
    pub corners: [[f32; 2]; 4],
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub mode: String,
    pub input_file: String,
    pub settings: serde_json::Value,
    pub status: String,
    pub total_candidates: usize,
    pub after_dedup: usize,
    pub frames: Vec<FrameManifestEntry>,
}

pub fn write_manifest(
    output_dir: &Path,
    mode: &str,
    input: &Path,
    settings: serde_json::Value,
    total_candidates: usize,
    entries: &[FrameManifestEntry],
    complete: bool,
) -> Result<(), String> {
    let manifest = Manifest {
        mode: mode.to_string(),
        input_file: input.to_string_lossy().to_string(),
        settings,
        status: if complete { "complete" } else { "processing" }.to_string(),
        total_candidates,
        after_dedup: entries.len(),
        frames: entries.to_vec(),
    };

    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize manifest: {}", e))?;

    fs::write(output_dir.join("manifest.json"), json)
        .map_err(|e| format!("Failed to write manifest: {}", e))?;

    Ok(())
}
