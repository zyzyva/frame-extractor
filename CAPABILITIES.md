# frame-extractor Capabilities

Rust CLI tool that extracts clean, deduplicated, OCR-optimized document frames from video or images. Single binary, no runtime dependencies beyond ffmpeg.

## Quick Reference

| Attribute | Value |
|---|---|
| Language | Rust (edition 2024) |
| Binary | `frame-extractor` |
| Runtime deps | ffmpeg (system) |
| Output | JPEG (default) or PNG images + manifest.json |
| Parallelism | rayon (all CPU cores) |
| Cloud storage | Cloudflare R2 (auto-detected from env) |

## Feature Matrix

| Category | Feature | Status |
|---|---|---|
| **Video Mode** | Scene change detection | Y |
| | Streaming ffmpeg extraction | Y |
| | Blur rejection (Laplacian variance) | Y |
| | Auto blur threshold (Q1 percentile) | Y |
| | Perceptual hash deduplication | Y |
| **Spread Mode** | Adaptive threshold segmentation | Y |
| | Canny edge segmentation | Y |
| | Auto method detection | Y |
| | Perspective correction | Y |
| | Contour-based rectangle detection | Y |
| **OCR Optimization** | Deskew (Hough line transform) | Y |
| | Percentile contrast normalization | Y |
| | Unsharp mask sharpening | Y |
| | DPI upscaling (Lanczos3) | Y |
| **Output** | JPEG output (default) | Y |
| | PNG output | Y |
| | Streaming manifest.json | Y |
| | Incremental manifest updates | Y |
| **Cloud** | R2 upload (auto-detected) | Y |
| | Upload retry (10x, exponential backoff) | Y |
| | HEAD verification (two-phase) | Y |
| | Manifest upload to R2 | Y |
| | `--local` flag to skip upload | Y |

---

## 1. Video Mode

Extract unique document frames from video of page flipping (books, documents, business cards).

```
frame-extractor video <input.mp4> [options]
```

### 1.1 Scene Change Detection
- [x] ffmpeg `select='gt(scene,threshold)'` filter via ffmpeg-sidecar
- [x] Streaming extraction — watcher thread polls temp dir as ffmpeg writes, sends frames via channel
- [x] Default threshold 0.08 (tuned for book page flips, not hard video cuts)
- [x] Configurable via `-s` / `--scene-threshold`
- **Implementation**: `src/scene.rs` — `extract_scene_frames_streaming()`

### 1.2 Blur Rejection
- [x] Laplacian filter (imageproc `laplacian_filter`) applied to grayscale frame
- [x] Variance of Laplacian output = sharpness score (high = sharp, low = blurry)
- [x] Auto threshold: Q1 (25th percentile) of all blur scores — rejects bottom quartile
- [x] Manual override via `-b` / `--blur-threshold`
- **Implementation**: `src/blur.rs` — `blur_score()`, `auto_threshold()`

### 1.3 Perceptual Hash Deduplication
- [x] DoubleGradient (dHash) algorithm via image_hasher, 8x8 hash size
- [x] Hamming distance comparison between all frame pairs
- [x] When duplicates found, keeps the frame with highest blur score (sharpest)
- [x] Default threshold 5 (tuned for text documents — tighter than general use)
- [x] Configurable via `-d` / `--dedup-threshold`
- [x] `--keep-all` to skip dedup entirely
- [x] Returns surviving hashes alongside surviving frames (avoids recomputation)
- **Implementation**: `src/dedup.rs` — `compute_hash()`, `deduplicate()`, `hash_to_hex_string()`

### 1.4 Parallel Processing
- [x] Blur scoring and hash computation run in parallel via rayon `par_iter`
- [x] Single pass: both operations done simultaneously per frame
- **Implementation**: `src/pipeline_video.rs` — `run()`

