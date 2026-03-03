mod blur;
mod dedup;
mod frame;
mod perspective;
mod pipeline_spread;
mod pipeline_video;
mod scene;
mod segment;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::segment::DetectionMethod;

#[derive(Parser)]
#[command(name = "frame-extractor")]
#[command(about = "Extract clean, deduplicated document frames from video or images")]
struct Cli {
    #[command(subcommand)]
    command: Command,
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
        #[arg(short, long, default_value = "0.3")]
        scene_threshold: f64,

        /// Minimum sharpness score (auto-calculated if not set)
        #[arg(short, long)]
        blur_threshold: Option<f64>,

        /// Max hamming distance for duplicate detection
        #[arg(short, long, default_value = "10")]
        dedup_threshold: u32,

        /// Skip deduplication, keep all sharp frames
        #[arg(long)]
        keep_all: bool,

        /// Show frame count without writing files
        #[arg(long)]
        dry_run: bool,

        /// Skip writing manifest.json
        #[arg(long)]
        no_manifest: bool,

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

        /// Detection method: threshold or edge
        #[arg(long, default_value = "threshold")]
        method: String,

        /// Skip perspective correction, just crop bounding box
        #[arg(long)]
        no_perspective: bool,

        /// Skip writing manifest.json
        #[arg(long)]
        no_manifest: bool,

        /// Show detection details
        #[arg(short, long)]
        verbose: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Video {
            input,
            output,
            scene_threshold,
            blur_threshold,
            dedup_threshold,
            keep_all,
            dry_run,
            no_manifest,
            verbose,
        } => {
            if !input.exists() {
                eprintln!("Error: input file not found: {}", input.display());
                std::process::exit(1);
            }

            let config = pipeline_video::PipelineConfig {
                scene_threshold,
                blur_threshold,
                dedup_threshold,
                keep_all,
                dry_run,
                write_manifest: !no_manifest,
                verbose,
            };

            match pipeline_video::run(&input, &output, &config) {
                Ok(result) => {
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
            no_perspective,
            no_manifest,
            verbose,
        } => {
            if !input.exists() {
                eprintln!("Error: input file not found: {}", input.display());
                std::process::exit(1);
            }

            let detection_method = match method.as_str() {
                "edge" => DetectionMethod::Edge,
                _ => DetectionMethod::Threshold,
            };

            let config = pipeline_spread::SpreadConfig {
                min_area_pct: min_area,
                max_area_pct: max_area,
                method: detection_method,
                no_perspective,
                write_manifest: !no_manifest,
                verbose,
            };

            match pipeline_spread::run(&input, &output, &config) {
                Ok(result) => {
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
