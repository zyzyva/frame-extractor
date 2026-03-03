use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use ffmpeg_sidecar::command::FfmpegCommand;

/// Extract scene-change frames from video, sending each frame path through
/// the channel as soon as ffmpeg writes it. This allows downstream processing
/// to start while ffmpeg is still extracting.
pub fn extract_scene_frames_streaming(
    input: &Path,
    temp_dir: &Path,
    scene_threshold: f64,
) -> Result<mpsc::Receiver<PathBuf>, String> {
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

    let (tx, rx) = mpsc::channel();
    let watch_dir = temp_dir.to_path_buf();

    // Watcher thread: polls the temp dir for new PNG files
    thread::spawn(move || {
        let mut seen = std::collections::HashSet::new();

        loop {
            if let Ok(entries) = std::fs::read_dir(&watch_dir) {
                let mut new_files: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().and_then(|e| e.to_str()) == Some("png") && !seen.contains(p)
                    })
                    .collect();

                new_files.sort();

                for path in new_files {
                    seen.insert(path.clone());
                    if tx.send(path).is_err() {
                        return; // Receiver dropped
                    }
                }
            }

            // Check if ffmpeg is done
            match child.as_inner_mut().try_wait() {
                Ok(Some(_)) => {
                    // ffmpeg finished — do one final scan for any remaining files
                    if let Ok(entries) = std::fs::read_dir(&watch_dir) {
                        let mut final_files: Vec<PathBuf> = entries
                            .filter_map(|e| e.ok())
                            .map(|e| e.path())
                            .filter(|p| {
                                p.extension().and_then(|e| e.to_str()) == Some("png")
                                    && !seen.contains(p)
                            })
                            .collect();

                        final_files.sort();

                        for path in final_files {
                            let _ = tx.send(path);
                        }
                    }
                    return;
                }
                Ok(None) => {} // Still running
                Err(_) => return,
            }

            thread::sleep(Duration::from_millis(50));
        }
    });

    Ok(rx)
}