### Options
| Flag | Default | Description |
|---|---|---|
| `-s` | 0.08 | Scene change sensitivity (0.0-1.0, lower = more sensitive) |
| `-b` | auto | Minimum sharpness score |
| `-d` | 5 | Max hamming distance for duplicates |
| `-f` | jpg | Output format (jpg or png) |
| `-o` | frames | Output directory |
| `--keep-all` | false | Skip deduplication |
| `--dry-run` | false | Show counts without writing files |
| `--raw` | false | Skip OCR optimization |
| `--no-manifest` | false | Skip manifest.json |
| `-v` | false | Verbose output |

---

## 2. Spread Mode

Extract individual documents from a single image of documents on a surface (desk, table).

```
frame-extractor spread <input.jpg> [options]
```

### 2.1 Document Segmentation
- [x] **Threshold method**: Gaussian blur → adaptive threshold → morphological close/open → contour detection
- [x] **Edge method**: Gaussian blur → Canny edge detection → dilate → close → contour detection
- [x] **Auto method** (default): Runs both methods, picks whichever detects more documents
- [x] Area filtering: min 1%, max 90% of total image area (configurable)
- [x] Outer contours only (ignores holes)
- **Implementation**: `src/segment.rs` — `detect_documents()`, `detect_with_method()`

### 2.2 Rectangle Detection
- [x] Douglas-Peucker polygon simplification with decreasing epsilon (0.05 → 0.01 of perimeter)
- [x] Perimeter approximation via Euclidean segment distances
- [x] Targets exactly 4 vertices (rectangle)
- [x] Fallback: `min_area_rect` for contours that don't simplify to 4 points
- **Implementation**: `src/segment.rs` — `find_rectangle()`, `approximate_perimeter()`; struct `DetectedDocument { corners: [Point<i32>; 4] }`

### 2.3 Perspective Correction
- [x] Corner ordering: sorted by coordinate sum/difference (top-left, top-right, bottom-right, bottom-left)
- [x] Output dimensions computed from max corner distances
- [x] Homography via `Projection::from_control_points` (imageproc)
- [x] Bilinear interpolation warp
- [x] `--no-perspective` to skip correction and just crop bounding box (uses `bounding_rect()` fallback)
- **Implementation**: `src/perspective.rs` — `order_corners()`, `compute_output_dimensions()`, `correct_perspective()`

### 2.4 Parallel Processing
- [x] All document extractions (perspective correct + score + hash) run in parallel via rayon
- [x] Each document processed independently via `process_document()` — perspective correct, save, blur score, hash
- **Implementation**: `src/pipeline_spread.rs` — `run()`, `process_document()`

### Options
| Flag | Default | Description |
|---|---|---|
| `--method` | auto | Detection method (auto, threshold, edge) |
| `--min-area` | 1.0 | Minimum document area (% of image) |
| `--max-area` | 90.0 | Maximum document area (% of image) |
| `-f` | jpg | Output format (jpg or png) |
| `-o` | frames | Output directory |
| `--no-perspective` | false | Skip perspective correction |
| `--raw` | false | Skip OCR optimization |
| `--no-manifest` | false | Skip manifest.json |
| `-v` | false | Verbose output |

---

## 3. OCR Optimization

Post-extraction pipeline that optimizes frames for maximum OCR accuracy. Enabled by default on both modes, bypassed with `--raw`.

### 3.1 Deskew
- [x] Downscale to ~800px for speed
- [x] Canny edge detection to find text edges
- [x] Hough line transform to detect dominant line angles
- [x] Filters for near-horizontal lines (text lines at 0-15 or 165-180 degrees)
- [x] Median angle for outlier robustness
- [x] Rotation correction via `rotate_about_center` (bilinear interpolation)
- [x] Skips correction if angle < 0.1 degrees (no skew) or > 15 degrees (too large)
- **Implementation**: `src/optimize.rs` — `detect_skew_angle()`

### 3.2 Contrast Normalization
- [x] Percentile-based intensity stretch (1st to 99th percentile)
- [x] Samples middle 90% of image to avoid border artifacts
- [x] Histogram-based for efficiency
- [x] Skips if dynamic range is too narrow (< 20 levels)
- **Implementation**: `src/optimize.rs` — `percentile_intensity()`

