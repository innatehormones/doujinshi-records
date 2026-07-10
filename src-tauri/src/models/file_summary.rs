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
    pub marked_for_delete: bool,
    pub physically_deleted: bool,
    pub current_location: String,
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
        marked_for_delete: m.marked_for_delete,
        physically_deleted: m.physically_deleted,
        current_location: m.current_location.clone(),
        cover_url: m.cover_path.as_ref().map(|_| format!("/api/covers/{}", m.hash)),
    }
}
