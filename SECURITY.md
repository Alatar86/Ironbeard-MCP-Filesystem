# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

## Security Model

Ironbeard MCP Filesystem is designed to give LLM agents controlled access to the local filesystem. Security is enforced through multiple layers:

### Path Sandboxing

All file operations are restricted to directories explicitly passed at startup. Paths are validated using OS-level `canonicalize()` — not string prefix matching — so the sandbox cannot be bypassed by crafted path strings.

### Symlink Escape Prevention

Symlinks are resolved to their real target before validation. A symlink pointing outside an allowed directory will be denied because its canonical path falls outside the sandbox.

### Traversal Prevention

`../` and `.` components are rejected in paths for file creation operations. For existing paths, `canonicalize()` resolves traversal naturally before the sandbox check runs.

### Permission Tiers

Tools are conditionally **registered** at startup based on CLI flags — they do not appear in the MCP tool listing at all unless the corresponding flag is set:

- **Read-only** (always available) — 7 tools for listing, reading, searching, and inspecting files.
- **Write** (`--allow-write`) — 3 additional tools for creating and editing files.
- **Destructive** (`--allow-destructive`, implies `--allow-write`) — 3 additional tools for deleting and moving files. `delete_directory` refuses non-empty directories.

### Additional Safeguards

- **Binary detection** — Files containing null bytes in the first 8 KB are rejected to prevent accidental binary file reads.
- **Size limits** — Full reads are capped at 10 MB by default (configurable via `--max-read-size`). Partial reads with `offset`/`limit` bypass this cap.
- **Result caps** — Directory listings, tree views, and search results are capped to prevent unbounded output.
- **Move validation** — `move_file` validates both source and destination independently against the allowlist.

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly via email:

**Email:** Ironbeardai@gmail.com

### What to include

- Description of the vulnerability
- Steps to reproduce
- Impact assessment (what an attacker could achieve)

### What qualifies as a vulnerability

- Sandbox escapes (accessing files outside allowed directories)
- Permission tier bypasses (performing write/destructive operations without the corresponding flag)
- Path traversal that circumvents validation
- Symlink-based escapes
- Denial of service through crafted inputs

### Response

- You will receive an acknowledgment within 48 hours.
- A fix will be prioritized based on severity.
- Credit will be given in the changelog unless you prefer otherwise.

Please **do not** open a public GitHub issue for security vulnerabilities.
