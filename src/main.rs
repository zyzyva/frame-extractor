mod blur;
mod dedup;
mod frame;
mod pipeline;
mod scene;

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "frame-extractor")]
#[command(about = "Extract clean, deduplicated document frames from video")]
struct Cli {
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
}

fn main() {
    let cli = Cli::parse();

    if !cli.input.exists() {
        eprintln!("Error: input file not found: {}", cli.input.display());
        std::process::exit(1);
    }

    let config = pipeline::PipelineConfig {
        scene_threshold: cli.scene_threshold,
        blur_threshold: cli.blur_threshold,
        dedup_threshold: cli.dedup_threshold,
        keep_all: cli.keep_all,
        dry_run: cli.dry_run,
        write_manifest: !cli.no_manifest,
        verbose: cli.verbose,
    };

    match pipeline::run(&cli.input, &cli.output, &config) {
        Ok(result) => {
            println!(
                "Done: {} candidates -> {} after blur rejection -> {} final frames",
                result.total_candidates, result.after_blur, result.after_dedup
            );
            if !result.output_frames.is_empty() {
                println!("Output: {}", cli.output.display());
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
