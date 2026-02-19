use crate::FilesystemService;
use rmcp::ServerHandler;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};

#[rmcp::tool_handler]
impl ServerHandler for FilesystemService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities {
                tools: Some(Default::default()),
                ..Default::default()
            },
            server_info: Implementation {
                name: "ironbeard-mcp-filesystem".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
            instructions: Some(
                "Secure filesystem access server. Use list_allowed_directories to see available paths.".to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::FilesystemService;
    use rmcp::ServerHandler;
    use tempfile::TempDir;

    fn make_service() -> (TempDir, FilesystemService) {
        let dir = TempDir::new().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        let config = crate::Config {
            allowed_directories: vec![canon],
            allow_write: false,
            allow_destructive: false,
            max_read_size: 10_485_760,
            max_depth: 10,
        };
        (dir, FilesystemService::new(config))
    }

    #[test]
    fn server_info_has_correct_name() {
        let (_dir, service) = make_service();
        let info = service.get_info();
        assert_eq!(info.server_info.name, "ironbeard-mcp-filesystem");
    }

    #[test]
    fn server_info_has_tools_capability() {
        let (_dir, service) = make_service();
        let info = service.get_info();
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_none());
        assert!(info.capabilities.prompts.is_none());
    }

    #[test]
    fn server_info_version_matches_cargo() {
        let (_dir, service) = make_service();
        let info = service.get_info();
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }
}
