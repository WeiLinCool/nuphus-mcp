//! nuphus-mcp binary entrypoint — stdio JSON-RPC transport layer.
//!
//! Protocol: one JSON line over stdin/stdout (matches the stdio client in `src/mcp/client.rs`).
//! stdout only carries JSON-RPC response lines; all logs go to stderr.
//!
//! Security option: `--confirm-write` (or env var `NUPHUS_MCP_CONFIRM_WRITE=1`)
//! enables strict confirmation mode — write tools require an explicit `"confirm": true` argument.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

use nuphus_mcp::security::SecurityPolicy;
use nuphus_mcp::server::McpServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nuphus_mcp=info,warn".into()),
        )
        .with_writer(std::io::stderr) // stdout is reserved for the JSON-RPC protocol
        .init();

    // --confirm-write CLI flag overrides the environment variable
    let cli_confirm = std::env::args().any(|a| a == "--confirm-write");
    let policy = if cli_confirm {
        SecurityPolicy {
            strict_confirm: true,
        }
    } else {
        SecurityPolicy::from_env()
    };
    if policy.strict_confirm {
        tracing::warn!("[nuphus-mcp] STRICT CONFIRM MODE: write tools require \"confirm\": true");
    }

    let mut server = McpServer::with_policy(policy);
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let stdout = tokio::io::stdout();
    let mut writer = BufWriter::new(stdout);

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break; // EOF (client closed the pipe / process ended)
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(trimmed).await {
            writer.write_all(response.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
    }

    tracing::info!("[nuphus-mcp] stdin EOF, shutting down");
    Ok(())
}