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

