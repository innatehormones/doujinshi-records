use anyhow::{anyhow, Result};
use std::path::Path;

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
}

