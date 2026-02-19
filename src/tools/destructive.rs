use crate::FilesystemService;
use crate::error::io_error_message;
use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema)]
struct DeleteFileParams {
    /// Absolute path to the file to delete
    path: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
struct MoveFileParams {
    /// Absolute path to the source file or directory
    source: String,
    /// Absolute path to the destination
    destination: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
struct DeleteDirectoryParams {
    /// Absolute path to the empty directory to delete
    path: String,
}

#[rmcp::tool_router(router = "destructive_tools_router", vis = "pub(crate)")]
impl FilesystemService {
    #[rmcp::tool(
        name = "delete_file",
        description = "Deletes a single file. The file must exist and be a regular file (not a directory).",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn delete_file(
        &self,
        Parameters(params): Parameters<DeleteFileParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self.security.validate_file(path).map_err(|e| e.to_string())?;
        tokio::fs::remove_file(&canonical)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;
        Ok(format!("Deleted file {}", canonical.display()))
    }

    #[rmcp::tool(
        name = "move_file",
        description = "Moves or renames a file or directory. Both source and destination must be within allowed directories. The source must exist.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn move_file(
        &self,
        Parameters(params): Parameters<MoveFileParams>,
    ) -> Result<String, String> {
        let source = std::path::Path::new(&params.source);
        let destination = std::path::Path::new(&params.destination);
        let canonical_source = self
            .security
            .validate_path_exists(source)
            .map_err(|e| e.to_string())?;
        let canonical_dest = self
            .security
            .validate_path(destination)
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(&canonical_source, &canonical_dest)
            .await
            .map_err(|e| io_error_message(e, &params.source))?;
        Ok(format!(
            "Moved {} to {}",
            canonical_source.display(),
            canonical_dest.display()
        ))
    }

    #[rmcp::tool(
        name = "delete_directory",
        description = "Deletes an empty directory. The directory must exist and be empty. Does NOT recursively delete contents.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn delete_directory(
        &self,
        Parameters(params): Parameters<DeleteDirectoryParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self
            .security
            .validate_directory(path)
            .map_err(|e| e.to_string())?;
        tokio::fs::remove_dir(&canonical)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;
        Ok(format!("Deleted directory {}", canonical.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_service(dirs: Vec<PathBuf>) -> FilesystemService {
        let config = Config {
            allowed_directories: dirs,
            allow_write: true,
            allow_destructive: true,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        FilesystemService::new(config)
    }

    #[tokio::test]
    async fn delete_file_success() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let file = dir.path().join("doomed.txt");
        std::fs::write(&file, "goodbye").unwrap();
        assert!(file.exists());
        let service = make_service(vec![canon]);
        let result = service
            .delete_file(Parameters(DeleteFileParams {
                path: file.to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.unwrap().contains("Deleted file"));
        assert!(!file.exists());
    }

    #[tokio::test]
    async fn delete_file_not_found() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);
        let result = service
            .delete_file(Parameters(DeleteFileParams {
                path: dir.path().join("nope.txt").to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_file_rejects_directory() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).unwrap();
        let service = make_service(vec![canon]);
        let result = service
            .delete_file(Parameters(DeleteFileParams {
                path: sub.to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_file_denied_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);
        let other = TempDir::new().unwrap();
        let outside = other.path().join("secret.txt");
        std::fs::write(&outside, "secret").unwrap();
        let result = service
            .delete_file(Parameters(DeleteFileParams {
                path: outside.to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert!(outside.exists());
    }

    #[tokio::test]
    async fn move_file_rename_success() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let src = dir.path().join("old.txt");
        let dst = dir.path().join("new.txt");
        std::fs::write(&src, "content").unwrap();
        let service = make_service(vec![canon]);
        let result = service
            .move_file(Parameters(MoveFileParams {
                source: src.to_string_lossy().to_string(),
                destination: dst.to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.unwrap().contains("Moved"));
        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "content");
    }

    #[tokio::test]
    async fn move_file_directory_rename() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let src_dir = dir.path().join("old_dir");
        std::fs::create_dir(&src_dir).unwrap();
        std::fs::write(src_dir.join("inner.txt"), "inner").unwrap();
        let dst_dir = dir.path().join("new_dir");
        let service = make_service(vec![canon]);
        let result = service
            .move_file(Parameters(MoveFileParams {
                source: src_dir.to_string_lossy().to_string(),
                destination: dst_dir.to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_ok());
        assert!(!src_dir.exists());
        assert_eq!(
            std::fs::read_to_string(dst_dir.join("inner.txt")).unwrap(),
            "inner"
        );
    }

    #[tokio::test]
    async fn move_file_source_not_found() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);
        let result = service
            .move_file(Parameters(MoveFileParams {
                source: dir.path().join("nope.txt").to_string_lossy().to_string(),
                destination: dir.path().join("dest.txt").to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn move_file_denied_source_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);
        let other = TempDir::new().unwrap();
        let outside = other.path().join("secret.txt");
        std::fs::write(&outside, "secret").unwrap();
        let result = service
            .move_file(Parameters(MoveFileParams {
                source: outside.to_string_lossy().to_string(),
                destination: dir.path().join("stolen.txt").to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn move_file_denied_destination_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let src = dir.path().join("file.txt");
        std::fs::write(&src, "data").unwrap();
        let service = make_service(vec![canon]);
        let other = TempDir::new().unwrap();
        let result = service
            .move_file(Parameters(MoveFileParams {
                source: src.to_string_lossy().to_string(),
                destination: other.path().join("exfil.txt").to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert!(src.exists());
    }

    #[tokio::test]
    async fn delete_directory_empty_success() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let sub = dir.path().join("empty_dir");
        std::fs::create_dir(&sub).unwrap();
        let service = make_service(vec![canon]);
        let result = service
            .delete_directory(Parameters(DeleteDirectoryParams {
                path: sub.to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.unwrap().contains("Deleted directory"));
        assert!(!sub.exists());
    }

    #[tokio::test]
    async fn delete_directory_rejects_nonempty() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let sub = dir.path().join("nonempty");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("file.txt"), "data").unwrap();
        let service = make_service(vec![canon]);
        let result = service
            .delete_directory(Parameters(DeleteDirectoryParams {
                path: sub.to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert!(sub.exists());
    }

    #[tokio::test]
    async fn delete_directory_rejects_file() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let file = dir.path().join("not_a_dir.txt");
        std::fs::write(&file, "data").unwrap();
        let service = make_service(vec![canon]);
        let result = service
            .delete_directory(Parameters(DeleteDirectoryParams {
                path: file.to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_directory_denied_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);
        let other = TempDir::new().unwrap();
        let outside = other.path().join("secret_dir");
        std::fs::create_dir(&outside).unwrap();
        let result = service
            .delete_directory(Parameters(DeleteDirectoryParams {
                path: outside.to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert!(outside.exists());
    }
}
