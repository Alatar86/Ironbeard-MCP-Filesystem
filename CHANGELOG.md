# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-02-19

### Added

- **Security model** — Path sandboxing via OS-level canonicalization, symlink escape prevention, `../` traversal rejection, and binary file detection (null-byte scan on first 8 KB).
- **Configuration** — CLI with `--allow-write`, `--allow-destructive`, `--max-read-size`, and `--max-depth` flags. `--allow-destructive` implies `--allow-write`.
- **Error handling** — Structured `FsError` enum with MCP error code mapping (`INVALID_PARAMS`, `RESOURCE_NOT_FOUND`, `INTERNAL_ERROR`).
- **7 read-only tools** (always available):
  - `list_directory` — Lists directory contents sorted by type then name, with sizes and dates.
  - `read_file` — Reads file contents with optional offset/limit for line ranges.
  - `read_multiple_files` — Reads multiple files with inline error reporting per file.
  - `get_file_info` — Returns detailed metadata including MIME type, timestamps, and permissions.
  - `directory_tree` — Visual tree with box-drawing characters, configurable depth.
  - `search_files` — Glob-pattern file search with configurable result cap (default 50, max 200).
  - `list_allowed_directories` — Lists all directories the server is permitted to access.
- **3 write tools** (gated behind `--allow-write`):
  - `write_file` — Creates or overwrites a file with provided content.
  - `edit_file` — Applies exact-text replacements with uniqueness enforcement and unified diff output.
  - `create_directory` — Creates directories recursively (like `mkdir -p`).
- **3 destructive tools** (gated behind `--allow-destructive`):
  - `delete_file` — Deletes a single regular file.
  - `move_file` — Moves or renames a file/directory with double-path validation.
  - `delete_directory` — Deletes an empty directory only (no recursive delete).
- **MCP transport** — stdio-based server (protocol version `2024-11-05`) with tracing logs on stderr.

[0.1.0]: https://github.com/Alatar86/Ironbeard-MCP-Filesystem/releases/tag/v0.1.0
