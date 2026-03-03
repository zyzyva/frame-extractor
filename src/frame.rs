use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Frame {
    pub path: PathBuf,
    pub index: usize,
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
