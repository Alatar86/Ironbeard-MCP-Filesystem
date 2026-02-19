# ironbeard-mcp-filesystem

A secure filesystem MCP server written in Rust. Provides 10 tools for file operations with strict path sandboxing.

## Features

- **7 read-only tools** — always available
- **3 write tools** — gated behind `--allow-write` flag
- **Path sandboxing** — only operates within explicitly allowed directories
- **Symlink escape prevention** — symlinks resolving outside allowed dirs are blocked
- **Binary file detection** — null-byte scanning in first 8KB
- **Large file handling** — configurable size limits with offset/limit support
- **Large directory safety** — results truncated at 1000 entries

## Installation

```bash
git clone <repo-url>
cd ironbeard-mcp-filesystem
cargo build --release
```

The binary will be at `target/release/ironbeard-mcp-filesystem` (or `.exe` on Windows).

## Usage

### Claude Desktop

Add to your Claude Desktop configuration (`claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "/path/to/ironbeard-mcp-filesystem",
      "args": ["/path/to/allowed/dir1", "/path/to/allowed/dir2"]
    }
  }
}
```

### With Write Access

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "/path/to/ironbeard-mcp-filesystem",
      "args": ["--allow-write", "/path/to/project"]
    }
  }
}
```

### CLI Arguments

```
ironbeard-mcp-filesystem [OPTIONS] <DIRECTORIES>...
```

## Tools

### Read-Only Tools (always available)

| Tool | Description | Parameters |
|------|-------------|------------|
| `list_allowed_directories` | Lists configured allowed directories | _(none)_ |
| `list_directory` | Lists directory contents with types and sizes | `path` |
| `read_file` | Reads file content with optional line range | `path`, `offset?`, `limit?` |
| `read_multiple_files` | Reads multiple files with inline error handling | `paths[]` |
| `get_file_info` | Gets file metadata (type, size, MIME, timestamps) | `path` |
| `directory_tree` | Shows visual directory tree with box-drawing chars | `path`, `max_depth?` |
| `search_files` | Searches for files matching a glob pattern | `path`, `pattern`, `max_results?` |

### Write Tools (require `--allow-write`)

| Tool | Description | Parameters |
|------|-------------|------------|
| `edit_file` | Applies exact-text replacements, returns unified diff | `path`, `edits[]` |
| `write_file` | Creates or overwrites a file | `path`, `content` |
| `create_directory` | Creates directory and parents (like `mkdir -p`) | `path` |

## Configuration

| Flag | Default | Description |
|------|---------|-------------|
| `--allow-write` | `false` | Enable write operations (edit, write, create) |
| `--max-read-size` | `10485760` (10 MB) | Maximum file size for read operations (bytes) |
| `--max-depth` | `10` | Maximum directory traversal depth |

## Security Model

All file operations are sandboxed to explicitly allowed directories:

- **Path validation** — every path is canonicalized and checked against the allowlist before any I/O
- **Symlink resolution** — symlinks are resolved to their real target; escapes outside allowed dirs are blocked
- **Traversal prevention** — `../` path components are neutralized via canonicalization
- **Write gating** — write tools are only registered when `--allow-write` is passed; they don't appear in tool listings otherwise
- **Binary detection** — `read_file` scans the first 8KB for null bytes and rejects binary files
- **Size limits** — large files are rejected unless offset/limit narrows the read

## Development

```bash
# Run tests
cargo test

# Format check
cargo fmt --check

# Lint
cargo clippy -- -D warnings

# Release build
cargo build --release
```

## License

MIT
