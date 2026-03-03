use std::path::Path;

use image::{GrayImage, Luma, open};
use imageproc::contrast::stretch_contrast;
use imageproc::edges::canny;
use imageproc::filter::sharpen_gaussian;
use imageproc::geometric_transformations::{rotate_about_center, Interpolation};
use imageproc::hough::{detect_lines, LineDetectionOptions};

/// Optimize an image for maximum OCR accuracy.
/// Pipeline: deskew -> contrast normalize -> sharpen -> upscale.
/// Overwrites the file in place.
pub fn optimize_for_ocr(path: &Path, target_dpi: u32, verbose: bool) -> Result<(), String> {
    let img = open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let mut gray = img.to_luma8();

    // Step 1: Deskew — detect dominant text angle and correct rotation
    let skew = detect_skew_angle(&gray);
    if skew.abs() > 0.1 && skew.abs() < 15.0 {
        if verbose {
            eprintln!("  Deskewing: {:.2} degrees", skew);
        }
        let radians = skew * std::f32::consts::PI / 180.0;
        gray = rotate_about_center(&gray, radians, Interpolation::Bilinear, Luma([255]));
    } else if verbose && skew.abs() >= 15.0 {
        eprintln!("  Skew {:.1}° too large, skipping deskew", skew);
    }

    // Step 2: Adaptive contrast normalization
    // Use percentile-based stretch to handle uneven lighting better than min/max
    let (low, high) = percentile_intensity(&gray, 1.0, 99.0);
    if high > low + 20 {
        gray = stretch_contrast(&gray, low, high, 0, 255);
        if verbose {
            eprintln!("  Contrast: stretched [{}, {}] -> [0, 255]", low, high);
        }
    }

    // Step 3: Unsharp mask sharpening with tuned parameters for text
    // sigma=1.5 captures text-scale edges, amount=0.8 sharpens without ringing
    gray = sharpen_gaussian(&gray, 1.5, 0.8);

    // Step 4: DPI upscaling if resolution is too low for OCR
    // OCR engines want ~300 DPI. For a standard document, that means width > ~2500px.
    // For business cards (~3.5"), width > ~1050px.
    // Use the smaller threshold to avoid over-scaling small documents.
    let min_width = if target_dpi > 0 { (target_dpi as f64 * 3.5) as u32 } else { 0 };
    if min_width > 0 && gray.width() < min_width {
        let scale = min_width as f64 / gray.width() as f64;
        let new_w = (gray.width() as f64 * scale) as u32;
        let new_h = (gray.height() as f64 * scale) as u32;

        if verbose {
            eprintln!("  Upscaling {}x{} -> {}x{} (target {}dpi)", gray.width(), gray.height(), new_w, new_h, target_dpi);
        }

        let dynamic = image::DynamicImage::ImageLuma8(gray);
        let resized = dynamic.resize(new_w, new_h, image::imageops::FilterType::Lanczos3);
        gray = resized.to_luma8();
    }

    gray.save(path).map_err(|e| format!("Failed to save {}: {}", path.display(), e))
}

/// Detect the skew angle of text in an image using Hough line transform.
/// Returns degrees to rotate clockwise to correct the skew.
fn detect_skew_angle(gray: &GrayImage) -> f32 {
    // Work on a downscaled version for speed
    let scale = (gray.width().max(gray.height()) as f32 / 800.0).max(1.0);
    let work = if scale > 1.5 {
        let w = (gray.width() as f32 / scale) as u32;
        let h = (gray.height() as f32 / scale) as u32;
        let dynamic = image::DynamicImage::ImageLuma8(gray.clone());
        dynamic.resize(w, h, image::imageops::FilterType::Triangle).to_luma8()
    } else {
        gray.clone()
    };

    // Edge detection to find text edges
    let edges = canny(&work, 50.0, 150.0);

    // Detect lines — vote threshold scaled to image size
    let min_dim = work.width().min(work.height());
    let vote_threshold = (min_dim / 8).max(30);

    let options = LineDetectionOptions {
        vote_threshold,
        suppression_radius: 8,
    };

    let lines = detect_lines(&edges, options);

    if lines.is_empty() {
        return 0.0;
    }

    // Collect angles near horizontal (text lines are ~0° or ~180°)
    // and near vertical (page edges are ~90°)
    // We only care about near-horizontal for text skew
    let mut text_angles: Vec<f32> = Vec::new();

    for line in &lines {
        let angle = line.angle_in_degrees as f32;

        // Near horizontal: 0-15° or 165-180°
        if angle <= 15.0 {
            text_angles.push(angle);
        } else if angle >= 165.0 {
            text_angles.push(angle - 180.0); // Convert to negative small angle
        }
    }

    if text_angles.is_empty() {
        return 0.0;
    }

    // Use median angle for robustness against outliers
    text_angles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_idx = text_angles.len() / 2;
    -text_angles[median_idx] // Negate: if text tilts clockwise, we rotate counterclockwise
}

/// Find intensity values at given percentiles.
/// More robust than min/max — ignores outlier pixels from borders and noise.
fn percentile_intensity(img: &GrayImage, low_pct: f32, high_pct: f32) -> (u8, u8) {
    let mut histogram = [0u32; 256];

    // Sample the middle 90% to avoid border artifacts
    let x_start = img.width() / 20;
    let x_end = img.width() - x_start;
    let y_start = img.height() / 20;
    let y_end = img.height() - y_start;

    let mut total = 0u32;
    for y in y_start..y_end {
        for x in x_start..x_end {
            histogram[img.get_pixel(x, y).0[0] as usize] += 1;
            total += 1;
        }
    }

    if total == 0 {
        return (0, 255);
    }

    let low_target = (total as f32 * low_pct / 100.0) as u32;
    let high_target = (total as f32 * high_pct / 100.0) as u32;

    let mut low = 0u8;
    let mut high = 255u8;
    let mut cumulative = 0u32;

    for (i, &count) in histogram.iter().enumerate() {
        cumulative += count;
        if cumulative >= low_target && low == 0 && i > 0 {
            low = i as u8;
        }
        if cumulative >= high_target {
            high = i as u8;
            break;
        }
    }

    (low, high)
}
