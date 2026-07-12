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

/// 解码 + 长边 ≤1600px（不放大）+ webp lossy q=70，返回 bytes。
/// 详情页大图预览用：体积远小于原图，又能保留观感。
pub fn transcode_to_preview_webp(raw: &[u8]) -> Result<Vec<u8>> {
    let img = image::ImageReader::new(std::io::Cursor::new(raw))
        .with_guessed_format()?
        .decode()?;
    // image::resize 不阻止放大；只在长边超过 1600 时才缩。
    let needs_resize = img.width() > 1600 || img.height() > 1600;
    let resized = if needs_resize {
        img.resize(1600, 1600, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let memory = webp::Encoder::from_image(&resized)
        .map_err(|e| anyhow::anyhow!("webp encoder init error: {}", e))?
        .encode(70.0);
    Ok(memory.to_vec())
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

    #[test]
    fn transcode_to_preview_webp_shrinks_large_png() {
        // 2000×1500 noise png ≈ 9MB raw; 详情页场景下转码后应远小于输入。
        let mut img = image::DynamicImage::new_rgb8(2000, 1500);
        for y in 0..1500 {
            for x in 0..2000 {
                img.as_mut_rgb8().unwrap().put_pixel(
                    x,
                    y,
                    image::Rgb([((x ^ y) as u8), ((x.wrapping_add(y)) as u8), ((x.wrapping_mul(y)) as u8)]),
                );
            }
        }
        let raw = {
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
            buf.into_inner()
        };
        assert!(raw.len() > 100_000, "sanity: noise png should be sizable");

        let webp = transcode_to_preview_webp(&raw).unwrap();
        assert!(webp.starts_with(b"RIFF") && webp[8..12] == *b"WEBP");
        assert!(webp.len() < raw.len() / 5, "preview webp should shrink noise png sharply");
    }

    #[test]
    fn transcode_to_preview_webp_does_not_upscale_small_input() {
        // 300×300 输入：函数应保持 300×300，不放大到 1600。
        let mut img = image::DynamicImage::new_rgb8(300, 300);
        for y in 0..300 {
            for x in 0..300 {
                img.as_mut_rgb8().unwrap().put_pixel(x, y, image::Rgb([128, 64, 32]));
            }
        }
        let raw = {
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
            buf.into_inner()
        };
        let webp_bytes = transcode_to_preview_webp(&raw).unwrap();
        let decoded = image::ImageReader::new(std::io::Cursor::new(&webp_bytes))
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!((decoded.width(), decoded.height()), (300, 300),
                   "transcode_to_preview_webp 不应放大 300×300 输入");
    }
}