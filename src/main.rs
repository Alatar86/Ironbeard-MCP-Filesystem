use clap::Parser;
use ironbeard_mcp_filesystem::{Config, FilesystemService};
use rmcp::ServiceExt;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing (logs to stderr so stdout stays clean for MCP protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::INFO)
        .init();

    let config = Config::parse().validate().unwrap_or_else(|e| {
        eprintln!("Configuration error: {e}");
        std::process::exit(1);
    });

    info!(
        "ironbeard-mcp-filesystem v{} starting",
        env!("CARGO_PKG_VERSION")
    );
    info!("Allowed directories: {:?}", config.allowed_directories);
    info!(
        "Write mode: {}",
        if config.allow_write {
            "enabled"
        } else {
            "disabled"
        }
    );
    info!(
        "Destructive mode: {}",
        if config.allow_destructive {
            "enabled"
        } else {
            "disabled"
        }
    );
    info!(
        "Max read size: {} bytes, Max depth: {}",
        config.max_read_size, config.max_depth
    );

    let service = FilesystemService::new(config);
    let server = service
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start MCP server: {e}"))?;

    info!("MCP server running on stdio");

    server
        .waiting()
        .await
        .map_err(|e| anyhow::anyhow!("Server error: {e}"))?;

    Ok(())
}
