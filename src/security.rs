use crate::error::FsError;
use std::path::{Path, PathBuf};

pub struct SecurityContext {
    allowed_dirs: Vec<PathBuf>,
}

impl SecurityContext {
    /// Creates a new SecurityContext. All directories must already be canonicalized.
    pub fn new(allowed_dirs: Vec<PathBuf>) -> Self {
        Self { allowed_dirs }
    }

    /// Canonicalizes the input path and checks it falls within an allowed directory.
    /// Works for both existing and not-yet-existing paths (canonicalizes parent for new files).
    pub fn validate_path(&self, path: &Path) -> Result<PathBuf, FsError> {
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                // Path doesn't exist yet — canonicalize parent + append filename
                let parent = path.parent().ok_or_else(|| FsError::PathDenied {
                    path: path.display().to_string(),
                })?;
                let file_name = path.file_name().ok_or_else(|| FsError::PathDenied {
                    path: path.display().to_string(),
                })?;
                let canonical_parent = parent.canonicalize().map_err(|_| FsError::NotFound {
                    path: parent.display().to_string(),
                })?;
                canonical_parent.join(file_name)
            }
        };

        if self
            .allowed_dirs
            .iter()
            .any(|dir| canonical.starts_with(dir))
        {
            Ok(canonical)
        } else {
            Err(FsError::PathDenied {
                path: path.display().to_string(),
            })
        }
    }

    /// Validates the path is within allowed directories and currently exists on disk.
    pub fn validate_path_exists(&self, path: &Path) -> Result<PathBuf, FsError> {
        let canonical = self.validate_path(path)?;
        if canonical.exists() {
            Ok(canonical)
        } else {
            Err(FsError::NotFound {
                path: path.display().to_string(),
            })
        }
    }

    /// Validates the path is allowed, exists, and is a regular file.
    pub fn validate_file(&self, path: &Path) -> Result<PathBuf, FsError> {
        let canonical = self.validate_path_exists(path)?;
        if canonical.is_file() {
            Ok(canonical)
        } else {
            Err(FsError::NotAFile {
                path: path.display().to_string(),
            })
        }
    }

    /// Validates a path for `create_directory` (mkdir -p).
    ///
    /// Walks up the path tree to find the nearest existing ancestor, canonicalizes
    /// it, validates it's within allowed directories, and rejects `.` or `..` in
    /// the non-existent tail segments.
    pub fn validate_creatable_path(&self, path: &Path) -> Result<PathBuf, FsError> {
        // Reject . or .. in any component up-front (before OS normalizes them away)
        for component in path.components() {
            match component {
                std::path::Component::CurDir | std::path::Component::ParentDir => {
                    return Err(FsError::PathDenied {
                        path: path.display().to_string(),
                    });
                }
                _ => {}
            }
        }

        // Walk up to find the nearest existing ancestor
        let mut existing = path.to_path_buf();
        let mut tail_segments: Vec<std::ffi::OsString> = Vec::new();

        while !existing.exists() {
            match existing.parent() {
                Some(parent) => {
                    if let Some(seg) = existing.file_name() {
                        tail_segments.push(seg.to_os_string());
                    } else {
                        return Err(FsError::PathDenied {
                            path: path.display().to_string(),
                        });
                    }
                    existing = parent.to_path_buf();
                }
                None => {
                    return Err(FsError::NotFound {
                        path: path.display().to_string(),
                    });
                }
            }
        }

        // Canonicalize the existing ancestor
        let canonical_base = existing.canonicalize().map_err(|_| FsError::NotFound {
            path: existing.display().to_string(),
        })?;

        // Validate the existing ancestor is within allowed dirs
        if !self
            .allowed_dirs
            .iter()
            .any(|dir| canonical_base.starts_with(dir))
        {
            return Err(FsError::PathDenied {
                path: path.display().to_string(),
            });
        }

        // Reconstruct the full canonical path
        let mut result = canonical_base;
        for seg in tail_segments.into_iter().rev() {
            result = result.join(seg);
        }

        Ok(result)
    }

    /// Validates the path is allowed, exists, and is a directory.
    pub fn validate_directory(&self, path: &Path) -> Result<PathBuf, FsError> {
        let canonical = self.validate_path_exists(path)?;
        if canonical.is_dir() {
            Ok(canonical)
        } else {
            Err(FsError::NotADirectory {
                path: path.display().to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, SecurityContext) {
        let dir = TempDir::new().unwrap();
        let canonical = dir.path().canonicalize().unwrap();
        let ctx = SecurityContext::new(vec![canonical]);
        (dir, ctx)
    }

    #[test]
    fn allows_path_inside_allowed_dir() {
        let (dir, ctx) = setup();
        fs::write(dir.path().join("hello.txt"), "hi").unwrap();
        let result = ctx.validate_path(&dir.path().join("hello.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn denies_path_outside_allowed_dir() {
        let (_dir, ctx) = setup();
        // Use a well-known directory that definitely isn't inside our temp dir
        let outside = std::env::temp_dir();
        // Create a file we know exists outside the allowed dir
        let outside_file = outside.join("ironbeard_test_outside.tmp");
        fs::write(&outside_file, "x").unwrap();
        let result = ctx.validate_path(&outside_file);
        let _ = fs::remove_file(&outside_file);
        assert!(matches!(result, Err(FsError::PathDenied { .. })));
    }

    #[test]
    fn denies_dotdot_traversal_escaping_allowed_dir() {
        let (dir, ctx) = setup();
        // Create a subdir so ../.. resolves to parent of the temp dir
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        let escape_path = sub.join("..").join("..").join("..");
        let result = ctx.validate_path(&escape_path);
        assert!(matches!(result, Err(FsError::PathDenied { .. })));
    }

    #[test]
    fn denies_symlink_resolving_outside() {
        let (dir, ctx) = setup();
        let outside_dir = TempDir::new().unwrap();
        let outside_file = outside_dir.path().join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        let link_path = dir.path().join("sneaky_link");

        // Attempt to create symlink — may fail on Windows without developer mode
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside_file, &link_path).unwrap();
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_file(&outside_file, &link_path).is_err() {
                // Skip test if symlink creation fails (requires elevated privileges)
                eprintln!("Skipping symlink test: insufficient privileges");
                return;
            }
        }

        let result = ctx.validate_path(&link_path);
        assert!(
            matches!(result, Err(FsError::PathDenied { .. })),
            "Symlink resolving outside allowed dir must be denied"
        );
    }

    #[test]
    fn validate_path_exists_succeeds_for_existing() {
        let (dir, ctx) = setup();
        fs::write(dir.path().join("exists.txt"), "data").unwrap();
        assert!(
            ctx.validate_path_exists(&dir.path().join("exists.txt"))
                .is_ok()
        );
    }

    #[test]
    fn validate_path_exists_fails_for_missing() {
        let (dir, ctx) = setup();
        let result = ctx.validate_path_exists(&dir.path().join("nonexistent.txt"));
        assert!(matches!(result, Err(FsError::NotFound { .. })));
    }

    #[test]
    fn validate_file_succeeds_for_file() {
        let (dir, ctx) = setup();
        fs::write(dir.path().join("file.txt"), "data").unwrap();
        assert!(ctx.validate_file(&dir.path().join("file.txt")).is_ok());
    }

    #[test]
    fn validate_file_fails_for_directory() {
        let (dir, ctx) = setup();
        let sub = dir.path().join("subdir");
        fs::create_dir(&sub).unwrap();
        let result = ctx.validate_file(&sub);
        assert!(matches!(result, Err(FsError::NotAFile { .. })));
    }

    #[test]
    fn validate_directory_succeeds_for_dir() {
        let (dir, ctx) = setup();
        let sub = dir.path().join("subdir");
        fs::create_dir(&sub).unwrap();
        assert!(ctx.validate_directory(&sub).is_ok());
    }

    #[test]
    fn validate_directory_fails_for_file() {
        let (dir, ctx) = setup();
        fs::write(dir.path().join("file.txt"), "data").unwrap();
        let result = ctx.validate_directory(&dir.path().join("file.txt"));
        assert!(matches!(result, Err(FsError::NotADirectory { .. })));
    }

    #[test]
    fn validate_path_works_for_nonexistent_file_in_allowed_dir() {
        let (dir, ctx) = setup();
        // Path doesn't exist, but parent does and is allowed
        let result = ctx.validate_path(&dir.path().join("new_file.txt"));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_creatable_path_single_new_dir() {
        let (dir, ctx) = setup();
        let result = ctx.validate_creatable_path(&dir.path().join("newdir"));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_creatable_path_deeply_nested() {
        let (dir, ctx) = setup();
        let result = ctx.validate_creatable_path(&dir.path().join("a").join("b").join("c"));
        assert!(result.is_ok());
        let canon = result.unwrap();
        assert!(
            canon.ends_with(std::path::Path::new("a/b/c"))
                || canon.ends_with(std::path::Path::new("a\\b\\c"))
        );
    }

    #[test]
    fn validate_creatable_path_rejects_dotdot_in_tail() {
        let (dir, ctx) = setup();
        let sneaky = dir.path().join("a").join("..").join("escape");
        let result = ctx.validate_creatable_path(&sneaky);
        assert!(matches!(result, Err(FsError::PathDenied { .. })));
    }

    #[test]
    fn validate_creatable_path_rejects_outside_allowed() {
        let (_dir, ctx) = setup();
        let other = TempDir::new().unwrap();
        let result = ctx.validate_creatable_path(&other.path().join("newdir"));
        assert!(matches!(result, Err(FsError::PathDenied { .. })));
    }

    #[test]
    fn validate_creatable_path_existing_dir_ok() {
        let (dir, ctx) = setup();
        let sub = dir.path().join("existing");
        fs::create_dir(&sub).unwrap();
        let result = ctx.validate_creatable_path(&sub);
        assert!(result.is_ok());
    }

    #[test]
    fn trailing_slash_normalized() {
        let (dir, ctx) = setup();
        let sub = dir.path().join("subdir");
        fs::create_dir(&sub).unwrap();

        let with_slash = ctx.validate_path(&sub.join(""));
        let without_slash = ctx.validate_path(&sub);

        // Both should succeed and resolve to the same canonical path
        assert!(with_slash.is_ok());
        assert!(without_slash.is_ok());
        assert_eq!(with_slash.unwrap(), without_slash.unwrap());
    }
}
