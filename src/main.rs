mod blur;
mod dedup;
mod frame;
mod optimize;
mod perspective;
mod pipeline_spread;
mod pipeline_video;
mod scene;
mod segment;
mod upload;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::segment::DetectionMethod;
use crate::upload::R2Config;

#[derive(Parser)]
#[command(name = "frame-extractor")]
#[command(about = "Extract clean, deduplicated document frames from video or images")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Keep frames local only, skip R2 upload even if credentials are set
    #[arg(long, global = true)]
    local: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Extract frames from video using scene change detection
    Video {
        /// Input video file
        input: PathBuf,

        /// Output directory
        #[arg(short, long, default_value = "frames")]
        output: PathBuf,

        /// Scene change sensitivity (0.0 to 1.0, lower = more sensitive)
        #[arg(short, long, default_value = "0.08")]
        scene_threshold: f64,

        /// Minimum sharpness score (auto-calculated if not set)
        #[arg(short, long)]
        blur_threshold: Option<f64>,

        /// Max hamming distance for duplicate detection
        #[arg(short, long, default_value = "5")]
        dedup_threshold: u32,

        /// Output format: jpg or png
        #[arg(short, long, default_value = "jpg")]
        format: String,

        /// Skip deduplication, keep all sharp frames
        #[arg(long)]
        keep_all: bool,

        /// Show frame count without writing files
        #[arg(long)]
        dry_run: bool,

        /// Skip writing manifest.json
        #[arg(long)]
        no_manifest: bool,

        /// Skip OCR optimization, output raw frames
        #[arg(long)]
        raw: bool,

        /// Show per-frame scores
        #[arg(short, long)]
        verbose: bool,
    },

    /// Extract individual documents from an image of a spread (desk, table)
    Spread {
        /// Input image file
        input: PathBuf,

        /// Output directory
        #[arg(short, long, default_value = "frames")]
        output: PathBuf,

        /// Minimum document area as % of image
        #[arg(long, default_value = "1.0")]
        min_area: f64,

        /// Maximum document area as % of image
        #[arg(long, default_value = "90.0")]
        max_area: f64,

        /// Detection method: auto, threshold, or edge
        #[arg(long, default_value = "auto")]
        method: String,

        /// Output format: jpg or png
        #[arg(short, long, default_value = "jpg")]
        format: String,

        /// Skip perspective correction, just crop bounding box
        #[arg(long)]
        no_perspective: bool,

        /// Skip writing manifest.json
        #[arg(long)]
        no_manifest: bool,

        /// Skip OCR optimization, output raw frames
        #[arg(long)]
        raw: bool,

        /// Show detection details
        #[arg(short, long)]
        verbose: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    // Check R2 credentials on startup
    let r2_available = !cli.local && R2Config::from_env("").is_ok();

    if r2_available {
        let bucket = std::env::var("FRAME_EXTRACTOR_R2_BUCKET").unwrap_or_default();
        eprintln!("R2 upload enabled (bucket: {}). Use --local to keep frames local only.", bucket);
    }

    match cli.command {
        Command::Video {
            input,
            output,
            scene_threshold,
            blur_threshold,
            dedup_threshold,
            format,
            keep_all,
            dry_run,
            raw,
            no_manifest,
            verbose,
        } => {
            if !input.exists() {
                eprintln!("Error: input file not found: {}", input.display());
                std::process::exit(1);
            }

            let ext = parse_format(&format);

            let config = pipeline_video::PipelineConfig {
                scene_threshold,
                blur_threshold,
                dedup_threshold,
                output_ext: ext.to_string(),
                keep_all,
                dry_run,
                write_manifest: !no_manifest,
                verbose,
            };

            match pipeline_video::run(&input, &output, &config) {
                Ok(result) => {
                    if !raw && !result.output_frames.is_empty() {
                        if verbose {
                            eprintln!("Optimizing frames for OCR...");
                        }
                        optimize_frames(&result.output_frames, verbose);
                    }

                    if r2_available && !result.output_frames.is_empty() {
                        upload_frames_to_r2(&result.output_frames, &output, &input, "video", verbose);
                    }

                    println!(
                        "Done: {} candidates -> {} after blur rejection -> {} final frames",
                        result.total_candidates, result.after_blur, result.after_dedup
                    );
                    if !result.output_frames.is_empty() {
                        println!("Output: {}", output.display());
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }

        Command::Spread {
            input,
            output,
            min_area,
            max_area,
            method,
            format,
            no_perspective,
            raw,
            no_manifest,
            verbose,
        } => {
            if !input.exists() {
                eprintln!("Error: input file not found: {}", input.display());
                std::process::exit(1);
            }

            let ext = parse_format(&format);

            let detection_method = match method.as_str() {
                "threshold" => DetectionMethod::Threshold,
                "edge" => DetectionMethod::Edge,
                _ => DetectionMethod::Auto,
            };

            let config = pipeline_spread::SpreadConfig {
                min_area_pct: min_area,
                max_area_pct: max_area,
                method: detection_method,
                output_ext: ext.to_string(),
                no_perspective,
                write_manifest: !no_manifest,
                verbose,
            };

            match pipeline_spread::run(&input, &output, &config) {
                Ok(result) => {
                    if !raw && !result.output_frames.is_empty() {
                        if verbose {
                            eprintln!("Optimizing documents for OCR...");
                        }
                        optimize_frames(&result.output_frames, verbose);
                    }

                    if r2_available && !result.output_frames.is_empty() {
                        upload_frames_to_r2(&result.output_frames, &output, &input, "spread", verbose);
                    }

                    println!(
                        "Done: {} documents detected -> {} after dedup",
                        result.total_detected, result.after_dedup
                    );
                    if !result.output_frames.is_empty() {
                        println!("Output: {}", output.display());
                    }
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

fn parse_format(format: &str) -> &str {
    match format.to_lowercase().as_str() {
        "png" => "png",
        _ => "jpg",
    }
}

fn optimize_frames(frames: &[PathBuf], verbose: bool) {
    use rayon::prelude::*;

    frames.par_iter().for_each(|path| {
        if let Err(e) = optimize::optimize_for_ocr(path, 300, verbose) {
            eprintln!("  Warning: optimization failed for {}: {}", path.display(), e);
        }
    });
}

fn upload_frames_to_r2(
    frames: &[PathBuf],
    output_dir: &std::path::Path,
    input: &std::path::Path,
    mode: &str,
    verbose: bool,
) {
    // Generate a job prefix from input filename + timestamp
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("job");
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let prefix = format!("{}/{}-{}", mode, stem, timestamp);

    let r2_config = match R2Config::from_env(&prefix) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: R2 upload skipped: {}", e);
            return;
        }
    };

    if verbose {
        eprintln!("Uploading {} frames to R2 ({}/{})...", frames.len(), r2_config.bucket_name, prefix);
    }

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Upload frames and collect URLs
    let mut urls: Vec<(usize, String)> = Vec::new();
    for (idx, path) in frames.iter().enumerate() {
        let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or("frame.jpg");
        match rt.block_on(upload::upload_and_verify(&r2_config, path, filename, verbose)) {
            Ok(url) => urls.push((idx, url)),
            Err(e) => eprintln!("  Warning: failed to upload {}: {}", filename, e),
        }
    }

    // Update manifest with URLs
    let manifest_path = output_dir.join("manifest.json");
    if manifest_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&manifest_path) {
            if let Ok(mut manifest) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(entries) = manifest.get_mut("frames").and_then(|f| f.as_array_mut()) {
                    for (idx, url) in &urls {
                        if let Some(entry) = entries.get_mut(*idx) {
                            entry["url"] = serde_json::Value::String(url.clone());
                        }
                    }
                }
                if let Ok(json) = serde_json::to_string_pretty(&manifest) {
                    let _ = std::fs::write(&manifest_path, &json);
                    // Also upload the final manifest to R2
                    let _ = rt.block_on(upload::upload_manifest(&r2_config, &json));
                }
            }
        }
    }

    if verbose {
        eprintln!("{} frames uploaded to R2", urls.len());
    }
}
