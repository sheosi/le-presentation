use std::path::Path;
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;
use tokio_stream::StreamExt;

/// Checks that the directory this belongs to doesn't have another file whose name
/// starts with this files name.
pub async fn has_prefix_matches(file_path: &Path) -> Result<bool, std::io::Error> {
    fn get_stem(path: &Path) -> Option<&str> {
        path.file_stem().and_then(|n| n.to_str())
    }

    let Some(stem) = get_stem(file_path) else {
        return Ok(false);
    };

    let Some(parent) = file_path.parent() else {
        return Ok(false);
    };

    let dir = fs::read_dir(parent).await?;
    let mut stream = ReadDirStream::new(dir);

    while let Some(entry) = stream.next().await {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(file_name) = get_stem(&path) {
                if file_name.starts_with(stem) {
                    return Ok(true);
                }
            }
        }
    }

    Ok(false)
}
