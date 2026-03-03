use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rayon::prelude::*;

use crate::blur;
use crate::dedup;
use crate::frame::{self, Frame, FrameManifestEntry};
use crate::scene;

pub struct PipelineConfig {
    pub scene_threshold: f64,
    pub blur_threshold: Option<f64>,
    pub dedup_threshold: u32,
    pub output_ext: String,
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

    // Stream frames from ffmpeg — start scoring as they arrive
    let rx = scene::extract_scene_frames_streaming(input, temp_dir.path(), config.scene_threshold)?;

    // Collect all paths (receiver is not Send for par_iter)
    let mut candidate_paths = Vec::new();
    while let Ok(path) = rx.recv_timeout(Duration::from_secs(30)) {
        candidate_paths.push(path);
    }

    let count = candidate_paths.len();

    if config.verbose {
        eprintln!("Found {} candidate frames", count);
    }

    if count == 0 {
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

    // Parallel blur scoring + hashing
    let scored_frames: Vec<_> = candidate_paths
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
                    blur_score: score,
                    timestamp: None,
                },
                hash,
            ))
        })
        .collect();

    let total = count;

    let all_scores: Vec<f64> = scored_frames.iter().map(|(f, _)| f.blur_score).collect();
    let blur_threshold = config.blur_threshold.unwrap_or_else(|| blur::auto_threshold(&all_scores));

    if config.verbose {
        eprintln!("Blur threshold: {:.1}", blur_threshold);
    }

    let (mut frames, hashes): (Vec<Frame>, Vec<_>) = scored_frames
        .into_iter()
        .filter(|(f, _)| f.blur_score >= blur_threshold)
        .unzip();

    let after_blur = frames.len();

    if config.verbose {
        eprintln!("{} frames passed blur rejection", after_blur);
    }

    let surviving_hashes = if config.keep_all {
        hashes
    } else {
        dedup::deduplicate(&mut frames, &hashes, config.dedup_threshold)
    };

    let after_dedup = frames.len();

    if config.verbose {
        eprintln!("{} frames after deduplication", after_dedup);
    }

    if config.dry_run {
        eprintln!("Dry run: would output {} frames", after_dedup);
        return Ok(PipelineResult {
            total_candidates: total,
            after_blur,
            after_dedup,
            output_frames: vec![],
        });
    }

    let settings = serde_json::json!({
        "scene_threshold": config.scene_threshold,
        "blur_threshold": blur_threshold,
        "dedup_threshold": config.dedup_threshold,
        "format": config.output_ext,
    });

    let mut output_frames = Vec::with_capacity(after_dedup);
    let mut manifest_entries = Vec::with_capacity(after_dedup);

    for (out_idx, (frame_ref, hash)) in frames.iter().zip(surviving_hashes.iter()).enumerate() {
        let filename = format!("page_{:03}.{}", out_idx + 1, config.output_ext);
        let dest = output_dir.join(&filename);

        if config.output_ext == "png" {
            fs::copy(&frame_ref.path, &dest)
                .map_err(|e| format!("Failed to copy frame: {}", e))?;
        } else {
            let img = image::open(&frame_ref.path)
                .map_err(|e| format!("Failed to open frame: {}", e))?;
            img.save(&dest)
                .map_err(|e| format!("Failed to save frame: {}", e))?;
        }

        manifest_entries.push(FrameManifestEntry {
            index: out_idx + 1,
            filename,
            blur_score: frame_ref.blur_score,
            phash: dedup::hash_to_hex_string(hash),
            timestamp: frame_ref.timestamp,
            bounds: None,
        });

        output_frames.push(dest);

        if config.write_manifest {
            frame::write_manifest(
                output_dir, "video", input, settings.clone(),
                total, &manifest_entries, false,
            )?;
        }
    }

    if config.write_manifest {
        frame::write_manifest(
            output_dir, "video", input, settings,
            total, &manifest_entries, true,
        )?;
    }

    Ok(PipelineResult {
        total_candidates: total,
        after_blur,
        after_dedup,
        output_frames,
    })
}
