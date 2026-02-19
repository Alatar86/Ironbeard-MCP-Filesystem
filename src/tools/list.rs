use crate::FilesystemService;
use crate::error::io_error_message;
use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::util::{format_date, format_size};

const MAX_DIR_ENTRIES: usize = 1000;

/// Parameters for the list_directory tool.
#[derive(Deserialize, Serialize, JsonSchema)]
struct ListDirectoryParams {
    /// Absolute path to the directory to list
    path: String,
}

impl FilesystemService {
    /// Formats the allowed directories as a newline-separated string of canonical paths.
    pub fn format_allowed_directories(&self) -> String {
        self.config
            .allowed_directories
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[rmcp::tool_router(router = "list_tools_router", vis = "pub(crate)")]
impl FilesystemService {
    /// Lists all directories that this server is allowed to access.
    ///
    /// Returns each allowed directory on its own line as a fully canonicalized path.
    /// Use this to discover which directories you can read from or write to.
    #[rmcp::tool(
        name = "list_allowed_directories",
        description = "Lists all directories that this server is allowed to access. Returns each allowed directory on its own line as a fully canonicalized path.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    fn list_allowed_directories(&self) -> String {
        self.format_allowed_directories()
    }

    /// Lists the contents of a directory with type, name, size, and modification date.
    #[rmcp::tool(
        name = "list_directory",
        description = "Lists the contents of a directory. Returns entries sorted with directories first, then files, each alphabetically. Each entry shows type, name, and for files, size and modification date.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    async fn list_directory(
        &self,
        Parameters(params): Parameters<ListDirectoryParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self
            .security
            .validate_directory(path)
            .map_err(|e| e.to_string())?;

        let mut dirs: Vec<String> = Vec::new();
        let mut files: Vec<String> = Vec::new();

        let mut entries = tokio::fs::read_dir(&canonical)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;

        loop {
            match entries.next_entry().await {
                Ok(Some(entry)) => {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let metadata = match entry.metadata().await {
                        Ok(m) => m,
                        Err(_) => continue,
                    };

                    if metadata.is_dir() {
                        dirs.push(format!("[DIR]  {name}/"));
                    } else if metadata.is_file() {
                        let size = format_size(metadata.len());
                        let modified = metadata
                            .modified()
                            .map(format_date)
                            .unwrap_or_else(|_| "unknown".to_string());
                        files.push(format!("[FILE] {name} ({size}, {modified})"));
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        dirs.sort();
        files.sort();

        let mut lines = dirs;
        lines.extend(files);

        if lines.is_empty() {
            Ok("(empty directory)".to_string())
        } else if lines.len() > MAX_DIR_ENTRIES {
            let total = lines.len();
            lines.truncate(MAX_DIR_ENTRIES);
            lines.push(format!(
                "\n(Showing first {MAX_DIR_ENTRIES} of {total} entries. Use search_files to find specific files.)"
            ));
            Ok(lines.join("\n"))
        } else {
            Ok(lines.join("\n"))
        }
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
            allow_write: false,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        FilesystemService::new(config)
    }

    #[test]
    fn format_single_directory() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon.clone()]);
        let result = service.format_allowed_directories();
        assert_eq!(result, canon.display().to_string());
    }

    #[test]
    fn format_multiple_directories() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let c1 = dir1.path().canonicalize().unwrap();
        let c2 = dir2.path().canonicalize().unwrap();
        let service = make_service(vec![c1.clone(), c2.clone()]);
        let result = service.format_allowed_directories();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.contains(&c1.display().to_string().as_str()));
        assert!(lines.contains(&c2.display().to_string().as_str()));
    }

    #[test]
    fn format_empty_directories() {
        let service = make_service(vec![]);
        let result = service.format_allowed_directories();
        assert!(result.is_empty());
    }

    #[test]
    fn tool_router_contains_both_list_tools() {
        let router = FilesystemService::list_tools_router();
        let tools = router.list_all();
        assert_eq!(tools.len(), 2);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"list_allowed_directories"));
        assert!(names.contains(&"list_directory"));
    }

    #[test]
    fn tool_has_correct_annotations() {
        let router = FilesystemService::list_tools_router();
        let tool = router.get("list_allowed_directories").unwrap();
        let annotations = tool.annotations.as_ref().unwrap();
        assert_eq!(annotations.read_only_hint, Some(true));
        assert_eq!(annotations.destructive_hint, Some(false));
    }

    #[tokio::test]
    async fn list_directory_shows_contents() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hello world").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .list_directory(Parameters(ListDirectoryParams {
                path: dir.path().to_string_lossy().to_string(),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("[DIR]  subdir/"));
        assert!(output.contains("[FILE] hello.txt"));
        assert!(output.contains("11 B"));
        // dirs come before files
        let dir_pos = output.find("[DIR]").unwrap();
        let file_pos = output.find("[FILE]").unwrap();
        assert!(dir_pos < file_pos);
    }

    #[tokio::test]
    async fn list_directory_empty_dir() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);
        let result = service
            .list_directory(Parameters(ListDirectoryParams {
                path: dir.path().to_string_lossy().to_string(),
            }))
            .await;
        assert_eq!(result.unwrap(), "(empty directory)");
    }

    #[tokio::test]
    async fn list_directory_denied_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);

        let other = TempDir::new().unwrap();
        let result = service
            .list_directory(Parameters(ListDirectoryParams {
                path: other.path().to_string_lossy().to_string(),
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Access denied"));
    }

    #[tokio::test]
    async fn list_directory_sorted_alphabetically() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::create_dir(dir.path().join("zeta")).unwrap();
        std::fs::create_dir(dir.path().join("alpha")).unwrap();
        std::fs::write(dir.path().join("banana.txt"), "b").unwrap();
        std::fs::write(dir.path().join("apple.txt"), "a").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .list_directory(Parameters(ListDirectoryParams {
                path: dir.path().to_string_lossy().to_string(),
            }))
            .await;

        let output = result.unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines[0].contains("alpha/"));
        assert!(lines[1].contains("zeta/"));
        assert!(lines[2].contains("apple.txt"));
        assert!(lines[3].contains("banana.txt"));
    }

    #[tokio::test]
    async fn list_directory_truncates_large() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        for i in 0..1005 {
            std::fs::write(dir.path().join(format!("file{i:04}.txt")), "x").unwrap();
        }

        let service = make_service(vec![canon]);
        let result = service
            .list_directory(Parameters(ListDirectoryParams {
                path: dir.path().to_string_lossy().to_string(),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("Showing first 1000"));
        assert!(output.contains("1005 entries"));
        assert!(output.contains("search_files"));
        let file_lines: Vec<&str> = output.lines().filter(|l| l.starts_with("[FILE]")).collect();
        assert_eq!(file_lines.len(), 1000);
    }
}
