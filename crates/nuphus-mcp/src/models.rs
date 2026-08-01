//! `desktop_perceive` local model management — downloads PaddleOCR models automatically on first run.
//!
//! Model files land in the same location as the main app (`NUPHUS_MODELS_DIR` or `data_dir/Nuphus/models`),
//! resolved by `vision::models` in desktop-api. Download sources mirror the main crate's build.rs:
//! - Detection/recognition ONNX: `hf-mirror.com/SWHL/RapidOCR` (HuggingFace mirror),
//!   falling back to the official `huggingface.co` source on failure.
//! - Character dictionary: `gitee.com/paddlepaddle/PaddleOCR`.
//!
//! YOLO's `icon_detect.onnx` (~80MB, must be exported from OmniParser) has no stable public URL,
//! so it is not auto-downloaded; when missing, perceive still runs in OCR mode and honestly reports YOLO as unavailable.
//! Users can provide a custom download URL via `NUPHUS_MCP_YOLO_MODEL_URL`.

use std::path::PathBuf;
use std::time::Duration;

use desktop_api::vision::models::{
    models_dir_for_write, validate_ocr_models, yolo_model_available, PADDLE_OCR_FILES,
};

/// Model readiness status (used by desktop_perceive).
#[derive(Debug, Clone)]
pub struct ModelStatus {
    pub dir: PathBuf,
    /// Whether the PaddleOCR trio is complete (hard prerequisite for perceive)
    pub ocr_ready: bool,
    /// Whether the YOLO icon detection model is available (optional enhancement)
    pub yolo_available: bool,
    /// Files actually downloaded by this call
    pub downloaded: Vec<String>,
}

/// Candidate download sources per file (tried in order).
fn sources_for(file: &str) -> Vec<&'static str> {
    match file {
        "ch_PP-OCRv4_det.onnx" => vec![
            "https://hf-mirror.com/SWHL/RapidOCR/resolve/main/PP-OCRv4/ch_PP-OCRv4_det_infer.onnx",
            "https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv4/ch_PP-OCRv4_det_infer.onnx",
        ],
        "ch_PP-OCRv4_rec.onnx" => vec![
            "https://hf-mirror.com/SWHL/RapidOCR/resolve/main/PP-OCRv4/ch_PP-OCRv4_rec_infer.onnx",
            "https://huggingface.co/SWHL/RapidOCR/resolve/main/PP-OCRv4/ch_PP-OCRv4_rec_infer.onnx",
        ],
        "ch_PP-OCR_keys_v1.txt" => {
            vec!["https://gitee.com/paddlepaddle/PaddleOCR/raw/main/ppocr/utils/ppocr_keys_v1.txt"]
        }
        _ => vec![],
    }
}

/// Ensure local models are ready: auto-download any missing PaddleOCR files.
///
/// - OCR files all present or downloaded successfully → `Ok(status)` (`yolo_available` reported honestly).
/// - Any OCR file download fails → `Err` (clear error + manual download instructions), no panic.
/// - Setting `NUPHUS_MCP_NO_MODEL_DOWNLOAD=1` skips auto-download (fast-fail for restricted networks/CI:
///   existence-only check that returns a clear error).
pub async fn ensure_models() -> Result<ModelStatus, String> {
    let dir = models_dir_for_write()?;
    let mut downloaded: Vec<String> = Vec::new();

    let skip_download = std::env::var("NUPHUS_MCP_NO_MODEL_DOWNLOAD")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // Validate first, to avoid unnecessary requests when everything is already present
    if !skip_download && validate_ocr_models(&dir).is_err() {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("build http client failed: {}", e))?;

        for file in PADDLE_OCR_FILES {
            let dest = dir.join(file);
            if dest.exists() {
                continue;
            }
            let sources = sources_for(file);
            if sources.is_empty() {
                continue;
            }
            match download_with_fallback(&client, &sources, &dest).await {
                Ok(()) => {
                    downloaded.push(file.to_string());
                }
                Err(e) => {
                    return Err(format!(
                        "desktop_perceive model download failed for {file}: {e}\n\
                         Please download the following files manually into {}:\n  - ch_PP-OCRv4_det.onnx ← https://hf-mirror.com/SWHL/RapidOCR\n  - ch_PP-OCRv4_rec.onnx ← https://hf-mirror.com/SWHL/RapidOCR\n  - ch_PP-OCR_keys_v1.txt ← https://gitee.com/paddlepaddle/PaddleOCR",
                        dir.display()
                    ));
                }
            }
        }
    }

    // Final validation: OCR must be complete, otherwise report an error (with missing files)
    validate_ocr_models(&dir).map_err(|e| {
        format!(
            "{e}\nAutomatic download did not complete. Please check your network and retry, or place the model files manually in {}",
            dir.display()
        )
    })?;

    Ok(ModelStatus {
        dir: dir.clone(),
        ocr_ready: true,
        yolo_available: yolo_model_available(&dir),
        downloaded,
    })
}

