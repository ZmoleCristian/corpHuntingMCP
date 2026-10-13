use thiserror::Error;

#[derive(Debug, Error)]
pub enum AtsError {
    #[error("http request to {platform} tenant {tenant} failed: {source}")]
    Http {
        platform: &'static str,
        tenant: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{platform} tenant {tenant} answered with status {status} for {url}")]
    Status {
        platform: &'static str,
        tenant: String,
        status: u16,
        url: String,
    },
    #[error("{platform} response for tenant {tenant} could not be decoded: {source}")]
    Decode {
        platform: &'static str,
        tenant: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{platform} response for tenant {tenant} could not be parsed: {source}")]
    Parse {
        platform: &'static str,
        tenant: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("{platform} posting under tenant {tenant} is missing field {field}")]
    Malformed {
        platform: &'static str,
        tenant: String,
        field: &'static str,
    },
    #[error("{platform} has no tenant {tenant} in the built-in table")]
    NoSuchTenant { platform: &'static str, tenant: String },
    #[error("{platform} tenant {tenant} has no job {job}")]
    NoSuchJob {
        platform: &'static str,
        tenant: String,
        job: String,
    },
}

#[derive(Debug, Error)]
pub enum HandleError {
    #[error("job handle {handle} is not valid base64: {source}")]
    Encoding {
        handle: String,
        #[source]
        source: base64::DecodeError,
    },
    #[error("job handle {handle} decodes to bytes that are not utf-8: {source}")]
    Utf8 {
        handle: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("job handle {handle} does not carry the three expected fields")]
    Shape { handle: String },
    #[error("job handle {handle} names an unknown ats platform: {source}")]
    Platform {
        handle: String,
        #[source]
        source: QueryError,
    },
}

#[derive(Debug, Error)]
pub enum QueryError {
    #[error("search needs at least one of title, term or company")]
    NoNeedle,
    #[error("unknown ats platform {platform}")]
    Platform { platform: String },
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("http client could not be built: {0}")]
    Http(#[from] reqwest::Error),
    #[error("mcp server failed: {0}")]
    Mcp(#[from] rmcp::RmcpError),
}
