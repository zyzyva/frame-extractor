use image::GrayImage;
use imageproc::contours::{find_contours, BorderType};
use imageproc::contrast::adaptive_threshold;
use imageproc::distance_transform::Norm;
use imageproc::edges::canny;
use imageproc::filter::gaussian_blur_f32;
use imageproc::geometry::{approximate_polygon_dp, contour_area, min_area_rect};
use imageproc::morphology::{close, dilate, open};
use imageproc::point::Point;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetectionMethod {
    Threshold,
    Edge,
}

#[derive(Debug, Clone)]
pub struct DetectedDocument {
    pub corners: [Point<i32>; 4],
    pub area: f64,
}

pub fn detect_documents(
    gray: &GrayImage,
    method: DetectionMethod,
    min_area_pct: f64,
    max_area_pct: f64,
    verbose: bool,
) -> Vec<DetectedDocument> {
    let blurred = gaussian_blur_f32(gray, 3.0);

    let binary = match method {
        DetectionMethod::Threshold => {
            let thresh = adaptive_threshold(&blurred, 15, -10);
            let closed = close(&thresh, Norm::L1, 3);
            open(&closed, Norm::L1, 2)
        }
        DetectionMethod::Edge => {
            let edges = canny(&blurred, 50.0, 150.0);
            let dilated = dilate(&edges, Norm::L1, 2);
            close(&dilated, Norm::L1, 3)
        }
    };

    let total_area = (gray.width() * gray.height()) as f64;
    let min_area = total_area * min_area_pct / 100.0;
    let max_area = total_area * max_area_pct / 100.0;

    let contours = find_contours::<i32>(&binary);

    if verbose {
        eprintln!("Found {} contours", contours.len());
    }

    let mut documents = Vec::new();

    for contour in &contours {
        if contour.border_type != BorderType::Outer {
            continue;
        }

        let area = contour_area(&contour.points);

        if area < min_area || area > max_area {
            continue;
        }

        let corners = find_rectangle(&contour.points);

        if let Some(corners) = corners {
            if verbose {
                eprintln!("  Document detected: area={:.0}, corners={:?}", area, corners);
            }
            documents.push(DetectedDocument { corners, area });
        }
    }

    if verbose {
        eprintln!("{} documents detected", documents.len());
    }

    documents
}

fn find_rectangle(points: &[Point<i32>]) -> Option<[Point<i32>; 4]> {
    // Try Douglas-Peucker with decreasing epsilon to get exactly 4 points
    let perimeter = approximate_perimeter(points);

    for factor in &[0.05, 0.04, 0.03, 0.02, 0.01] {
        let epsilon = perimeter * factor;
        let approx = approximate_polygon_dp(points, epsilon, true);

        if approx.len() == 4 {
            return Some([approx[0], approx[1], approx[2], approx[3]]);
        }
    }

    // Fallback: use min_area_rect
    if points.len() >= 3 {
        Some(min_area_rect(points))
    } else {
        None
    }
}

fn approximate_perimeter(points: &[Point<i32>]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }

    let mut perimeter = 0.0;
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        let dx = (points[j].x - points[i].x) as f64;
        let dy = (points[j].y - points[i].y) as f64;
        perimeter += (dx * dx + dy * dy).sqrt();
    }
    perimeter
}
