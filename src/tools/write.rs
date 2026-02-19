use crate::FilesystemService;
use crate::error::{FsError, io_error_message};
use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use similar::TextDiff;

use super::util::format_size;

/// A single text replacement within a file.
#[derive(Deserialize, Serialize, JsonSchema)]
struct EditOperation {
    /// The exact text to search for in the file
    old_text: String,
    /// The text to replace it with
    new_text: String,
}

/// Parameters for the edit_file tool.
#[derive(Deserialize, Serialize, JsonSchema)]
struct EditFileParams {
    /// Absolute path to the file to edit
    path: String,
    /// List of edit operations to apply sequentially
    edits: Vec<EditOperation>,
}

/// Parameters for the write_file tool.
#[derive(Deserialize, Serialize, JsonSchema)]
struct WriteFileParams {
    /// Absolute path to the file to create or overwrite
    path: String,
    /// The content to write
    content: String,
}

/// Parameters for the create_directory tool.
#[derive(Deserialize, Serialize, JsonSchema)]
struct CreateDirectoryParams {
    /// Absolute path to the directory to create (supports nested creation like mkdir -p)
    path: String,
}

#[rmcp::tool_router(router = "write_tools_router", vis = "pub(crate)")]
impl FilesystemService {
    /// Applies a sequence of exact-text replacements to a file and returns a unified diff.
    #[rmcp::tool(
        name = "edit_file",
        description = "Applies a sequence of exact-text replacements to a file. Each edit must match exactly one location. Returns a unified diff of all changes.",
        annotations(read_only_hint = false, destructive_hint = false)
    )]
    async fn edit_file(
        &self,
        Parameters(params): Parameters<EditFileParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self
            .security
            .validate_file(path)
            .map_err(|e| e.to_string())?;

        let original = tokio::fs::read_to_string(&canonical)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;

        let mut content = original.clone();

        for edit in &params.edits {
            let count = content.matches(&edit.old_text).count();
            if count == 0 {
                return Err(FsError::EditFailed {
                    path: params.path.clone(),
                    reason: format!(
                        "old_text not found: {:?}",
                        edit.old_text.chars().take(80).collect::<String>()
                    ),
                }
                .to_string());
            }
            if count > 1 {
                return Err(FsError::EditFailed {
                    path: params.path.clone(),
                    reason: format!(
                        "old_text matches {} locations (must be unique): {:?}",
                        count,
                        edit.old_text.chars().take(80).collect::<String>()
                    ),
                }
                .to_string());
            }
            content = content.replacen(&edit.old_text, &edit.new_text, 1);
        }

        tokio::fs::write(&canonical, &content)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;

        let diff = TextDiff::from_lines(&original, &content);
        let unified = diff
            .unified_diff()
            .header(&params.path, &params.path)
            .to_string();

        Ok(format!(
            "Applied {} edit(s) to {}\n\n{}",
            params.edits.len(),
            canonical.display(),
            unified,
        ))
    }

    /// Creates or overwrites a file with the given content.
    #[rmcp::tool(
        name = "write_file",
        description = "Creates a new file or overwrites an existing file with the provided content. Parent directory must already exist.",
        annotations(read_only_hint = false, destructive_hint = true)
    )]
    async fn write_file(
        &self,
        Parameters(params): Parameters<WriteFileParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self
            .security
            .validate_path(path)
            .map_err(|e| e.to_string())?;

        tokio::fs::write(&canonical, &params.content)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;

        let size = params.content.len() as u64;
        Ok(format!(
            "Wrote {} to {}",
            format_size(size),
            canonical.display(),
        ))
    }

    /// Creates a directory (and any necessary parent directories).
    #[rmcp::tool(
        name = "create_directory",
        description = "Creates a directory and any necessary parent directories (like mkdir -p). Succeeds silently if the directory already exists.",
        annotations(read_only_hint = false, destructive_hint = false)
    )]
    async fn create_directory(
        &self,
        Parameters(params): Parameters<CreateDirectoryParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self
            .security
            .validate_creatable_path(path)
            .map_err(|e| e.to_string())?;

        tokio::fs::create_dir_all(&canonical)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;

        Ok(format!("Created directory {}", canonical.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, FilesystemService};
    use rmcp::handler::server::wrapper::Parameters;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_service(dirs: Vec<PathBuf>) -> FilesystemService {
        let config = Config {
            allowed_directories: dirs,
            allow_write: true,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        FilesystemService::new(config)
    }

    // --- Router tests ---

    #[test]
    fn write_tools_router_contains_all_three() {
        let router = FilesystemService::write_tools_router();
        let tools = router.list_all();
        assert_eq!(tools.len(), 3);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"create_directory"));
    }

    #[test]
    fn edit_file_annotations_correct() {
        let router = FilesystemService::write_tools_router();
        let tool = router.get("edit_file").unwrap();
        let ann = tool.annotations.as_ref().unwrap();
        assert_eq!(ann.read_only_hint, Some(false));
        assert_eq!(ann.destructive_hint, Some(false));
    }

    #[test]
    fn write_file_annotations_correct() {
        let router = FilesystemService::write_tools_router();
        let tool = router.get("write_file").unwrap();
        let ann = tool.annotations.as_ref().unwrap();
        assert_eq!(ann.read_only_hint, Some(false));
        assert_eq!(ann.destructive_hint, Some(true));
    }

    #[test]
    fn create_directory_annotations_correct() {
        let router = FilesystemService::write_tools_router();
        let tool = router.get("create_directory").unwrap();
        let ann = tool.annotations.as_ref().unwrap();
        assert_eq!(ann.read_only_hint, Some(false));
        assert_eq!(ann.destructive_hint, Some(false));
    }

    // --- Conditional visibility tests ---

    #[test]
    fn write_tools_hidden_when_flag_off() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let config = Config {
            allowed_directories: vec![canon],
            allow_write: false,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        let service = FilesystemService::new(config);
        let tools = service.tool_router.list_all();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(!names.contains(&"edit_file"));
        assert!(!names.contains(&"write_file"));
        assert!(!names.contains(&"create_directory"));
        assert_eq!(tools.len(), 7);
    }

    #[test]
    fn write_tools_visible_when_flag_on() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let config = Config {
            allowed_directories: vec![canon],
            allow_write: true,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        let service = FilesystemService::new(config);
        let tools = service.tool_router.list_all();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"edit_file"));
        assert!(names.contains(&"write_file"));
        assert!(names.contains(&"create_directory"));
        assert_eq!(tools.len(), 10);
    }

    // --- edit_file tests ---

    #[tokio::test]
    async fn edit_file_success() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "Hello World\nGoodbye World\n").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .edit_file(Parameters(EditFileParams {
                path: file.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_text: "Hello".to_string(),
                    new_text: "Hi".to_string(),
                }],
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("Applied 1 edit(s)"));
        assert!(output.contains("-Hello World"));
        assert!(output.contains("+Hi World"));

        let on_disk = std::fs::read_to_string(&file).unwrap();
        assert_eq!(on_disk, "Hi World\nGoodbye World\n");
    }

    #[tokio::test]
    async fn edit_file_not_found() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .edit_file(Parameters(EditFileParams {
                path: dir
                    .path()
                    .join("nonexistent.txt")
                    .to_string_lossy()
                    .to_string(),
                edits: vec![EditOperation {
                    old_text: "x".to_string(),
                    new_text: "y".to_string(),
                }],
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn edit_file_old_text_missing() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "Hello World\n").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .edit_file(Parameters(EditFileParams {
                path: file.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_text: "NONEXISTENT".to_string(),
                    new_text: "y".to_string(),
                }],
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("old_text not found"));
    }

    #[tokio::test]
    async fn edit_file_ambiguous_match() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let file = dir.path().join("test.txt");
        std::fs::write(&file, "foo bar foo\n").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .edit_file(Parameters(EditFileParams {
                path: file.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_text: "foo".to_string(),
                    new_text: "baz".to_string(),
                }],
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("matches 2 locations"));
    }

    #[tokio::test]
    async fn edit_file_diff_output_format() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let file = dir.path().join("code.rs");
        std::fs::write(&file, "fn main() {\n    println!(\"old\");\n}\n").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .edit_file(Parameters(EditFileParams {
                path: file.to_string_lossy().to_string(),
                edits: vec![EditOperation {
                    old_text: "\"old\"".to_string(),
                    new_text: "\"new\"".to_string(),
                }],
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("---"));
        assert!(output.contains("+++"));
        assert!(output.contains("@@"));
    }

    // --- write_file tests ---

    #[tokio::test]
    async fn write_file_create_new() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let file = dir.path().join("new.txt");

        let service = make_service(vec![canon]);
        let result = service
            .write_file(Parameters(WriteFileParams {
                path: file.to_string_lossy().to_string(),
                content: "Hello, new file!\n".to_string(),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("Wrote"));
        assert!(output.contains("17 B"));

        let on_disk = std::fs::read_to_string(&file).unwrap();
        assert_eq!(on_disk, "Hello, new file!\n");
    }

    #[tokio::test]
    async fn write_file_overwrite_existing() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let file = dir.path().join("existing.txt");
        std::fs::write(&file, "old content").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .write_file(Parameters(WriteFileParams {
                path: file.to_string_lossy().to_string(),
                content: "new content".to_string(),
            }))
            .await;

        assert!(result.is_ok());
        let on_disk = std::fs::read_to_string(&file).unwrap();
        assert_eq!(on_disk, "new content");
    }

    #[tokio::test]
    async fn write_file_denied_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);

        let other = TempDir::new().unwrap();
        let result = service
            .write_file(Parameters(WriteFileParams {
                path: other.path().join("hack.txt").to_string_lossy().to_string(),
                content: "pwned".to_string(),
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Access denied"));
    }

    // --- create_directory tests ---

    #[tokio::test]
    async fn create_directory_single() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let new_dir = dir.path().join("newdir");

        let service = make_service(vec![canon]);
        let result = service
            .create_directory(Parameters(CreateDirectoryParams {
                path: new_dir.to_string_lossy().to_string(),
            }))
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().contains("Created directory"));
        assert!(new_dir.is_dir());
    }

    #[tokio::test]
    async fn create_directory_nested() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let deep = dir.path().join("a").join("b").join("c");

        let service = make_service(vec![canon]);
        let result = service
            .create_directory(Parameters(CreateDirectoryParams {
                path: deep.to_string_lossy().to_string(),
            }))
            .await;

        assert!(result.is_ok());
        assert!(deep.is_dir());
    }

    #[tokio::test]
    async fn create_directory_existing_ok() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let sub = dir.path().join("exists");
        std::fs::create_dir(&sub).unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .create_directory(Parameters(CreateDirectoryParams {
                path: sub.to_string_lossy().to_string(),
            }))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn create_directory_denied_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);

        let other = TempDir::new().unwrap();
        let result = service
            .create_directory(Parameters(CreateDirectoryParams {
                path: other.path().join("hack").to_string_lossy().to_string(),
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Access denied"));
    }
}
