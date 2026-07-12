use anyhow::{anyhow, Result};
use std::path::Path;

use crate::services::rar_detect::{RarLocation, RarTool};

const IMG_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];

#[derive(Debug, Clone)]
pub struct ArchiveImageEntry {
    pub name: String,
    pub data: Vec<u8>,
}

pub fn list_images(path: &Path) -> Result<Vec<ArchiveImageEntry>> {
    if path.extension().and_then(|e| e.to_str()) != Some("zip") {
        return Err(anyhow!("unsupported archive format (zip only for V1)"));
    }
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_string();
        let lower = name.to_lowercase();
        if IMG_EXTS.iter().any(|e| lower.ends_with(&format!(".{}", e))) {
            let mut data = Vec::with_capacity(entry.size() as usize);
            std::io::copy(&mut entry, &mut data)?;
            out.push(ArchiveImageEntry { name, data });
        }
    }
    Ok(out)
}

pub fn pick_cover(candidates: &[ArchiveImageEntry]) -> Option<&ArchiveImageEntry> {
    // 1) name contains cover keyword
    if let Some(c) = candidates.iter().find(|e| {
        let n = e.name.to_lowercase();
        n.contains("cover") || n.contains("表紙")
    }) {
        return Some(c);
    }
    // 2) first in zip order
    candidates.first()
}

/// Extract a single image from a zip by its position in the
/// image-filtered list (matches the order returned by
/// `list_image_names`). Returns `(name, bytes)` so callers can
/// derive a MIME type from the extension without re-parsing.
///
/// Skips directories and non-image entries internally so the index
/// lines up with the public listing.
pub fn read_image_at(path: &Path, index: usize) -> Result<(String, Vec<u8>)> {
    if path.extension().and_then(|e| e.to_str()) != Some("zip") {
        return Err(anyhow!("unsupported archive format (zip only for V1)"));
    }
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    let mut seen = 0usize;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_string();
        let lower = name.to_lowercase();
        if !IMG_EXTS.iter().any(|e| lower.ends_with(&format!(".{}", e))) {
            continue;
        }
        if seen == index {
            let mut data = Vec::with_capacity(entry.size() as usize);
            std::io::copy(&mut entry, &mut data)?;
            return Ok((name, data));
        }
        seen += 1;
    }
    Err(anyhow!("image index {} out of range", index))
}

/// Like `list_images` but only returns entry names — used by the
/// conflict compare endpoint which never needs the file bytes.
///
/// Intentionally zip-only for V1; the RAR compare path waits for
/// sub-plan #7 (full RAR extraction).
pub fn list_image_names(path: &Path) -> Result<Vec<String>> {
    if path.extension().and_then(|e| e.to_str()) != Some("zip") {
        return Err(anyhow!("unsupported archive format (zip only for V1)"));
    }
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    let mut names = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_string();
        let lower = name.to_lowercase();
        if IMG_EXTS.iter().any(|e| lower.ends_with(&format!(".{}", e))) {
            names.push(name);
        }
    }
    Ok(names)
}

// =================================================================
// RAR 子进程封装（task #7-3）
// =================================================================
//
// 我们不解析 RAR 格式本身——而是调用 unrar 或 7z 子进程。两种工具的
// 参数差异比较大（unrar 用单字符开关，7z 用 `-开关` 风格），所以每个
// 函数都按 tool 分两路。list_rar_images 不解压（节省 IO），但只拿
// 到文件名——data 字段留空，调用方按需 extract。

#[derive(Debug, Clone, Default)]
pub struct ExtractStats {
    pub extracted_count: usize,
    pub total_bytes: u64,
}

