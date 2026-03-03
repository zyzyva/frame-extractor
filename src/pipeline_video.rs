use std::fs;
use std::path::{Path, PathBuf};

use image_hasher::ImageHash;
use rayon::prelude::*;

use crate::blur;
use crate::dedup;
use crate::frame::{Frame, FrameManifestEntry, Manifest};
use crate::scene;

pub struct PipelineConfig {
    pub scene_threshold: f64,
    pub blur_threshold: Option<f64>,
    pub dedup_threshold: u32,
    pub keep_all: bool,
    pub dry_run: bool,
    pub write_manifest: bool,
    pub verbose: bool,
}

pub struct PipelineResult {
    pub total_candidates: usize,
    pub after_blur: usize,
    pub after_dedup: usize,
    pub output_frames: Vec<PathBuf>,
}

pub fn run(input: &Path, output_dir: &Path, config: &PipelineConfig) -> Result<PipelineResult, String> {
    fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output dir: {}", e))?;

    let temp_dir = tempfile::tempdir()
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;

    if config.verbose {
        eprintln!("Extracting candidate frames (scene threshold: {})...", config.scene_threshold);
    }

    let candidate_paths = scene::extract_scene_frames(input, temp_dir.path(), config.scene_threshold)?;
    let total_candidates = candidate_paths.len();

    if config.verbose {
        eprintln!("Found {} candidate frames", total_candidates);
    }

    if total_candidates == 0 {
        return Ok(PipelineResult {
            total_candidates: 0,
            after_blur: 0,
            after_dedup: 0,
            output_frames: vec![],
        });
    }

    if config.verbose {
        eprintln!("Scoring sharpness and computing hashes in parallel...");
    }

    let scored: Vec<(Frame, ImageHash)> = candidate_paths
        .par_iter()
        .enumerate()
        .filter_map(|(idx, path)| {
            let score = blur::blur_score(path).ok()?;
            let hash = dedup::compute_hash(path).ok()?;

            if config.verbose {
                eprintln!("  frame {:04}: blur_score={:.1}", idx + 1, score);
            }

            Some((
                Frame {
                    path: path.clone(),
                    index: idx,
                    blur_score: score,
                    timestamp: None,
                },
                hash,
            ))
        })
        .collect();

    let all_scores: Vec<f64> = scored.iter().map(|(f, _)| f.blur_score).collect();
    let blur_threshold = config.blur_threshold.unwrap_or_else(|| blur::auto_threshold(&all_scores));

    if config.verbose {
        eprintln!("Blur threshold: {:.1}", blur_threshold);
    }

    let (mut frames, hashes): (Vec<Frame>, Vec<ImageHash>) = scored
        .into_iter()
        .filter(|(f, _)| f.blur_score >= blur_threshold)
        .unzip();

    let after_blur = frames.len();

    if config.verbose {
        eprintln!("{} frames passed blur rejection", after_blur);
    }

    if !config.keep_all {
        dedup::deduplicate(&mut frames, &hashes, config.dedup_threshold);
    }

    let after_dedup = frames.len();

    if config.verbose {
        eprintln!("{} frames after deduplication", after_dedup);
    }

    if config.dry_run {
        eprintln!("Dry run: would output {} frames", after_dedup);
        return Ok(PipelineResult {
            total_candidates,
            after_blur,
            after_dedup,
            output_frames: vec![],
        });
    }

    let mut output_frames = Vec::with_capacity(after_dedup);
    let mut manifest_entries = Vec::with_capacity(after_dedup);

    for (out_idx, frame) in frames.iter().enumerate() {
        let filename = format!("page_{:03}.png", out_idx + 1);
        let dest = output_dir.join(&filename);

        fs::copy(&frame.path, &dest)
            .map_err(|e| format!("Failed to copy frame: {}", e))?;

        let hash_hex = dedup::compute_hash(&dest)
            .map(|h| {
                let bytes = dedup::hash_to_bytes(&h);
                dedup::hash_to_hex(&bytes)
            })
            .unwrap_or_default();

        manifest_entries.push(FrameManifestEntry {
            index: out_idx + 1,
            filename: filename.clone(),
            blur_score: frame.blur_score,
            phash: hash_hex,
            timestamp: frame.timestamp,
            bounds: None,
        });

        output_frames.push(dest);

        if config.write_manifest {
            write_manifest(output_dir, input, config, blur_threshold, total_candidates, &manifest_entries, false)?;
        }
    }

    if config.write_manifest {
        write_manifest(output_dir, input, config, blur_threshold, total_candidates, &manifest_entries, true)?;
    }

    Ok(PipelineResult {
        total_candidates,
        after_blur,
        after_dedup,
        output_frames,
    })
}

fn write_manifest(
    output_dir: &Path,
    input: &Path,
    config: &PipelineConfig,
    blur_threshold: f64,
    total_candidates: usize,
    entries: &[FrameManifestEntry],
    complete: bool,
) -> Result<(), String> {
    let settings = serde_json::json!({
        "scene_threshold": config.scene_threshold,
        "blur_threshold": blur_threshold,
        "dedup_threshold": config.dedup_threshold,
    });

    let manifest = Manifest {
        mode: "video".to_string(),
        input_file: input.to_string_lossy().to_string(),
        settings,
        status: if complete { "complete".to_string() } else { "processing".to_string() },
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
