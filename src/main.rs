mod mcp;
mod subtitle;

use rmcp::{ServiceExt, transport::stdio};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = mcp::SubtitleServer::default().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
