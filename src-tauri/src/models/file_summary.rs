use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FileSummary {
    pub id: i64,
    pub title: String,
    pub circle: Option<String>,
    pub hash: String,
    pub ext: String,
    pub size_bytes: i64,
    pub viewed: bool,
    pub current_location: String,
    pub has_physical_file: bool,
    pub cover_url: Option<String>,
}

pub fn from_model(m: &crate::db::entities::doujinshi_file::Model) -> FileSummary {
    FileSummary {
        id: m.id,
        title: m.title.clone(),
        circle: m.circle.clone(),
        hash: m.hash.clone(),
        ext: m.ext.clone(),
        size_bytes: m.size_bytes,
        viewed: m.viewed,
        current_location: m.current_location.clone(),
        has_physical_file: m.has_physical_file,
        cover_url: m.cover_path.as_ref().map(|_| format!("/api/covers/{}", m.hash)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::entities::doujinshi_file;

    #[test]
    fn from_model_includes_location_and_has_physical_file() {
        let now = chrono::Utc::now();
        let m = doujinshi_file::Model {
            id: 1,
            title: "t".into(),
            filename: "f.zip".into(),
            hash: "h".into(),
            ext: "zip".into(),
            size_bytes: 100,
            circle: None,
            series: None,
            translator: None,
            version_tag: None,
            current_path: "p".into(),
            current_location: "archived".into(),
            cover_path: Some("covers/h.webp".into()),
            marked_for_delete: false,
            physically_deleted: true,
            has_physical_file: false,
            viewed: false,
            note: None,
            rating: None,
            created_at: now,
            updated_at: now,
        };
        let s = from_model(&m);
        assert_eq!(s.current_location, "archived");
        assert!(!s.has_physical_file);
        assert_eq!(s.cover_url.as_deref(), Some("/api/covers/h"));
    }
}