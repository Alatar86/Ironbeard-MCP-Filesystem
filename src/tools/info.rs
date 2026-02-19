use crate::FilesystemService;
use crate::error::io_error_message;
use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::util::{format_date, format_permissions, format_size};

const MAX_TREE_ENTRIES: usize = 1000;

/// Parameters for the get_file_info tool.
#[derive(Deserialize, Serialize, JsonSchema)]
struct GetFileInfoParams {
    /// Absolute path to the file or directory
    path: String,
}

/// Parameters for the directory_tree tool.
#[derive(Deserialize, Serialize, JsonSchema)]
struct DirectoryTreeParams {
    /// Absolute path to the directory
    path: String,
    /// Maximum depth to traverse (defaults to config max_depth)
    #[schemars(description = "Maximum depth to traverse")]
    max_depth: Option<u32>,
}

#[rmcp::tool_router(router = "info_tools_router", vis = "pub(crate)")]
impl FilesystemService {
    /// Returns detailed metadata about a file or directory.
    #[rmcp::tool(
        name = "get_file_info",
        description = "Returns detailed metadata about a file or directory including size, type, MIME type, timestamps, and permissions.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    async fn get_file_info(
        &self,
        Parameters(params): Parameters<GetFileInfoParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self
            .security
            .validate_path_exists(path)
            .map_err(|e| e.to_string())?;

        let metadata = tokio::fs::symlink_metadata(&canonical)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;

        let file_type = if metadata.is_file() {
            "file"
        } else if metadata.is_dir() {
            "directory"
        } else if metadata.file_type().is_symlink() {
            "symlink"
        } else {
            "other"
        };

        let size_str = format_size(metadata.len());

        let mime = if metadata.is_file() {
            mime_guess::from_path(&canonical)
                .first()
                .map(|m| m.to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string())
        } else {
            "N/A".to_string()
        };

        let modified = metadata
            .modified()
            .map(format_date)
            .unwrap_or_else(|_| "unknown".to_string());

        let created = metadata
            .created()
            .map(format_date)
            .unwrap_or_else(|_| "unknown".to_string());

        let permissions = format_permissions(&metadata);

        Ok(format!(
            "Path: {}\nType: {}\nSize: {}\nMIME: {}\nModified: {}\nCreated: {}\nPermissions: {}",
            canonical.display(),
            file_type,
            size_str,
            mime,
            modified,
            created,
            permissions,
        ))
    }

    /// Displays a visual tree of directory structure with box-drawing characters.
    #[rmcp::tool(
        name = "directory_tree",
        description = "Displays a visual tree of directory structure with box-drawing characters. Shows directories first (sorted), then files with sizes. Hidden files/directories (starting with '.') are skipped by default.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    async fn directory_tree(
        &self,
        Parameters(params): Parameters<DirectoryTreeParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self
            .security
            .validate_directory(path)
            .map_err(|e| e.to_string())?;

        let max_depth = params
            .max_depth
            .map(|d| d as usize)
            .unwrap_or(self.config.max_depth);

        let canonical_clone = canonical.clone();
        let tree = tokio::task::spawn_blocking(move || {
            let mut count = 0;
            build_tree_sync(&canonical_clone, "", max_depth, 0, &mut count)
        })
        .await
        .map_err(|e| e.to_string())??;

        Ok(format!("{}/\n{}", canonical.display(), tree))
    }
}

