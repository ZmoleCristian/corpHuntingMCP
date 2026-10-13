#![deny(unused_must_use)]
#![deny(for_loops_over_fallibles)]
#![deny(dead_code)]
#![deny(unused_variables)]
#![deny(unused_assignments)]

mod ats;
mod cache;
mod config;
mod error;
mod filter;
mod handle;
mod http;
mod mcp;
mod model;
mod place;
mod tenants;
mod text;

use rmcp::ServiceExt;

use crate::config::Config;
use crate::error::AppError;
use crate::mcp::HuntServer;

const USAGE: &str = "corphuntingmcp — reverse head-hunting over the ATS boards, stdio transport.

Takes no arguments: it speaks the Model Context Protocol on stdin/stdout and is
meant to be spawned by an MCP client, not run by hand.

  -V, --version    print version and exit
  -h, --help       print this help and exit

Register it with Claude Code:
  claude mcp add corphuntingmcp -- corphuntingmcp

Tools: search_jobs(title|term|company, ...), search_jobs_async(same, to a file),
       get_job(id), list_companies(needle).
Async results land in $CORPHUNT_OUT, defaulting to a corphuntingmcp/ folder
inside the platform temp directory.
Logging: set RUST_LOG (e.g. RUST_LOG=debug), output goes to stderr.";

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let config = Config::new();
    let args: Vec<String> = std::env::args().skip(1).collect();
    for arg in &args {
        if arg == "-V" || arg == "--version" {
            println!("{} {}", config.name, config.version);
            return Ok(());
        }
        if arg == "-h" || arg == "--help" {
            println!("{USAGE}");
            return Ok(());
        }
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let server = HuntServer::new(config)?;
    let service = server
        .serve((tokio::io::stdin(), tokio::io::stdout()))
        .await
        .map_err(rmcp::RmcpError::from)?;
    service.waiting().await.map_err(rmcp::RmcpError::from)?;
    Ok(())
}
