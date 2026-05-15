use foia_search::FoiaSearchServer;
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("starting foia-search MCP server");

    let server = FoiaSearchServer::create()?;
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}
