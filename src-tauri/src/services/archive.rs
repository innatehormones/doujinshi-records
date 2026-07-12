use anyhow::{anyhow, Result};
use std::cmp::Ordering;
use std::path::Path;

use crate::services::rar_detect::{RarLocation, RarTool};

const IMG_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];

#[derive(Debug, Clone)]
pub struct ArchiveImageEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// "Natural" sort key for archive entry names: split into alternating
/// digit / non-digit runs; digit runs compare as `u128` (so `2` < `10`),
/// non-digit runs compare lexicographically.  Mirrors the human intuition
/// behind tools like 7-Zip's "natural sort" so 7z/WinRAR-packed zips
/// whose Central Directory order does not match the visible filename
/// order (e.g. `imgi_2_..._01.jpg` stored before `imgi_10_..._10.jpg`)
/// display in page order, not writer order.
fn natural_sort_key(s: &str) -> Vec<NaturalChunk> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut n: u128 = 0;
            // Cap at 39 digits to keep `n` well inside u128; we only
            // care about ordinal comparison, not the actual number.
            let mut taken = 0;
            while i < bytes.len() && bytes[i].is_ascii_digit() && taken < 39 {
                n = n.saturating_mul(10).saturating_add((bytes[i] - b'0') as u128);
                i += 1;
                taken += 1;
            }
            out.push(NaturalChunk::Digits(n));
            let _ = start;
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_digit() {
                i += 1;
            }
            out.push(NaturalChunk::Text(s[start..i].to_string()));
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NaturalChunk {
    Digits(u128),
    Text(String),
}

impl PartialOrd for NaturalChunk {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NaturalChunk {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (NaturalChunk::Digits(a), NaturalChunk::Digits(b)) => a.cmp(b),
            (NaturalChunk::Text(a), NaturalChunk::Text(b)) => a.cmp(b),
            // Tie-break: digit chunks come before text chunks when one
            // side is exhausted early (e.g. "page2" vs "page10" — at
            // the position where '2' is on one side and '1' on the
            // other, both are digits so we never hit this; but a text
            // chunk vs a digit chunk is unreachable because we always
            // emit digit→text→digit→text in lockstep from the same
            // index).  Left here as a defensive fallback.
            (NaturalChunk::Digits(_), NaturalChunk::Text(_)) => Ordering::Less,
            (NaturalChunk::Text(_), NaturalChunk::Digits(_)) => Ordering::Greater,
        }
    }
}

/// Sort image names by natural key. `unstable` is fine — names are
/// unique within an archive, so equal keys can keep their original
/// relative order without observable difference.
fn sort_names_natural(names: &mut [String]) {
    names.sort_unstable_by(|a, b| natural_sort_key(a).cmp(&natural_sort_key(b)));
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
    // Cover selection uses `candidates.first()` as a fallback, and
    // the public listing is sorted, so sort here too for stability
    // between the two callers.
    out.sort_unstable_by(|a, b| natural_sort_key(&a.name).cmp(&natural_sort_key(&b.name)));
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
    // 2) first in listing order (post-sort)
    candidates.first()
}

/// Extract a single image from a zip by its position in the
/// image-filtered list (matches the order returned by
/// `list_image_names`). Returns `(name, bytes)` so callers can
/// derive a MIME type from the extension without re-parsing.
///
/// Skips directories and non-image entries internally so the index
/// lines up with the public listing.  The list is sorted with a
/// natural-order key so the index is stable across archives whose
/// Central Directory order does not match the visible filename
/// order — see `natural_sort_key`.
pub fn read_image_at(path: &Path, index: usize) -> Result<(String, Vec<u8>)> {
    if path.extension().and_then(|e| e.to_str()) != Some("zip") {
        return Err(anyhow!("unsupported archive format (zip only for V1)"));
    }
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f)?;
    // 1) collect all image names in the same order callers will see
    //    them (Central Directory order, skipping directories and
    //    non-image entries).
    let mut names: Vec<String> = Vec::new();
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
    // 2) sort with the same natural key so `index` matches the
    //    detail page's grid order.
    sort_names_natural(&mut names);
    let target = names
        .into_iter()
        .nth(index)
        .ok_or_else(|| anyhow!("image index {} out of range", index))?;

    // 3) reopen and read the matching entry by name.  Opening twice
    //    is cheaper than the alternative of keeping the archive open
    //    while we look up the entry by re-scanning Central Directory.
    let f2 = std::fs::File::open(path)?;
    let mut zip2 = zip::ZipArchive::new(f2)?;
    let mut entry = zip2.by_name(&target)?;
    let mut data = Vec::with_capacity(entry.size() as usize);
    std::io::copy(&mut entry, &mut data)?;
    Ok((target, data))
}

/// Like `list_images` but only returns entry names — used by the
/// conflict compare endpoint which never needs the file bytes.
/// Names are sorted with the natural-order key so the returned
/// order matches the public listing (`list_image_names_sorted`).
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
    sort_names_natural(&mut names);
    Ok(names)
}

