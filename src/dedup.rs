use std::path::Path;

use image::open;
use image_hasher::{HashAlg, HasherConfig, ImageHash};

use crate::frame::Frame;

pub fn compute_hash(path: &Path) -> Result<ImageHash, String> {
    let img = open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;

    let hasher = HasherConfig::new()
        .hash_alg(HashAlg::DoubleGradient)
        .hash_size(8, 8)
        .to_hasher();

    Ok(hasher.hash_image(&img))
}

pub fn hash_to_bytes(hash: &ImageHash) -> [u8; 8] {
    let bytes = hash.as_bytes();
    let mut arr = [0u8; 8];
    let len = bytes.len().min(8);
    arr[..len].copy_from_slice(&bytes[..len]);
    arr
}

pub fn hash_to_hex(bytes: &[u8; 8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn deduplicate(frames: &mut Vec<Frame>, hashes: &[ImageHash], threshold: u32) {
    if frames.is_empty() {
        return;
    }

    let mut keep = vec![true; frames.len()];

    for i in 0..frames.len() {
        if !keep[i] {
            continue;
        }

        for j in (i + 1)..frames.len() {
            if !keep[j] {
                continue;
            }

            let dist = hashes[i].dist(&hashes[j]);

            if dist <= threshold {
                if frames[j].blur_score > frames[i].blur_score {
                    keep[i] = false;
                    break;
                } else {
                    keep[j] = false;
                }
            }
        }
    }

    let mut idx = 0;
    frames.retain(|_| {
        let k = keep[idx];
        idx += 1;
        k
    });
}
