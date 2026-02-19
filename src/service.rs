use crate::config::Config;
use crate::security::SecurityContext;
use rmcp::handler::server::router::tool::ToolRouter;

pub struct FilesystemService {
    pub config: Config,
    pub security: SecurityContext,
    pub(crate) tool_router: ToolRouter<FilesystemService>,
}

impl FilesystemService {
    pub fn new(config: Config) -> Self {
        let security = SecurityContext::new(config.allowed_directories.clone());
        let mut tool_router = Self::list_tools_router()
            + Self::read_tools_router()
            + Self::info_tools_router()
            + Self::search_tools_router();
        if config.allow_write {
            tool_router += Self::write_tools_router();
        }
        if config.allow_destructive {
            tool_router += Self::destructive_tools_router();
        }
        Self {
            config,
            security,
            tool_router,
        }
    }
}
