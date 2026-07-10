use anyhow::Result;
use image::ImageEncoder;
use image::ImageReader;
use image::EncodableLayout;
use std::path::{Path, PathBuf};
use tokio::task;

pub async fn extract_and_save(raw: &[u8], out_path: &Path) -> Result<PathBuf> {
    let out = out_path.to_owned();
    let raw = raw.to_vec();
    task::spawn_blocking(move || -> Result<PathBuf> {
        let img = ImageReader::new(std::io::Cursor::new(&raw))
            .with_guessed_format()?
            .decode()?;
        let (w, h) = (img.width(), img.height());
        let max = w.max(h);
        let scaled = if max > 800 {
            let ratio = 800.0 / max as f32;
            let nw = ((w as f32) * ratio) as u32;
            let nh = ((h as f32) * ratio) as u32;
            img.resize(nw, nh, image::imageops::FilterType::Lanczos3).to_rgb8()
        } else {
            img.to_rgb8()
        };
        let mut quality = 75u8;
        let bytes = loop {
            let mut buf = Vec::new();
            let mut cur = std::io::Cursor::new(&mut buf);
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cur, quality);
            encoder.write_image(
                scaled.as_bytes(),
                scaled.width(),
                scaled.height(),
                image::ExtendedColorType::Rgb8,
            )?;
            if buf.len() <= 100 * 1024 || quality <= 40 {
                break buf;
            }
            quality = (quality as i32 - 15).max(40) as u8;
        };
        std::fs::write(&out, &bytes)?;
        Ok(out)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use tempfile::TempDir;

    /// Build an in-memory 1024x768 RGB gradient as raw bytes for the
    /// compressor to chew on. We don't need real image features - the
    /// point of this test is the size budget and JPEG magic, not PSNR.
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
    async fn compresses_to_jpeg_within_size_budget() {
        let dir = TempDir::new().unwrap();
        let out_path = dir.path().join("cover.jpg");
        let raw = make_test_png_bytes();
        let written = extract_and_save(&raw, &out_path).await.unwrap();
        assert_eq!(written, out_path);
        let bytes = std::fs::read(&out_path).unwrap();
        // JPEG SOI marker.
        assert_eq!(&bytes[0..3], &[0xFF, 0xD8, 0xFF]);
        // Spec §"封面提取规则": <= 100 KB.
        assert!(
            bytes.len() <= 100 * 1024,
            "cover too large: {} bytes",
            bytes.len()
        );
    }
}