/// List image entries in a RAR archive by name only (no extraction).
/// Requires the caller to have already located a `RarLocation` via
/// `rar_detect::detect()` — we never shell out to an unknown binary.
pub async fn list_rar_images(path: &Path, tool: &RarLocation) -> Result<Vec<ArchiveImageEntry>> {
    let output = match tool.tool {
        RarTool::Unrar => {
            tokio::process::Command::new(&tool.path)
                .args(["l", "-p-", path.to_str().unwrap()])
                .output()
                .await?
        }
        RarTool::SevenZip => {
            tokio::process::Command::new(&tool.path)
                .args(["l", "-slt", path.to_str().unwrap()])
                .output()
                .await?
        }
    };
    if !output.status.success() {
        return Err(anyhow!(
            "rar listing failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_rar_list(&stdout)
        .into_iter()
        .map(|name| ArchiveImageEntry { name, data: vec![] })
        .collect())
}

pub(crate) fn parse_rar_list(stdout: &str) -> Vec<String> {
    // unrar l 输出格式（简化）：
    //   Name             Size   Packed Ratio  Date   Time   Attr  CRC  Meth Ver
    //   images/01.jpg  123456  123456 100%  ...
    // 7z l -slt 输出更复杂，每条记录有 "Path = ..." 单独一行。
    // 简化策略：扫所有含 IMG_EXTS 的行，取第一列（unrar）或 `Path = ` 后（7z）。
    let mut out = Vec::new();
    for line in stdout.lines() {
        let lower = line.to_lowercase();
        for ext in IMG_EXTS {
            if lower.contains(&format!(".{}", ext)) && !lower.starts_with("----") {
                let name = if let Some(rest) = line.strip_prefix("Path = ") {
                    rest.trim().to_string()
                } else if let Some(rest) = line.strip_prefix("path = ") {
                    rest.trim().to_string()
                } else {
                    line.split_whitespace().next().unwrap_or("").to_string()
                };
                if !name.is_empty() {
                    out.push(name);
                }
                break;
            }
        }
    }
    out
}

/// Extract a RAR archive to `dest_dir`. Returns the count and total
/// size of extracted files so the caller can check disk space.
pub async fn extract_rar(
    path: &Path,
    dest_dir: &Path,
    tool: &RarLocation,
) -> Result<ExtractStats> {
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("rar path is not valid UTF-8: {}", path.display()))?;
    let dest_str = dest_dir
        .to_str()
        .ok_or_else(|| anyhow!("dest dir is not valid UTF-8: {}", dest_dir.display()))?;

    let output = match tool.tool {
        RarTool::Unrar => {
            tokio::process::Command::new(&tool.path)
                .args(["x", "-y", "-o+", path_str, dest_str])
                .output()
                .await?
        }
        RarTool::SevenZip => {
            // 7z 用 `-o{dir}` 表示输出目录（无空格）。
            let mut output_arg = String::from("-o");
            output_arg.push_str(dest_str);
            tokio::process::Command::new(&tool.path)
                .args(["x", "-y", "-aoa", path_str, &output_arg])
                .output()
                .await?
        }
    };
    if !output.status.success() {
        return Err(anyhow!(
            "rar extract failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut count = 0usize;
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(dest_dir) {
        let e = entry?;
        if e.file_type().is_file() {
            count += 1;
            total += e.metadata().ok().map(|m| m.len()).unwrap_or(0);
        }
    }
    Ok(ExtractStats {
        extracted_count: count,
        total_bytes: total,
    })
}

/// Walk a directory and return image entries found inside it (used
/// after `extract_rar` so we can pick a cover without re-extracting).
pub fn list_images_in_dir(dir: &Path) -> Result<Vec<ArchiveImageEntry>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(dir) {
        let e = entry?;
        if !e.file_type().is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        if IMG_EXTS.iter().any(|ext| lower.ends_with(&format!(".{}", ext))) {
            let data = std::fs::read(e.path())?;
            out.push(ArchiveImageEntry { name, data });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a minimal zip in-memory containing the given (name, bytes)
    /// pairs. Used by both `list_images` and `list_image_names` tests.
    fn build_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::SimpleFileOptions =
                zip::write::SimpleFileOptions::default();
            for (name, data) in entries {
                zw.start_file(*name, opts).unwrap();
                zw.write_all(data).unwrap();
            }
            zw.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn list_image_names_skips_directories_and_non_images() {
        let zip_bytes = build_test_zip(&[
            ("images/01.jpg", b"fake-jpg-data"),
            ("images/02.png", b"fake-png-data"),
            ("readme.txt", b"hello"),
            ("subdir/", b""),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.zip");
        std::fs::write(&p, zip_bytes).unwrap();
        let names = list_image_names(&p).unwrap();
        assert_eq!(names, vec!["images/01.jpg", "images/02.png"]);
    }

    #[test]
    fn list_image_names_rejects_rar() {
        let p = std::path::Path::new("foo.rar");
        assert!(list_image_names(p).is_err());
    }

    #[test]
    fn read_image_at_returns_image_by_filtered_index() {
        let zip_bytes = build_test_zip(&[
            ("images/01.jpg", b"jpg-bytes"),
            ("readme.txt", b"ignored"),
            ("images/02.png", b"png-bytes"),
            ("images/03.webp", b"webp-bytes"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.zip");
        std::fs::write(&p, zip_bytes).unwrap();

        let (n0, d0) = read_image_at(&p, 0).unwrap();
        assert_eq!(n0, "images/01.jpg");
        assert_eq!(d0, b"jpg-bytes");
        let (n2, d2) = read_image_at(&p, 2).unwrap();
        assert_eq!(n2, "images/03.webp");
        assert_eq!(d2, b"webp-bytes");
    }

    #[test]
    fn read_image_at_out_of_range_errors() {
        let zip_bytes = build_test_zip(&[("only.jpg", b"x")]);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.zip");
        std::fs::write(&p, zip_bytes).unwrap();
        assert!(read_image_at(&p, 1).is_err());
    }
}

