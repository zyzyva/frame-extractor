use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Frame {
    pub path: PathBuf,
    pub index: usize,
    pub blur_score: f64,
    pub timestamp: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct FrameManifestEntry {
    pub index: usize,
    pub filename: String,
    pub blur_score: f64,
    pub phash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub input_file: String,
    pub settings: ManifestSettings,
    pub status: String,
    pub total_candidates: usize,
    pub after_blur_rejection: usize,
    pub after_dedup: usize,
    pub frames: Vec<FrameManifestEntry>,
}

#[derive(Debug, Serialize)]
pub struct ManifestSettings {
    pub scene_threshold: f64,
    pub blur_threshold: f64,
    pub dedup_threshold: u32,
}
