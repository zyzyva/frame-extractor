use image::{DynamicImage, RgbImage};
use imageproc::geometric_transformations::{warp_into, Interpolation, Projection};
use imageproc::point::Point;

pub fn order_corners(corners: &[Point<i32>; 4]) -> [(f32, f32); 4] {
    let mut pts: Vec<(f32, f32)> = corners.iter().map(|p| (p.x as f32, p.y as f32)).collect();

    // Sort by sum (x+y): smallest = top-left, largest = bottom-right
    pts.sort_by(|a, b| {
        let sum_a = a.0 + a.1;
        let sum_b = b.0 + b.1;
        sum_a.partial_cmp(&sum_b).unwrap()
    });

    let top_left = pts[0];
    let bottom_right = pts[3];

    // Of the remaining two, sort by difference (y-x): smaller = top-right, larger = bottom-left
    let mut middle = [pts[1], pts[2]];
    middle.sort_by(|a, b| {
        let diff_a = a.1 - a.0;
        let diff_b = b.1 - b.0;
        diff_a.partial_cmp(&diff_b).unwrap()
    });

    let top_right = middle[0];
    let bottom_left = middle[1];

    [top_left, top_right, bottom_right, bottom_left]
}

pub fn compute_output_dimensions(ordered: &[(f32, f32); 4]) -> (u32, u32) {
    let [tl, tr, br, bl] = *ordered;

    let width_top = distance(tl, tr);
    let width_bottom = distance(bl, br);
    let width = width_top.max(width_bottom) as u32;

    let height_left = distance(tl, bl);
    let height_right = distance(tr, br);
    let height = height_left.max(height_right) as u32;

    (width.max(1), height.max(1))
}

pub fn correct_perspective(
    image: &DynamicImage,
    ordered_corners: &[(f32, f32); 4],
) -> Option<RgbImage> {
    let (width, height) = compute_output_dimensions(ordered_corners);

    let dst: [(f32, f32); 4] = [
        (0.0, 0.0),
        (width as f32 - 1.0, 0.0),
        (width as f32 - 1.0, height as f32 - 1.0),
        (0.0, height as f32 - 1.0),
    ];

    // from_control_points maps dst->src (inverse mapping for warp)
    let projection = Projection::from_control_points(dst, *ordered_corners)?;

    let rgb = image.to_rgb8();
    let mut output = RgbImage::new(width, height);

    warp_into(
        &rgb,
        &projection,
        Interpolation::Bilinear,
        image::Rgb([0, 0, 0]),
        &mut output,
    );

    Some(output)
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    (dx * dx + dy * dy).sqrt()
}
