use crate::FilesystemService;
use crate::error::{FsError, io_error_message};
use rmcp::handler::server::wrapper::Parameters;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::util::format_size;

/// Number of bytes to check for null bytes when detecting binary files.
const BINARY_CHECK_SIZE: usize = 8192;

/// Parameters for the read_file tool.
#[derive(Deserialize, Serialize, JsonSchema)]
struct ReadFileParams {
    /// Absolute path to the file to read
    path: String,
    /// Line offset (0-based) to start reading from
    #[schemars(description = "Line offset (0-based) to start reading from")]
    offset: Option<u64>,
    /// Maximum number of lines to read
    #[schemars(description = "Maximum number of lines to read")]
    limit: Option<u64>,
}

/// Parameters for the read_multiple_files tool.
#[derive(Deserialize, Serialize, JsonSchema)]
struct ReadMultipleFilesParams {
    /// List of absolute file paths to read
    paths: Vec<String>,
}

#[rmcp::tool_router(router = "read_tools_router", vis = "pub(crate)")]
impl FilesystemService {
    /// Reads a file and returns its contents, optionally reading a specific line range.
    #[rmcp::tool(
        name = "read_file",
        description = "Reads a file and returns its contents. Supports reading specific line ranges using offset (0-based) and limit parameters. Returns a header with file path and line information.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    async fn read_file(
        &self,
        Parameters(params): Parameters<ReadFileParams>,
    ) -> Result<String, String> {
        let path = std::path::Path::new(&params.path);
        let canonical = self
            .security
            .validate_file(path)
            .map_err(|e| e.to_string())?;

        let metadata = tokio::fs::metadata(&canonical)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;
        let file_size = metadata.len();

        let has_range = params.offset.is_some() || params.limit.is_some();

        // Check file size limit (relaxed when offset/limit narrows the read)
        if !has_range && file_size > self.config.max_read_size as u64 {
            return Err(FsError::FileTooLarge {
                path: params.path,
                size: file_size,
                max: self.config.max_read_size as u64,
            }
            .to_string());
        }

        let content = tokio::fs::read(&canonical)
            .await
            .map_err(|e| io_error_message(e, &params.path))?;

        // Detect binary files (null bytes in first 8KB)
        let check_len = content.len().min(BINARY_CHECK_SIZE);
        if content[..check_len].contains(&0) {
            return Err(FsError::BinaryFile { path: params.path }.to_string());
        }

        let text = String::from_utf8_lossy(&content);
        let lines: Vec<&str> = text.lines().collect();
        let total_lines = lines.len();

        let size_str = format_size(file_size);

        // Handle empty files
        if total_lines == 0 {
            return Ok(format!(
                "File: {} (0 B)\n\n(empty file)",
                canonical.display()
            ));
        }

        let offset = params.offset.unwrap_or(0) as usize;
        let limit = params.limit.map(|l| l as usize);

        if offset >= total_lines {
            return Err(format!(
                "Offset {offset} is beyond end of file ({total_lines} lines)"
            ));
        }

        let end = match limit {
            Some(l) => (offset + l).min(total_lines),
            None => total_lines,
        };

        let selected = &lines[offset..end];

        let header = format!(
            "File: {} (Lines {}-{} of {} total, {})",
            canonical.display(),
            offset + 1,
            end,
            total_lines,
            size_str,
        );

        Ok(format!("{header}\n\n{}", selected.join("\n")))
    }

    /// Reads multiple files and returns their contents with clear separators.
    #[rmcp::tool(
        name = "read_multiple_files",
        description = "Reads multiple files and returns their contents with clear separators between each file. If any file fails to read, the error is included inline and remaining files are still processed.",
        annotations(read_only_hint = true, destructive_hint = false)
    )]
    async fn read_multiple_files(
        &self,
        Parameters(params): Parameters<ReadMultipleFilesParams>,
    ) -> Result<String, String> {
        let mut sections = Vec::new();

        for file_path in &params.paths {
            let path = std::path::Path::new(file_path);

            let result: Result<String, String> = async {
                let canonical = self
                    .security
                    .validate_file(path)
                    .map_err(|e| e.to_string())?;

                let metadata = tokio::fs::metadata(&canonical)
                    .await
                    .map_err(|e| io_error_message(e, file_path))?;
                let file_size = metadata.len();

                if file_size > self.config.max_read_size as u64 {
                    return Err(FsError::FileTooLarge {
                        path: file_path.clone(),
                        size: file_size,
                        max: self.config.max_read_size as u64,
                    }
                    .to_string());
                }

                let content = tokio::fs::read(&canonical)
                    .await
                    .map_err(|e| io_error_message(e, file_path))?;

                let check_len = content.len().min(BINARY_CHECK_SIZE);
                if content[..check_len].contains(&0) {
                    return Err(FsError::BinaryFile {
                        path: file_path.clone(),
                    }
                    .to_string());
                }

                let text = String::from_utf8_lossy(&content);
                let total_lines = text.lines().count();
                let size_str = format_size(file_size);

                Ok(format!(
                    "=== {} ({} lines, {}) ===\n{}",
                    canonical.display(),
                    total_lines,
                    size_str,
                    text,
                ))
            }
            .await;

            match result {
                Ok(section) => sections.push(section),
                Err(err) => sections.push(format!("=== {file_path} ===\nError: {err}")),
            }
        }

        Ok(sections.join("\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, FilesystemService};
    use rmcp::handler::server::wrapper::Parameters;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn make_service_with_max(dirs: Vec<PathBuf>, max_read_size: usize) -> FilesystemService {
        let config = Config {
            allowed_directories: dirs,
            allow_write: false,
            allow_destructive: false,
            max_read_size,
            max_depth: 10,
        };
        FilesystemService::new(config)
    }

    fn make_service(dirs: Vec<PathBuf>) -> FilesystemService {
        make_service_with_max(dirs, 10_485_760)
    }

    #[test]
    fn read_tools_router_contains_read_file() {
        let router = FilesystemService::read_tools_router();
        let tools = router.list_all();
        assert_eq!(tools.len(), 2);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"read_multiple_files"));
    }

    #[tokio::test]
    async fn read_file_entire_small() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(
            dir.path().join("test.txt"),
            "line one\nline two\nline three",
        )
        .unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_file(Parameters(ReadFileParams {
                path: dir.path().join("test.txt").to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("Lines 1-3 of 3 total"));
        assert!(output.contains("line one"));
        assert!(output.contains("line two"));
        assert!(output.contains("line three"));
    }

    #[tokio::test]
    async fn read_file_with_offset_and_limit() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(
            dir.path().join("test.txt"),
            "line0\nline1\nline2\nline3\nline4",
        )
        .unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_file(Parameters(ReadFileParams {
                path: dir.path().join("test.txt").to_string_lossy().to_string(),
                offset: Some(1),
                limit: Some(2),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("Lines 2-3 of 5 total"));
        assert!(output.contains("line1"));
        assert!(output.contains("line2"));
        assert!(!output.contains("line0"));
        assert!(!output.contains("line3"));
    }

    #[tokio::test]
    async fn read_file_with_limit_only() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("test.txt"), "a\nb\nc\nd").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_file(Parameters(ReadFileParams {
                path: dir.path().join("test.txt").to_string_lossy().to_string(),
                offset: None,
                limit: Some(2),
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("Lines 1-2 of 4 total"));
        assert!(output.contains("a\nb"));
        assert!(!output.contains("\nc"));
    }

    #[tokio::test]
    async fn read_file_too_large() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("big.txt"), "x".repeat(200)).unwrap();

        let service = make_service_with_max(vec![canon], 100);
        let result = service
            .read_file(Parameters(ReadFileParams {
                path: dir.path().join("big.txt").to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("File too large"));
    }

    #[tokio::test]
    async fn read_file_too_large_bypassed_with_range() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("big.txt"), "line1\nline2\nline3").unwrap();

        let service = make_service_with_max(vec![canon], 5);
        let result = service
            .read_file(Parameters(ReadFileParams {
                path: dir.path().join("big.txt").to_string_lossy().to_string(),
                offset: Some(0),
                limit: Some(1),
            }))
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().contains("line1"));
    }

    #[tokio::test]
    async fn read_file_binary_detected() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("binary.bin"), b"hello\x00world").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_file(Parameters(ReadFileParams {
                path: dir.path().join("binary.bin").to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Binary file"));
    }

    #[tokio::test]
    async fn read_file_empty() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("empty.txt"), "").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_file(Parameters(ReadFileParams {
                path: dir.path().join("empty.txt").to_string_lossy().to_string(),
                offset: None,
                limit: None,
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("(empty file)"));
        assert!(output.contains("0 B"));
    }

    #[tokio::test]
    async fn read_file_offset_beyond_end() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("test.txt"), "one\ntwo").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_file(Parameters(ReadFileParams {
                path: dir.path().join("test.txt").to_string_lossy().to_string(),
                offset: Some(10),
                limit: None,
            }))
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("beyond end of file"));
    }

    #[tokio::test]
    async fn read_file_denied_outside() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let service = make_service(vec![canon]);

        let other = TempDir::new().unwrap();
        std::fs::write(other.path().join("secret.txt"), "secret").unwrap();
        let result = service
            .read_file(Parameters(ReadFileParams {
                path: other
                    .path()
                    .join("secret.txt")
                    .to_string_lossy()
                    .to_string(),
                offset: None,
                limit: None,
            }))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Access denied"));
    }

    #[tokio::test]
    async fn read_multiple_files_all_valid() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("a.txt"), "alpha").unwrap();
        std::fs::write(dir.path().join("b.txt"), "bravo\ncharlie").unwrap();
        std::fs::write(dir.path().join("c.txt"), "delta").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_multiple_files(Parameters(ReadMultipleFilesParams {
                paths: vec![
                    dir.path().join("a.txt").to_string_lossy().to_string(),
                    dir.path().join("b.txt").to_string_lossy().to_string(),
                    dir.path().join("c.txt").to_string_lossy().to_string(),
                ],
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("a.txt"));
        assert!(output.contains("1 lines, 5 B"));
        assert!(output.contains("alpha"));
        assert!(output.contains("b.txt"));
        assert!(output.contains("2 lines,"));
        assert!(output.contains("bravo"));
        assert!(output.contains("c.txt"));
        assert!(output.contains("delta"));
        // Verify separator format
        assert!(output.contains("=== "));
        assert!(output.contains(" ==="));
    }

    #[tokio::test]
    async fn read_multiple_files_one_invalid_continues() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("good.txt"), "hello").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_multiple_files(Parameters(ReadMultipleFilesParams {
                paths: vec![
                    dir.path().join("good.txt").to_string_lossy().to_string(),
                    dir.path().join("missing.txt").to_string_lossy().to_string(),
                ],
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("hello"));
        assert!(output.contains("Error:"));
        assert!(output.contains("missing.txt"));
    }

    #[tokio::test]
    async fn read_multiple_files_outside_allowed_inline_error() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("ok.txt"), "fine").unwrap();

        let other = TempDir::new().unwrap();
        std::fs::write(other.path().join("secret.txt"), "secret").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_multiple_files(Parameters(ReadMultipleFilesParams {
                paths: vec![
                    dir.path().join("ok.txt").to_string_lossy().to_string(),
                    other
                        .path()
                        .join("secret.txt")
                        .to_string_lossy()
                        .to_string(),
                ],
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("fine"));
        assert!(output.contains("Error:"));
        assert!(output.contains("Access denied"));
    }

    #[tokio::test]
    async fn read_multiple_files_binary_inline_error() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        std::fs::write(dir.path().join("text.txt"), "readable").unwrap();
        std::fs::write(dir.path().join("binary.bin"), b"hello\x00world").unwrap();

        let service = make_service(vec![canon]);
        let result = service
            .read_multiple_files(Parameters(ReadMultipleFilesParams {
                paths: vec![
                    dir.path().join("text.txt").to_string_lossy().to_string(),
                    dir.path().join("binary.bin").to_string_lossy().to_string(),
                ],
            }))
            .await;

        let output = result.unwrap();
        assert!(output.contains("readable"));
        assert!(output.contains("Error:"));
        assert!(output.contains("Binary file"));
    }
}
