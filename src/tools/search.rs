use crate::FilesystemService;
use crate::error::FsError;
use globset::Glob;
use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::util::format_size;

/// Parameters for the search_files tool.
#[derive(Deserialize, Serialize, JsonSchema)]
struct SearchFilesParams {
    /// Absolute path to the directory to search in
    path: String,
    /// Glob pattern to match file paths against (e.g., "*.rs", "**/*.txt")
    pattern: String,
    /// Maximum number of results to return (default: 50, max: 200)
    #[schemars(description = "Maximum number of results to return (default: 50, max: 200)")]
    max_results: Option<u32>,
}

#[rmcp::tool_router(router = "search_tools_router", vis = "pub(crate)")]
impl FilesystemService {
    /// Searches for files matching a glob pattern within a directory tree.
    #[rmcp::tool(
        name = "search_files",
        description = "Searches for files matching a glob pattern within a directory tree. Returns matched file paths with sizes. Use '*.ext' for files in the root directory, '**/*.ext' for recursive matching.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    async fn search_files(
        &self,
        Parameters(params): Parameters<SearchFilesParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self
            .security
            .validate_directory(path)
            .map_err(|e| e.to_string())?;

        let matcher = Glob::new(&params.pattern)
            .map_err(|e| FsError::PatternError(e.to_string()).to_string())?
            .compile_matcher();

        let max_results = params.max_results.unwrap_or(50).min(200) as usize;
        let max_depth = self.config.max_depth;

        let mut results: Vec<(std::path::PathBuf, u64)> = Vec::new();
        let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(canonical.clone(), 0)];

        while let Some((dir, depth)) = stack.pop() {
            let mut entries = match tokio::fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(_) => continue,
            };

            let mut subdirs = Vec::new();

            loop {
                match entries.next_entry().await {
                    Ok(Some(entry)) => {
                        let metadata = match entry.metadata().await {
                            Ok(m) => m,
                            Err(_) => continue,
                        };

                        let entry_path = entry.path();

                        if metadata.is_dir() && depth < max_depth {
                            subdirs.push(entry_path);
                        } else if metadata.is_file() {
                            let relative =
                                entry_path.strip_prefix(&canonical).unwrap_or(&entry_path);
                            if matcher.is_match(relative) {
                                results.push((entry_path, metadata.len()));
                                if results.len() >= max_results {
                                    return Ok(format_search_results(
                                        &canonical,
                                        &params.pattern,
                                        &results,
                                        true,
                                    ));
                                }
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }

            subdirs.sort();
            for subdir in subdirs.into_iter().rev() {
                stack.push((subdir, depth + 1));
            }
        }

        Ok(format_search_results(
            &canonical,
            &params.pattern,
            &results,
            false,
        ))
    }
}

fn format_search_results(
    root: &std::path::Path,
    pattern: &str,
    results: &[(std::path::PathBuf, u64)],
    truncated: bool,
) -> String {
    if results.is_empty() {
        return format!(
            "No matches found for pattern \"{}\" in {}",
            pattern,
            root.display()
        );
    }

    let mut output = format!(
        "Found {} match{} for pattern \"{}\" in {}{}:\n\n",
        results.len(),
        if results.len() == 1 { "" } else { "es" },
        pattern,
        root.display(),
        if truncated {
            " (results truncated)"
        } else {
            ""
        },
    );

    for (path, size) in results {
        let size_str = format_size(*size);
        output.push_str(&format!("{} ({})\n", path.display(), size_str));
    }

    output
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

    fn make_service_with_depth(dirs: Vec<PathBuf>, max_depth: usize) -> FilesystemService {
        let config = Config {
            allowed_directories: dirs,
            allow_write: false,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth,
        };
        FilesystemService::new(config)
    }

    #[test]
    fn search_tools_router_contains_search_files() {
        let router = FilesystemService::search_tools_router();
        let tools = router.list_all();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name.as_ref(), "search_files");
    }

    #[tokio::test]
    async fn search_files_finds_matching() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("lib.rs"), "// lib").unwrap();
        std::fs::write(dir.path().join("readme.md"), "# Readme").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .search_files(Parameters(SearchFilesParams {
                path: dir.path().to_string_lossy().to_string(),
                pattern: "*.rs".to_string(),
                max_results: None,
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("main.rs"));
        assert!(output.contains("lib.rs"));
        assert!(!output.contains("readme.md"));
        assert!(output.contains("2 matches"));
    }

    #[tokio::test]
    async fn search_files_invalid_glob() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .search_files(Parameters(SearchFilesParams {
                path: dir.path().to_string_lossy().to_string(),
                pattern: "[invalid".to_string(),
                max_results: None,
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid pattern"));
    }

    #[tokio::test]
    async fn search_files_respects_max_results() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        for i in 0..10 {
            std::fs::write(dir.path().join(format!("file{i}.txt")), "content").unwrap();
        }

        let service = make_service(vec![canon]);
        let result = service
            .search_files(Parameters(SearchFilesParams {
                path: dir.path().to_string_lossy().to_string(),
                pattern: "*.txt".to_string(),
                max_results: Some(3),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("3 matches"));
        assert!(output.contains("truncated"));
    }

    #[tokio::test]
    async fn search_files_respects_max_depth() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("root.txt"), "root").unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("nested.txt"), "nested").unwrap();
        let deep = sub.join("deep");
        std::fs::create_dir(&deep).unwrap();
        std::fs::write(deep.join("deep.txt"), "deep").unwrap();

        let service = make_service_with_depth(vec![canon], 1);
        let result = service
            .search_files(Parameters(SearchFilesParams {
                path: dir.path().to_string_lossy().to_string(),
                pattern: "**/*.txt".to_string(),
                max_results: None,
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("root.txt"));
        assert!(output.contains("nested.txt"));
        assert!(!output.contains("deep.txt"));
    }

    #[tokio::test]
    async fn search_files_no_matches() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("file.txt"), "content").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .search_files(Parameters(SearchFilesParams {
                path: dir.path().to_string_lossy().to_string(),
                pattern: "*.rs".to_string(),
                max_results: None,
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("No matches"));
    }

    #[tokio::test]
    async fn search_files_denied_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);

        let other = TempDir::new().unwrap();
        let result = service
            .search_files(Parameters(SearchFilesParams {
                path: other.path().to_string_lossy().to_string(),
                pattern: "*.txt".to_string(),
                max_results: None,
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Access denied"));
    }
}
