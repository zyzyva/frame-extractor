use std::fs;
use std::path::{Path, PathBuf};

use image::open;
use rayon::prelude::*;

use crate::blur;
use crate::dedup;
use crate::frame::{self, BoundingBox, FrameManifestEntry};
use crate::perspective;
use crate::segment::{self, DetectedDocument, DetectionMethod};

pub struct SpreadConfig {
    pub min_area_pct: f64,
    pub max_area_pct: f64,
    pub method: DetectionMethod,
    pub no_perspective: bool,
    pub write_manifest: bool,
    pub verbose: bool,
}

pub struct SpreadResult {
    pub total_detected: usize,
    pub after_dedup: usize,
    pub output_frames: Vec<PathBuf>,
}

pub fn run(input: &Path, output_dir: &Path, config: &SpreadConfig) -> Result<SpreadResult, String> {
    fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output dir: {}", e))?;

    let img = open(input).map_err(|e| format!("Failed to open image: {}", e))?;
    let gray = img.to_luma8();

    if config.verbose {
        eprintln!("Image: {}x{}", gray.width(), gray.height());
        eprintln!("Detecting documents ({:?} method)...", config.method);
    }

    let documents = segment::detect_documents(
        &gray,
        config.method,
        config.min_area_pct,
        config.max_area_pct,
        config.verbose,
    );

    let total_detected = documents.len();

    if total_detected == 0 {
        if config.verbose {
            eprintln!("No documents detected");
        }
        return Ok(SpreadResult {
            total_detected: 0,
            after_dedup: 0,
            output_frames: vec![],
        });
    }

    let results: Vec<Option<(usize, FrameManifestEntry, PathBuf)>> = documents
        .par_iter()
        .enumerate()
        .map(|(idx, doc)| process_document(idx, doc, &img, output_dir, config))
        .collect();

    let mut entries: Vec<(usize, FrameManifestEntry, PathBuf)> = results
        .into_iter()
        .flatten()
        .collect();

    entries.sort_by_key(|(idx, _, _)| *idx);

    let settings = serde_json::json!({
        "min_area_pct": config.min_area_pct,
        "max_area_pct": config.max_area_pct,
        "method": format!("{:?}", config.method),
        "perspective_correction": !config.no_perspective,
    });

    let mut final_entries: Vec<FrameManifestEntry> = Vec::new();
    let mut final_paths: Vec<PathBuf> = Vec::new();

    for (_, entry, path) in entries {
        final_entries.push(entry);
        final_paths.push(path);

        if config.write_manifest {
            frame::write_manifest(
                output_dir, "spread", input, settings.clone(),
                total_detected, &final_entries, false,
            )?;
        }
    }

    if config.write_manifest {
        frame::write_manifest(
            output_dir, "spread", input, settings,
            total_detected, &final_entries, true,
        )?;
    }

    Ok(SpreadResult {
        total_detected,
        after_dedup: final_entries.len(),
        output_frames: final_paths,
    })
}

fn process_document(
    idx: usize,
    doc: &DetectedDocument,
    img: &image::DynamicImage,
    output_dir: &Path,
    config: &SpreadConfig,
) -> Option<(usize, FrameManifestEntry, PathBuf)> {
    let ordered = perspective::order_corners(&doc.corners);

    let filename = format!("doc_{:03}.png", idx + 1);
    let dest = output_dir.join(&filename);

    if config.no_perspective {
        let (min_x, min_y, max_x, max_y) = bounding_rect(&ordered);
        let cropped = img.crop_imm(min_x, min_y, max_x - min_x, max_y - min_y);
        cropped.save(&dest).ok()?;
    } else {
        let corrected = perspective::correct_perspective(img, &ordered)?;
        corrected.save(&dest).ok()?;
    }

    let blur_score = blur::blur_score(&dest).unwrap_or(0.0);
    let hash_hex = dedup::compute_hash(&dest)
        .map(|h| dedup::hash_to_hex_string(&h))
        .unwrap_or_default();

    let corners_arr = [
        [ordered[0].0, ordered[0].1],
        [ordered[1].0, ordered[1].1],
        [ordered[2].0, ordered[2].1],
        [ordered[3].0, ordered[3].1],
    ];

    let entry = FrameManifestEntry {
        index: idx + 1,
        filename,
        blur_score,
        phash: hash_hex,
        timestamp: None,
        bounds: Some(BoundingBox { corners: corners_arr }),
    };

    Some((idx, entry, dest))
}

fn bounding_rect(corners: &[(f32, f32); 4]) -> (u32, u32, u32, u32) {
    let min_x = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min) as u32;
    let min_y = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min) as u32;
    let max_x = corners.iter().map(|c| c.0).fold(f32::NEG_INFINITY, f32::max) as u32;
    let max_y = corners.iter().map(|c| c.1).fold(f32::NEG_INFINITY, f32::max) as u32;
    (min_x, min_y, max_x, max_y)
}
