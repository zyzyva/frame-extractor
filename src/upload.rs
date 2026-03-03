use std::path::Path;
use std::time::Duration;

use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::Region;

pub struct R2Config {
    pub account_id: String,
    pub access_key: String,
    pub secret_key: String,
    pub bucket_name: String,
    pub prefix: String,
}

impl R2Config {
    pub fn from_env(prefix: &str) -> Result<Self, String> {
        let account_id = std::env::var("R2_ACCOUNT_ID")
            .map_err(|_| "R2_ACCOUNT_ID not set")?;
        let access_key = std::env::var("R2_ACCESS_KEY_ID")
            .map_err(|_| "R2_ACCESS_KEY_ID not set")?;
        let secret_key = std::env::var("R2_SECRET_ACCESS_KEY")
            .map_err(|_| "R2_SECRET_ACCESS_KEY not set")?;
        let bucket_name = std::env::var("R2_BUCKET")
            .map_err(|_| "R2_BUCKET not set")?;

        Ok(R2Config {
            account_id,
            access_key,
            secret_key,
            bucket_name,
            prefix: prefix.to_string(),
        })
    }

    fn create_bucket(&self) -> Result<Box<Bucket>, String> {
        let region = Region::Custom {
            region: "auto".to_string(),
            endpoint: format!("https://{}.r2.cloudflarestorage.com", self.account_id),
        };

        let credentials = Credentials::new(
            Some(&self.access_key),
            Some(&self.secret_key),
            None,
            None,
            None,
        )
        .map_err(|e| format!("Failed to create credentials: {}", e))?;

        let bucket = Bucket::new(&self.bucket_name, region, credentials)
            .map_err(|e| format!("Failed to create bucket: {}", e))?
            .with_path_style();

        Ok(bucket)
    }
}

/// Upload a file to R2 and verify it exists before returning the URL.
/// Uses exponential backoff for retries and HEAD verification.
pub async fn upload_and_verify(
    config: &R2Config,
    local_path: &Path,
    filename: &str,
    verbose: bool,
) -> Result<String, String> {
    let bucket = config.create_bucket()?;

    let content = std::fs::read(local_path)
        .map_err(|e| format!("Failed to read {}: {}", local_path.display(), e))?;

    let content_type = match local_path.extension().and_then(|e| e.to_str()) {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    };

    let key = format!("{}/{}", config.prefix, filename);

    // Upload with retries (up to 10 attempts, exponential backoff)
    #[allow(unused_assignments)]
    let mut last_err = String::new();
    for attempt in 0..10 {
        match bucket
            .put_object_with_content_type(&key, &content, content_type)
            .await
        {
            Ok(response) if response.status_code() >= 200 && response.status_code() < 300 => {
                if verbose {
                    eprintln!("  Uploaded {} ({} bytes)", key, content.len());
                }
                break;
            }
            Ok(response) => {
                last_err = format!("R2 returned status {}", response.status_code());
            }
            Err(e) => {
                last_err = format!("{}", e);
            }
        }

        if attempt < 9 {
            let delay = Duration::from_millis(std::cmp::min(5000, 500 * 2u64.pow(attempt)));
            if verbose {
                eprintln!(
                    "  Upload attempt {} failed ({}), retrying in {:?}",
                    attempt + 1,
                    last_err,
                    delay
                );
            }
            tokio::time::sleep(delay).await;
        } else {
            return Err(format!("Upload failed after 10 attempts: {}", last_err));
        }
    }

    // Verify with HEAD — fast phase (8 retries, 100ms) then backoff (4 retries)
    verify_exists(&bucket, &key, verbose).await?;

    // Return the R2 URL (S3 endpoint + key)
    let url = format!(
        "https://{}.r2.cloudflarestorage.com/{}/{}",
        config.account_id, config.bucket_name, key
    );

    Ok(url)
}

/// Verify an object exists in R2 using HEAD requests.
/// Two-phase: fast polling (8 x 100ms), then exponential backoff (4 retries).
async fn verify_exists(bucket: &Bucket, key: &str, verbose: bool) -> Result<(), String> {
    // Fast phase: 8 retries at 100ms
    for attempt in 0..8 {
        match bucket.head_object(key).await {
            Ok((_, status)) if status >= 200 && status < 300 => return Ok(()),
            Ok((_, 404)) => {
                if attempt == 0 && verbose {
                    eprintln!("  Verifying R2 availability...");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            _ => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }

    // Backoff phase: 4 retries with exponential backoff
    for attempt in 0..4 {
        let delay = Duration::from_millis(std::cmp::min(2000, 500 * 2u64.pow(attempt)));
        tokio::time::sleep(delay).await;

        match bucket.head_object(key).await {
            Ok((_, status)) if status >= 200 && status < 300 => return Ok(()),
            Ok((_, status)) => {
                if verbose {
                    eprintln!("  Verify attempt {} got status {}", 9 + attempt, status);
                }
            }
            Err(e) => {
                if verbose {
                    eprintln!("  Verify attempt {} error: {}", 9 + attempt, e);
                }
            }
        }
    }

    Err(format!("File not available in R2 after verification: {}", key))
}

/// Upload a manifest.json to R2 (no verification needed — overwritten frequently)
pub async fn upload_manifest(
    config: &R2Config,
    manifest_json: &str,
) -> Result<(), String> {
    let bucket = config.create_bucket()?;
    let key = format!("{}/manifest.json", config.prefix);

    bucket
        .put_object_with_content_type(&key, manifest_json.as_bytes(), "application/json")
        .await
        .map_err(|e| format!("Failed to upload manifest: {}", e))?;

    Ok(())
}