fn build_tree_sync(
    dir: &std::path::Path,
    prefix: &str,
    max_depth: usize,
    current_depth: usize,
    entry_count: &mut usize,
) -> Result<String, String> {
    let read_dir = std::fs::read_dir(dir).map_err(|e| e.to_string())?;

    let mut dirs: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut files: Vec<(String, u64)> = Vec::new();

    for entry_result in read_dir {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name().to_string_lossy().to_string();

        if name.starts_with('.') {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if metadata.is_dir() {
            dirs.push((name, entry.path()));
        } else if metadata.is_file() {
            files.push((name, metadata.len()));
        }
    }

    dirs.sort_by(|a, b| a.0.cmp(&b.0));
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let total = dirs.len() + files.len();
    if total == 0 {
        return Ok(String::new());
    }

    let mut output = String::new();
    let mut index = 0;

    for (name, path) in &dirs {
        *entry_count += 1;
        if *entry_count > MAX_TREE_ENTRIES {
            output.push_str(&format!(
                "{prefix}... (truncated, exceeded {MAX_TREE_ENTRIES} entries. Use search_files to find specific files.)\n"
            ));
            return Ok(output);
        }
        let is_last = index == total - 1;
        let connector = if is_last {
            "\u{2514}\u{2500}\u{2500} "
        } else {
            "\u{251c}\u{2500}\u{2500} "
        };
        output.push_str(&format!("{prefix}{connector}{name}/\n"));

        if current_depth < max_depth {
            let child_prefix = if is_last {
                format!("{prefix}    ")
            } else {
                format!("{prefix}\u{2502}   ")
            };
            let subtree = build_tree_sync(
                path,
                &child_prefix,
                max_depth,
                current_depth + 1,
                entry_count,
            )?;
            output.push_str(&subtree);
            if *entry_count > MAX_TREE_ENTRIES {
                return Ok(output);
            }
        }

        index += 1;
    }

    for (name, size) in &files {
        *entry_count += 1;
        if *entry_count > MAX_TREE_ENTRIES {
            output.push_str(&format!(
                "{prefix}... (truncated, exceeded {MAX_TREE_ENTRIES} entries. Use search_files to find specific files.)\n"
            ));
            return Ok(output);
        }
        let is_last = index == total - 1;
        let connector = if is_last {
            "\u{2514}\u{2500}\u{2500} "
        } else {
            "\u{251c}\u{2500}\u{2500} "
        };
        let size_str = format_size(*size);
        output.push_str(&format!("{prefix}{connector}{name} ({size_str})\n"));
        index += 1;
    }

    Ok(output)
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
            allow_write: false,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        FilesystemService::new(config)
    }

    #[test]
    fn info_tools_router_contains_get_file_info() {
        let router = FilesystemService::info_tools_router();
        let tools = router.list_all();
        assert_eq!(tools.len(), 2);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"get_file_info"));
        assert!(names.contains(&"directory_tree"));
    }

    #[tokio::test]
    async fn get_file_info_for_file() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .get_file_info(Parameters(GetFileInfoParams {
                path: dir.path().join("test.txt").to_string_lossy().to_string(),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("Type: file"));
        assert!(output.contains("11 B"));
        assert!(output.contains("text/plain"));
    }

    #[tokio::test]
    async fn get_file_info_for_directory() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .get_file_info(Parameters(GetFileInfoParams {
                path: sub.to_string_lossy().to_string(),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("Type: directory"));
        assert!(output.contains("MIME: N/A"));
    }

    #[tokio::test]
    async fn get_file_info_mime_type() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("image.png"), "fake png").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .get_file_info(Parameters(GetFileInfoParams {
                path: dir.path().join("image.png").to_string_lossy().to_string(),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("image/png"));
    }

    #[tokio::test]
    async fn get_file_info_denied_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);

        let other = TempDir::new().unwrap();
        std::fs::write(other.path().join("secret.txt"), "secret").unwrap();
        let result = service
            .get_file_info(Parameters(GetFileInfoParams {
                path: other
                    .path()
                    .join("secret.txt")
                    .to_string_lossy()
                    .to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Access denied"));
    }

    #[tokio::test]
    async fn get_file_info_not_found() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);

        let result = service
            .get_file_info(Parameters(GetFileInfoParams {
                path: dir.path().join("nope.txt").to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not found"));
    }

    #[tokio::test]
    async fn directory_tree_correct_structure() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .directory_tree(Parameters(DirectoryTreeParams {
                path: dir.path().to_string_lossy().to_string(),
                max_depth: None,
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("src/"));
        assert!(output.contains("main.rs"));
        assert!(output.contains("Cargo.toml"));
        assert!(
            output.contains("\u{251c}\u{2500}\u{2500}")
                || output.contains("\u{2514}\u{2500}\u{2500}")
        );
    }

    #[tokio::test]
    async fn directory_tree_respects_max_depth() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let sub = dir.path().join("level1");
        std::fs::create_dir(&sub).unwrap();
        let deep = sub.join("level2");
        std::fs::create_dir(&deep).unwrap();
        std::fs::write(deep.join("deep.txt"), "deep").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .directory_tree(Parameters(DirectoryTreeParams {
                path: dir.path().to_string_lossy().to_string(),
                max_depth: Some(0),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("level1/"));
        assert!(!output.contains("level2"));
        assert!(!output.contains("deep.txt"));
    }

    #[tokio::test]
    async fn directory_tree_skips_hidden() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target/").unwrap();
        std::fs::write(dir.path().join("visible.txt"), "visible").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .directory_tree(Parameters(DirectoryTreeParams {
                path: dir.path().to_string_lossy().to_string(),
                max_depth: None,
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("visible.txt"));
        assert!(!output.contains(".hidden"));
        assert!(!output.contains(".gitignore"));
    }

    #[tokio::test]
    async fn directory_tree_denied_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);

        let other = TempDir::new().unwrap();
        let result = service
            .directory_tree(Parameters(DirectoryTreeParams {
                path: other.path().to_string_lossy().to_string(),
                max_depth: None,
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Access denied"));
    }

    #[tokio::test]
    async fn directory_tree_empty_dir() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .directory_tree(Parameters(DirectoryTreeParams {
                path: dir.path().to_string_lossy().to_string(),
                max_depth: None,
            }))
            .await;

        let output = result.unwrap();
        assert!(output.ends_with("/\n"));
    }

    #[tokio::test]
    async fn directory_tree_truncates_large() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        for i in 0..1005 {
            std::fs::write(dir.path().join(format!("file{i:04}.txt")), "x").unwrap();
        }

        let service = make_service(vec![canon]);
        let result = service
            .directory_tree(Parameters(DirectoryTreeParams {
                path: dir.path().to_string_lossy().to_string(),
                max_depth: None,
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("truncated"));
        assert!(output.contains("search_files"));
    }
}
