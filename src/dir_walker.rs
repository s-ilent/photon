use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Scans directories for image files and provides fast indexing
pub struct DirWalker {
    pub files: Vec<PathBuf>,
}

impl DirWalker {
    /// Creates a new DirWalker scanning the given directory for images
    pub fn new(path: &Path) -> Option<Self> {
        let base_path = path.to_path_buf();
        let is_dir = base_path.is_dir();

        let files: Vec<PathBuf> = if is_dir {
            // If it's a directory, scan for all images in it
            Self::scan_directory(&base_path)
        } else if base_path.is_file() {
            // If it's a file, use its parent directory and find its index
            if let Some(parent) = base_path.parent() {
                let mut files = Self::scan_directory(parent);
                files.sort();
                files
            } else {
                Vec::new()
            }
        } else {
            return None;
        };

        Some(Self { files })
    }

    /// Scans a directory for supported image files
    fn scan_directory(path: &Path) -> Vec<PathBuf> {
        WalkDir::new(path)
            .max_depth(1) // Only immediate directory, no recursion
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') // Skip hidden files
            })
            .filter(|e| Self::is_supported_image(e.path()))
            .map(|e| e.path().to_path_buf())
            .collect()
    }

    /// Checks if a file has a supported image extension
    fn is_supported_image(path: &Path) -> bool {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => {
                let ext_lower = ext.to_lowercase();
                matches!(
                    ext_lower.as_str(),
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "gif"
                        | "bmp"
                        | "tiff"
                        | "tif"
                        | "webp"
                        | "dds"
                        | "exr"
                )
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    #[test]
    fn test_is_supported_image() {
        assert!(DirWalker::is_supported_image(Path::new("test.png")));
        assert!(DirWalker::is_supported_image(Path::new("test.jpg")));
        assert!(DirWalker::is_supported_image(Path::new("test.PNG")));
        assert!(DirWalker::is_supported_image(Path::new("test.dds")));
        assert!(DirWalker::is_supported_image(Path::new("test.exr")));
        assert!(!DirWalker::is_supported_image(Path::new("test.txt")));
        assert!(!DirWalker::is_supported_image(Path::new("test")));
    }
}