/// Try each candidate source in order; return Err if all fail.
async fn download_with_fallback(
    client: &reqwest::Client,
    sources: &[&'static str],
    dest: &std::path::Path,
) -> Result<(), String> {
    let mut last_err: Option<String> = None;
    for url in sources {
        match download_file(client, url, dest).await {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| "no source".to_string()))
}

/// Minimum accepted file size per model (bytes).
///
/// Guards against empty/truncated downloads that succeed with HTTP 200 but carry an
/// error page or a partial body. RapidOCR PP-OCRv4 ONNX models are ~4-6 MB and the
/// character dictionary ~60 KB, so these floors can never reject a real artifact.
fn min_expected_bytes(file: &str) -> u64 {
    match file {
        "ch_PP-OCRv4_det.onnx" | "ch_PP-OCRv4_rec.onnx" => 100_000,
        "ch_PP-OCR_keys_v1.txt" => 1_000,
        _ => 0,
    }
}

/// Optional per-file SHA-256 enforcement.
///
/// If `NUPHUS_MCP_MODEL_SHA256_<FILE>` is set, downloaded bytes must match the given
/// digest or the download is rejected. This lets operators pin downloads to a known-good
/// artifact (e.g. a trusted mirror) against supply-chain tampering. When unset, only the
/// size floor above applies.
fn expected_sha256(file: &str) -> Option<String> {
    let key = format!("NUPHUS_MCP_MODEL_SHA256_{file}");
    std::env::var(&key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Stream-download a single file to a temp path, verify integrity, then rename atomically
/// (avoid a partial or tampered file being treated as a valid model).
async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GET {url} failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GET {url} -> HTTP {}", resp.status()));
    }

    let tmp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("create temp file {} failed: {e}", tmp.display()))?;

    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("read response body failed: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("write temp file failed: {e}"))?;
    }
    file.flush()
        .await
        .map_err(|e| format!("flush failed: {e}"))?;
    drop(file);

    // Integrity check before the temp file can become a "valid" model.
    let file_name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let meta = tokio::fs::metadata(&tmp)
        .await
        .map_err(|e| format!("stat temp file {} failed: {e}", tmp.display()))?;
    let floor = min_expected_bytes(file_name);
    if meta.len() < floor {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!(
            "download too small ({} bytes < {floor} floor), refusing to use as model: {file_name}",
            meta.len()
        ));
    }
    if let Some(expected) = expected_sha256(file_name) {
        let bytes = tokio::fs::read(&tmp)
            .await
            .map_err(|e| format!("read temp file for hashing failed: {e}"))?;
        use sha2::{Digest, Sha256};
        let actual = hex::encode(Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(&expected) {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(format!(
                "model SHA-256 mismatch for {file_name}: got {actual}, expected {expected}. \
                 Fix NUPHUS_MCP_MODEL_SHA256_{file_name} or unset it to skip hashing"
            ));
        }
    }

    std::fs::rename(&tmp, dest).map_err(|e| format!("rename to {} failed: {e}", dest.display()))?;
    tracing::info!("[models] download complete: {} <- {}", dest.display(), url);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paddle_ocr_file_constants_cover_all_three() {
        assert_eq!(PADDLE_OCR_FILES.len(), 3);
        for f in PADDLE_OCR_FILES {
            assert!(!sources_for(f).is_empty(), "{} must have sources", f);
        }
    }

    #[test]
    fn unknown_file_has_no_source() {
        assert!(sources_for("nope.onnx").is_empty());
        assert_eq!(desktop_api::vision::models::YOLO_MODEL, "icon_detect.onnx");
    }

    #[test]
    fn size_floor_covers_known_models() {
        for f in PADDLE_OCR_FILES {
            assert!(min_expected_bytes(f) > 0, "{f} must have a size floor");
        }
        assert_eq!(min_expected_bytes("nope.onnx"), 0);
    }

    #[test]
    fn sha256_env_override_respected() {
        let _lock = crate::test_support::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        const KEY: &str = "NUPHUS_MCP_MODEL_SHA256_ch_PP-OCRv4_det.onnx";
        std::env::set_var(KEY, "  abc  ");
        assert_eq!(
            expected_sha256("ch_PP-OCRv4_det.onnx"),
            Some("abc".to_string())
        );
        std::env::remove_var(KEY);
        assert_eq!(expected_sha256("ch_PP-OCRv4_det.onnx"), None);
    }
}
