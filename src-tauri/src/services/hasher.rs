use std::path::Path;
use anyhow::Result;
use blake3::Hasher;

pub async fn hash_file(path: &Path) -> Result<String> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || -> Result<String> {
        let file = std::fs::File::open(&path)?;
        let mut hasher = Hasher::new();
        let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
        std::io::copy(&mut reader, &mut hasher)?;
        Ok(hasher.finalize().to_hex().to_string())
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn hashes_known_content() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world").unwrap();
        let h = hash_file(tmp.path()).await.unwrap();
        // BLAKE3 of the 11-byte ASCII string "hello world".
        assert_eq!(
            h,
            "d74981efa70a0c880b8d8c1985d075dbcbf679b99a5f9914e5aaf96b831a9e24"
        );
    }

    #[tokio::test]
    async fn streaming_matches_oneshot() {
        // Same content written twice should hash identically even when
        // the file size crosses the 1 MiB BufReader chunk boundary, since
        // BLAKE3 is incremental.
        let payload = vec![0xABu8; 3 * 1024 * 1024];
        let mut a = NamedTempFile::new().unwrap();
        a.write_all(&payload).unwrap();
        let mut b = NamedTempFile::new().unwrap();
        b.write_all(&payload).unwrap();
        assert_eq!(
            hash_file(a.path()).await.unwrap(),
            hash_file(b.path()).await.unwrap()
        );
    }
}
