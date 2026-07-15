use anyhow::Result;
use image::ImageReader;
use std::path::{Path, PathBuf};
use tokio::task;

pub async fn extract_and_save(raw: &[u8], out_path: &Path) -> Result<PathBuf> {
    let out = out_path.to_owned();
    let raw = raw.to_vec();
    task::spawn_blocking(move || -> Result<PathBuf> {
        let img = ImageReader::new(std::io::Cursor::new(&raw))
            .with_guessed_format()?
            .decode()?;

        // V3: webp lossless 编码；如果超 100KB 再缩一档重试。
        // 默认 600（V2 jpg 用 800，但 lossless webp 难压 gradient 图）。
        let scaled = resize_to_max(img, 600);
        crate::services::cover_format::encode_webp(scaled.clone(), &out)?;

        // 预算控制：超 100KB 则基于第一次的 600px scaled 缩到 400 重写——
        // 复用 `scaled` 而不是重新 decode 原图，省掉 1-2s jpeg decode。
        // 600 → 400 已经很小，resize 几乎瞬时。
        let budget = std::fs::metadata(&out)?.len();
        if budget > 100 * 1024 {
            let smaller = resize_to_max(scaled, 400);
            crate::services::cover_format::encode_webp(smaller, &out)?;
        }
        Ok(out)
    })
    .await?
}

fn resize_to_max(img: image::DynamicImage, max: u32) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    let m = w.max(h);
    if m <= max {
        img
    } else {
        let ratio = max as f32 / m as f32;
        let nw = ((w as f32) * ratio) as u32;
        let nh = ((h as f32) * ratio) as u32;
        // Triangle（双线性）足够用于 600px 缩略图；Lanczos3 是单核
        // 纯 Rust 无 SIMD，5000x7000 → 600 需要 ~2.7s，Triangle < 300ms。
        img.resize(nw, nh, image::imageops::FilterType::Triangle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use tempfile::TempDir;

    /// Build an in-memory 1024x768 RGB gradient as raw bytes for the
    /// compressor to chew on. We don't need real image features - the
    /// point of this test is the size budget and WebP magic, not PSNR.
    fn make_test_png_bytes() -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(1024, 768, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        });
        let mut buf: Vec<u8> = Vec::new();
        let dyn_img = image::DynamicImage::ImageRgb8(img);
        dyn_img
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[tokio::test]
    async fn compresses_to_webp_within_size_budget() {
        let dir = TempDir::new().unwrap();
        let out_path = dir.path().join("cover.pwb");
        let raw = make_test_png_bytes();
        let written = extract_and_save(&raw, &out_path).await.unwrap();
        assert_eq!(written, out_path);
        let bytes = std::fs::read(&out_path).unwrap();
        // WebP RIFF magic.
        assert!(bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP");
        // Spec §"封面提取规则": <= 100 KB.
        assert!(
            bytes.len() <= 100 * 1024,
            "cover too large: {} bytes",
            bytes.len()
        );
    }
}