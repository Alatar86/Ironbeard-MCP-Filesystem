use clap::Parser;
use std::path::PathBuf;

/// A secure filesystem MCP server with read-only and write-gated operations
#[derive(Parser, Debug, Clone)]
#[command(name = "ironbeard-mcp-filesystem")]
#[command(about = "A secure filesystem MCP server")]
pub struct Config {
    /// Directories to allow access to (must exist)
    #[arg(required = true)]
    pub allowed_directories: Vec<PathBuf>,

    /// Enable write operations (create, edit, move, delete)
    #[arg(long, default_value_t = false)]
    pub allow_write: bool,

    /// Enable destructive operations (delete, move). Implies --allow-write.
    #[arg(long, default_value_t = false)]
    pub allow_destructive: bool,

    /// Maximum file size for read operations in bytes
    #[arg(long, default_value_t = 10_485_760)]
    pub max_read_size: usize,

    /// Maximum directory traversal depth
    #[arg(long, default_value_t = 10)]
    pub max_depth: usize,
}

impl Config {
    /// Validates and canonicalizes all allowed directories.
    /// Returns a descriptive error string if any directory is invalid.
    pub fn validate(mut self) -> Result<Self, String> {
        if self.allow_destructive {
            self.allow_write = true;
        }
        let mut canonicalized = Vec::with_capacity(self.allowed_directories.len());
        for dir in &self.allowed_directories {
            let canon = dir
                .canonicalize()
                .map_err(|e| format!("Failed to resolve directory '{}': {}", dir.display(), e))?;
            if !canon.is_dir() {
                return Err(format!("'{}' is not a directory", dir.display()));
            }
            canonicalized.push(canon);
        }
        self.allowed_directories = canonicalized;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use tempfile::TempDir;

    /// Helper to parse Config from an argument list (mimicking CLI invocation).
    fn parse(args: &[&str]) -> Result<Config, clap::Error> {
        Config::try_parse_from(args)
    }

    #[test]
    fn parses_single_directory_with_defaults() {
        let dir = TempDir::new().unwrap();
        let dir_str = dir.path().to_str().unwrap();
        let config = parse(&["ironbeard", dir_str]).unwrap();
        assert_eq!(config.allowed_directories.len(), 1);
        assert!(!config.allow_write);
        assert_eq!(config.max_read_size, 10_485_760);
        assert_eq!(config.max_depth, 10);
    }

    #[test]
    fn parses_multiple_directories_and_flags() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let d1 = dir1.path().to_str().unwrap();
        let d2 = dir2.path().to_str().unwrap();
        let config = parse(&[
            "ironbeard",
            d1,
            d2,
            "--allow-write",
            "--max-read-size",
            "2048",
            "--max-depth",
            "5",
        ])
        .unwrap();
        assert_eq!(config.allowed_directories.len(), 2);
        assert!(config.allow_write);
        assert_eq!(config.max_read_size, 2048);
        assert_eq!(config.max_depth, 5);
    }

    #[test]
    fn requires_at_least_one_directory() {
        let result = parse(&["ironbeard"]);
        assert!(result.is_err());
    }

    #[test]
    fn validate_rejects_nonexistent_directory() {
        let config = Config {
            allowed_directories: vec![PathBuf::from("/definitely/does/not/exist/abc123")],
            allow_write: false,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to resolve"));
    }

    #[test]
    fn validate_rejects_file_as_directory() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("not_a_dir.txt");
        std::fs::write(&file_path, "data").unwrap();
        let config = Config {
            allowed_directories: vec![file_path],
            allow_write: false,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a directory"));
    }

    #[test]
    fn validate_canonicalizes_directories() {
        let dir = TempDir::new().unwrap();
        let expected = dir.path().canonicalize().unwrap();
        let config = Config {
            allowed_directories: vec![dir.path().to_path_buf()],
            allow_write: false,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        let validated = config.validate().unwrap();
        assert_eq!(validated.allowed_directories[0], expected);
    }

    #[test]
    fn parses_allow_destructive_flag() {
        let dir = TempDir::new().unwrap();
        let dir_str = dir.path().to_str().unwrap();
        let config = parse(&["ironbeard", dir_str, "--allow-destructive"]).unwrap();
        assert!(config.allow_destructive);
    }

    #[test]
    fn allow_destructive_defaults_to_false() {
        let dir = TempDir::new().unwrap();
        let dir_str = dir.path().to_str().unwrap();
        let config = parse(&["ironbeard", dir_str]).unwrap();
        assert!(!config.allow_destructive);
    }

    #[test]
    fn allow_destructive_auto_enables_allow_write() {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let config = Config {
            allowed_directories: vec![canon],
            allow_write: false,
            allow_destructive: true,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        let validated = config.validate().unwrap();
        assert!(validated.allow_write);
        assert!(validated.allow_destructive);
    }
}