### 3.3 Sharpening
- [x] Unsharp mask via `sharpen_gaussian` (sigma=1.5, amount=0.8)
- [x] Parameters tuned for text-scale edges without ringing
- **Implementation**: `src/optimize.rs` — `optimize_for_ocr()`

### 3.4 DPI Upscaling
- [x] Target: 300 DPI equivalent (minimum width 1050px, based on 3.5" business card)
- [x] Lanczos3 resampling for quality
- [x] Only upscales when resolution is below threshold
- **Implementation**: `src/optimize.rs` — `optimize_for_ocr()`

### 3.5 Grayscale Conversion
- [x] All frames converted to grayscale during optimization
- [x] Reduces file size and removes color noise for OCR

---

## 4. Output & Manifest

### 4.1 File Output
- [x] JPEG output by default (6x smaller than PNG, no OCR accuracy loss)
- [x] PNG output via `--format png`
- [x] Sequential naming: `page_001.jpg`, `page_002.jpg` (video) or `doc_001.jpg` (spread)
- [x] Temporal ordering preserved (video mode)

### 4.2 Streaming Manifest
- [x] `manifest.json` written to output directory
- [x] Updated incrementally after each frame — consumers can start processing immediately
- [x] `"status": "processing"` during extraction, `"complete"` when done
- [x] Consumers watch manifest to process frames in parallel with extraction
- **Implementation**: `src/frame.rs` — `write_manifest()`

### 4.3 Manifest Schema
```json
{
  "mode": "video" | "spread",
  "input_file": "/path/to/input",
  "settings": { ... },
  "status": "processing" | "complete",
  "total_candidates": 11,
  "after_dedup": 6,
  "frames": [
    {
      "index": 1,
      "filename": "page_001.jpg",
      "blur_score": 33.7,
      "phash": "a1b2c3d4e5",
      "timestamp": null,
      "bounds": null,
      "url": "https://...r2.cloudflarestorage.com/..."
    }
  ]
}
```

Fields per frame:
- `index` — sequential position
- `filename` — local filename
- `blur_score` — Laplacian variance sharpness score
- `phash` — perceptual hash hex string
- `timestamp` — video timestamp (video mode, if available)
- `bounds` — bounding box corners (spread mode only)
- `url` — R2 URL (when upload is enabled)

---

## 5. R2 Cloud Upload

Auto-detected from environment. When credentials are present, frames upload to Cloudflare R2 after extraction and optimization.

### 5.1 Auto-Detection
- [x] Checks for `FRAME_EXTRACTOR_R2_*` env vars on startup
- [x] Prints status message: "R2 upload enabled (bucket: X)"
- [x] `--local` flag to skip upload even when credentials are present
- **Implementation**: `src/main.rs` — startup check

### 5.2 Upload with Retry
- [x] 10 upload attempts with exponential backoff (500ms → 5s cap)
- [x] S3-compatible API via rust-s3 crate with path-style addressing
- [x] Content-type detection (image/jpeg, image/png)
- **Implementation**: `src/upload.rs` — `upload_and_verify()`

### 5.3 HEAD Verification
- [x] Two-phase verification after upload (handles R2 eventual consistency)
- [x] Fast phase: 8 retries at 100ms intervals
- [x] Backoff phase: 4 retries with exponential backoff (500ms → 2s cap)
- [x] URL only added to manifest after verification passes
- **Implementation**: `src/upload.rs` — `verify_exists()`

### 5.4 Manifest Upload
- [x] Final manifest.json uploaded to R2 alongside frames
- [x] Remote consumers can read manifest from R2 to discover frame URLs
- **Implementation**: `src/upload.rs` — `upload_manifest()`

### 5.5 Storage Layout
```
{bucket}/{mode}/{input_stem}-{timestamp}/
  page_001.jpg
  page_002.jpg
  manifest.json
```

### Environment Variables
| Variable | Required | Description |
|---|---|---|
| `FRAME_EXTRACTOR_R2_ACCOUNT_ID` | Yes | Cloudflare account ID |
| `FRAME_EXTRACTOR_R2_ACCESS_KEY_ID` | Yes | R2 API access key |
| `FRAME_EXTRACTOR_R2_SECRET_ACCESS_KEY` | Yes | R2 API secret key |
| `FRAME_EXTRACTOR_R2_BUCKET` | Yes | R2 bucket name |

---

## 6. Dependencies

| Crate | Version | Purpose |
|---|---|---|
| ffmpeg-sidecar | 2.4 | ffmpeg CLI wrapper for scene detection |
| image | 0.25.9 | Image I/O (load, save, convert) |
| imageproc | 0.26.1 | CV operations (blur, contours, transforms, Hough) |
| image_hasher | 3.1.1 | Perceptual hashing (dHash) for deduplication |
| rayon | 1.11 | Data parallelism across CPU cores |
| clap | 4.5 | CLI argument parsing with derive macros |
| serde / serde_json | 1 | JSON serialization for manifest |
| tempfile | 3 | Temporary directory for ffmpeg candidates |
| rust-s3 | 0.37 | S3-compatible API client for R2 upload |
| tokio | 1 | Async runtime for R2 operations |

**System dependency**: ffmpeg (runtime only, not a build dependency)

---

## 7. Data Structures

### Internal
| Struct | Module | Fields | Purpose |
|---|---|---|---|
| `Frame` | frame.rs | path, blur_score, timestamp | Internal frame during pipeline processing |
| `DetectedDocument` | segment.rs | corners: [Point<i32>; 4] | Rectangle detected in spread mode |
| `PipelineConfig` | pipeline_video.rs | scene_threshold, blur_threshold, dedup_threshold, output_ext, keep_all, dry_run, write_manifest, verbose | Video mode configuration |
| `PipelineResult` | pipeline_video.rs | total_candidates, after_blur, after_dedup, output_frames | Video mode result |
| `SpreadConfig` | pipeline_spread.rs | min_area_pct, max_area_pct, method, output_ext, no_perspective, write_manifest, verbose | Spread mode configuration |
| `SpreadResult` | pipeline_spread.rs | total_detected, after_dedup, output_frames | Spread mode result |
| `R2Config` | upload.rs | account_id, access_key, secret_key, bucket_name, prefix | R2 connection config (from_env() constructor) |

### Serialized (manifest.json)
| Struct | Module | Purpose |
|---|---|---|
| `Manifest` | frame.rs | Top-level manifest structure |
| `FrameManifestEntry` | frame.rs | Per-frame entry with index, filename, blur_score, phash, timestamp, bounds, url |
| `BoundingBox` | frame.rs | 4-corner coordinates for spread mode documents |

### Enums
| Enum | Module | Variants | Purpose |
|---|---|---|---|
| `DetectionMethod` | segment.rs | Threshold, Edge, Auto | Spread mode segmentation strategy |

---

## 8. Global CLI Flags

These flags apply to both `video` and `spread` subcommands:

| Flag | Default | Description |
|---|---|---|
| `--local` | false | Skip R2 upload even if credentials are present |

---

## 9. Architecture

### Module Dependency Graph
```
main.rs              CLI parsing, R2 detection, orchestration
  |
  +-- pipeline_video.rs    Video mode: scene detect -> score -> dedup -> output
  |     +-- scene.rs           Streaming ffmpeg scene extraction
  |     +-- blur.rs            Laplacian variance blur scoring
  |     +-- dedup.rs           Perceptual hash dedup
  |
  +-- pipeline_spread.rs   Spread mode: segment -> correct -> score -> output
  |     +-- segment.rs         Contour-based document detection
  |     +-- perspective.rs     Corner ordering + homography warp
  |     +-- blur.rs            (shared)
  |     +-- dedup.rs           (shared)
  |
  +-- optimize.rs          OCR optimization: deskew, contrast, sharpen, upscale
  +-- upload.rs            R2 upload with retry + verification
  +-- frame.rs             Data structs + manifest writer
```

---

## 10. Revision History

| Date | Change |
|---|---|
| 2026-03-02 | Initial implementation: video mode, spread mode, OCR optimization, R2 upload |
