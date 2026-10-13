use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub name: &'static str,
    pub title: &'static str,
    pub version: &'static str,
    pub user_agent: &'static str,
    pub http_timeout: Duration,
    pub concurrency: usize,
    pub batch_pause: Duration,
    pub board_ttl: Duration,
    pub hit_cap: usize,
    pub per_tenant_cap: usize,
}

impl Config {
    pub fn new() -> Config {
        Config {
            name: "corphuntingmcp",
            title: "corpHuntingMCP — hunt the corporations back",
            version: env!("CARGO_PKG_VERSION"),
            user_agent: "Mozilla/5.0 (X11; Linux x86_64; rv:120.0) Gecko/20100101 Firefox/120.0",
            http_timeout: Duration::from_secs(15),
            concurrency: 4,
            batch_pause: Duration::from_millis(1000),
            board_ttl: Duration::from_secs(600),
            hit_cap: 200,
            per_tenant_cap: 60,
        }
    }
}

pub fn search_limit() -> usize {
    40
}

pub fn default_reach() -> usize {
    50
}

pub fn roster_cap() -> usize {
    60
}

pub fn out_dir() -> PathBuf {
    match std::env::var_os("CORPHUNT_OUT") {
        Some(raw) => PathBuf::from(raw),
        None => std::env::temp_dir().join("corphuntingmcp"),
    }
}

pub fn result_file(stamp: u128) -> PathBuf {
    out_dir().join(format!("sweep-{stamp}.json"))
}

pub fn stamp() -> u128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_nanos(),
        Err(drift) => {
            tracing::warn!(error = %drift, "clock sits behind the unix epoch");
            drift.duration().as_nanos()
        }
    }
}
