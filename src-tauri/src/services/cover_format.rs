//! WebP 编码：把任意 image::DynamicImage 编码为 webp。
//!
//! image crate 0.25 内置 webp encoder **只支持 lossless**（无 quality 参数）。
//! 调用方负责先把图缩放到合理尺寸；这里仅做编码 + 写文件。

use anyhow::Result;
use image::ImageEncoder;
use std::path::Path;

pub fn encode_webp(img: image::DynamicImage, dest: &Path) -> Result<()> {
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());
    let mut buf = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut buf)
        .write_image(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8)
        .map_err(|e| anyhow::anyhow!("webp encode error: {}", e))?;
    std::fs::write(dest, buf)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_webp_produces_riff_magic() {
        let mut img = image::DynamicImage::new_rgb8(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                img.as_mut_rgb8().unwrap()
                    .put_pixel(x, y, image::Rgb([(x * 30) as u8, (y * 30) as u8, 128]));
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("t.webp");
        encode_webp(img, &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        assert!(
            bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP",
            "expected RIFF/WEBP magic, got first 12 bytes: {:?}",
            &bytes[..bytes.len().min(12)]
        );
    }
}