/// Public listing for the detail page: same filtering as
/// `list_image_names` but guaranteed to be sorted by natural key
/// before returning, and the public index in the detail UI
/// (`ImageEntry.url` and `read_image_at(path, idx)`) lines up with
/// this order.  `list_image_names` is kept for back-compat with
/// callers that don't need ordering (e.g. the conflict compare path
/// which just lists names without indexes).
pub fn list_image_names_sorted(path: &Path) -> Result<Vec<String>> {
    list_image_names(path)
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

    /// 7-Zip / WinRAR 打包的 zip 经常把 entry 写入顺序与"页号"顺序
    /// 不一致（爬虫 / 多线程下载时按 NN 自然序写入文件，但页号是另
    /// 一段数字）。`list_image_names` 必须按自然序重排，详情页
    /// 才能按页号顺序展示。
    #[test]
    fn list_image_names_sorts_by_natural_key_when_writer_order_differs() {
        // 模拟 unzip -l 看到的实际存储顺序：imgi_10 在最前，
        // imgi_2/imgi_3 插在中间和末尾。
        let zip_bytes = build_test_zip(&[
            (
                "第01话/imgi_10_g%2F公主的秘密与秘密的私生子%2F第01话%2F10.jpg",
                b"img-10",
            ),
            (
                "第01话/imgi_11_g%2F公主的秘密与秘密的私生子%2F第01话%2F11.jpg",
                b"img-11",
            ),
            (
                "第01话/imgi_2_g%2F公主的秘密与秘密的私生子%2F第01话%2F01.jpg",
                b"img-1",
            ),
            (
                "第01话/imgi_3_g%2F公主的秘密与秘密的私生子%2F第01话%2F02.jpg",
                b"img-2",
            ),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("out_of_order.zip");
        std::fs::write(&p, zip_bytes).unwrap();

        let names = list_image_names(&p).unwrap();
        assert_eq!(
            names,
            vec![
                "第01话/imgi_2_g%2F公主的秘密与秘密的私生子%2F第01话%2F01.jpg",
                "第01话/imgi_3_g%2F公主的秘密与秘密的私生子%2F第01话%2F02.jpg",
                "第01话/imgi_10_g%2F公主的秘密与秘密的私生子%2F第01话%2F10.jpg",
                "第01话/imgi_11_g%2F公主的秘密与秘密的私生子%2F第01话%2F11.jpg",
            ],
        );
    }

    /// `read_image_at` 必须跟 `list_image_names` 用同一个自然序——
    /// 详情页 idx=0 的图必须跟 grid 第一个 cell 是同一张。
    #[test]
    fn read_image_at_uses_natural_sort_index() {
        let zip_bytes = build_test_zip(&[
            (
                "第01话/imgi_10_g%2F...%2F10.jpg",
                b"img-10",
            ),
            (
                "第01话/imgi_2_g%2F...%2F01.jpg",
                b"img-1",
            ),
            (
                "第01话/imgi_3_g%2F...%2F02.jpg",
                b"img-2",
            ),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("out_of_order.zip");
        std::fs::write(&p, zip_bytes).unwrap();

        // idx=0 必须是 01（自然序第一位），不是 zip 内写入顺序第一位（10）。
        let (n, d) = read_image_at(&p, 0).unwrap();
        assert!(n.contains("01.jpg"), "idx=0 should be the 01.jpg entry, got {}", n);
        assert_eq!(d, b"img-1");
        let (n, d) = read_image_at(&p, 2).unwrap();
        assert!(n.contains("10.jpg"), "idx=2 should be the 10.jpg entry, got {}", n);
        assert_eq!(d, b"img-10");
    }

    /// `list_images` 跟 `list_image_names` 必须用同一个自然序，
    /// cover 提取用的 `pick_cover` 在没有 cover 关键字时回退到
    /// `candidates.first()`，这个 first 应当跟详情页 grid 的第 0
    /// 张图是同一张。
    #[test]
    fn list_images_uses_natural_sort_index() {
        let zip_bytes = build_test_zip(&[
            ("第01话/imgi_10_g%2F...%2F10.jpg", b"img-10"),
            ("第01话/imgi_2_g%2F...%2F01.jpg", b"img-1"),
            ("第01话/imgi_3_g%2F...%2F02.jpg", b"img-2"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("out_of_order.zip");
        std::fs::write(&p, zip_bytes).unwrap();

        let images = list_images(&p).unwrap();
        assert!(images[0].name.contains("01.jpg"));
        assert!(images[2].name.contains("10.jpg"));
    }

    /// `pick_cover` 在无 cover 关键字时回退到 first；这个 first 必
    /// 须是自然序的第一张，而不是 zip 写入顺序的第一张。
    #[test]
    fn pick_cover_falls_back_to_natural_first() {
        let zip_bytes = build_test_zip(&[
            ("第01话/imgi_10_g%2F...%2F10.jpg", b"img-10"),
            ("第01话/imgi_2_g%2F...%2F01.jpg", b"img-1"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("out_of_order.zip");
        std::fs::write(&p, zip_bytes).unwrap();

        let images = list_images(&p).unwrap();
        let cover = pick_cover(&images).unwrap();
        assert!(
            cover.name.contains("01.jpg"),
            "pick_cover should fall back to natural-sort first (01.jpg), got {}",
            cover.name
        );
    }
}

