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
}
