use std::path::{Path, PathBuf};

use ffmpeg_sidecar::command::FfmpegCommand;

pub fn extract_scene_frames(
    input: &Path,
    temp_dir: &Path,
    scene_threshold: f64,
) -> Result<Vec<PathBuf>, String> {
    let output_pattern = temp_dir.join("candidate_%04d.png");

    let filter = format!("select='gt(scene,{})',setpts=N/FRAME_RATE/TB", scene_threshold);

    let mut child = FfmpegCommand::new()
        .hide_banner()
        .input(input.to_string_lossy().as_ref())
        .args(["-vf", &filter])
        .args(["-fps_mode", "vfr"])
        .output(output_pattern.to_string_lossy().as_ref())
        .overwrite()
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

    let status = child
        .wait()
        .map_err(|e| format!("ffmpeg process error: {}", e))?;

    if !status.success() {
        return Err(format!("ffmpeg exited with status: {}", status));
    }

    let mut frames: Vec<PathBuf> = std::fs::read_dir(temp_dir)
        .map_err(|e| format!("Failed to read temp dir: {}", e))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("png") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    frames.sort();
    Ok(frames)
}
