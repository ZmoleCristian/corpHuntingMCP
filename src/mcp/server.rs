use std::path::Path;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::ats;
use crate::cache::BoardCache;
use crate::config::{self, Config};
use crate::filter::{self, Needles, Query};
use crate::handle;
use crate::model::{Ats, Sweep};
use crate::tenants::TABLES;

const INSTRUCTIONS: &str = "Reverse head-hunting: you hunt the companies. search_jobs sweeps \
    every tenant of ten ATS platforms (greenhouse, lever, ashby, workable, recruitee, \
    smartrecruiters, workday, bamboohr, teamtailor, jobvite) in parallel; get_job(id) fetches one \
    posting fresh; list_companies says who is covered. There is no central index of job boards, so \
    a search asks employers one by one: reach caps how many per platform (default 50, 0 for all of \
    them), boards are cached for ten minutes, and naming a company lifts the cap for that \
    employer. A cold sweep costs roughly a second per employer per platform and most clients \
    abandon a call at two minutes, so keep reach under about 80 unless you also narrow with ats. A \
    search that times out still finishes and fills the cache, so repeating it is cheap; \
    search_jobs_async escapes the ceiling entirely by writing the full result to a file. Read a \
    default search as a sample, not an answer: it touches a small, unranked slice of the table, so \
    to trust it name a company, or narrow with ats and raise reach. Three limits are worth \
    knowing. term only reaches the posting body on boards that publish it in their listing \
    (greenhouse, lever, ashby, recruitee, teamtailor, workable) and falls back to the title alone \
    on the rest (smartrecruiters, workday, bamboohr, jobvite). posted_within_days drops every hit \
    from a board that publishes no date, which today means all of bamboohr and jobvite. Undated \
    hits sort last, so truncation cuts them first.";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchJobsArgs {
    pub title: Option<String>,
    pub term: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub country: Option<String>,
    pub remote: Option<bool>,
    pub company: Option<String>,
    pub ats: Option<Vec<String>>,
    pub posted_within_days: Option<u32>,
    #[serde(default = "config::search_limit")]
    pub limit: usize,
    #[serde(default = "config::default_reach")]
    pub reach: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetJobArgs {
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListCompaniesArgs {
    pub needle: Option<String>,
    #[serde(default = "config::roster_cap")]
    pub limit: usize,
}

#[derive(Clone)]
pub struct HuntServer {
    tool_router: ToolRouter<Self>,
    http: reqwest::Client,
    config: Config,
    boards: BoardCache,
}

impl HuntServer {
    pub fn new(config: Config) -> Result<HuntServer, reqwest::Error> {
        let http = reqwest::Client::builder()
            .user_agent(config.user_agent)
            .timeout(config.http_timeout)
            .build()?;
        Ok(HuntServer {
            tool_router: HuntServer::tool_router(),
            http,
            config,
            boards: BoardCache::new(config.board_ttl),
        })
    }
}

#[tool_router(router = tool_router)]
impl HuntServer {
    #[tool(
        name = "search_jobs",
        description = "Search live job postings across every tenant of ten ATS platforms at once. Give at least one of title (matched against the job title), term (a list of words or phrases, all of which must appear in the title or posting body) or company (matched against the employer name, and enough on its own to list everything that employer has open). A one-word term matches whole words, so rust does not match trust; an entry with spaces matches as a phrase. exclude is the mirror of term: any hit naming one of its entries in the title, body or location is dropped. Narrow further with country (ISO-2 like RO, DE, US), remote, ats and posted_within_days; remote=true beside a country widens to \"in that country or remote\". Every hit carries an opaque id for get_job. The server instructions cover reach, caching and what coverage actually means."
    )]
    async fn search_jobs(
        &self,
        p: Parameters<SearchJobsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = query_of(p.0)?;
        let sweep = ats::search_all(&self.http, &query, &self.config, &self.boards).await;
        let returned = sweep.hits.len();
        let summary = summarise(&sweep);
        Ok(CallToolResult {
            content: vec![Content::text(summary)],
            structured_content: Some(json!({
                "found": sweep.found,
                "returned": returned,
                "employers_searched": sweep.scanned,
                "employers_known": sweep.known,
                "hits": sweep.hits,
            })),
            is_error: None,
            meta: None,
        })
    }

    #[tool(
        name = "search_jobs_async",
        description = "Same arguments and same semantics as search_jobs, but it returns \
        immediately with a file path and runs the sweep in the background. The file appears at \
        that path when the sweep finishes, holding the full result as JSON. Use it when a search \
        would outlive the client timeout: reach set high or to 0, no ats narrowing, or a company \
        with many boards. Nothing is returned inline, so read the file to see the hits. The sweep \
        also fills the shared cache, so a later search_jobs over the same ground is instant."
    )]
    async fn search_jobs_async(
        &self,
        p: Parameters<SearchJobsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let query = query_of(p.0)?;
        let path = config::result_file(config::stamp());
        let target = path.clone();
        let http = self.http.clone();
        let config = self.config;
        let boards = self.boards.clone();
        tokio::spawn(async move {
            let sweep = ats::search_all(&http, &query, &config, &boards).await;
            match record(&target, &sweep) {
                Ok(()) => tracing::info!(
                    found = sweep.found,
                    scanned = sweep.scanned,
                    path = %target.display(),
                    "async sweep written"
                ),
                Err(e) => tracing::warn!(error = %e, "async sweep could not be written"),
            }
        });
        let where_at = path.display().to_string();
        Ok(CallToolResult {
            content: vec![Content::text(format!(
                "sweep started in the background; the full result will appear at {where_at}"
            ))],
            structured_content: Some(json!({ "path": where_at })),
            is_error: None,
            meta: None,
        })
    }

    #[tool(
        name = "get_job",
        description = "Fetch the full posting for an id returned by search_jobs: description text, department, employment type and compensation when the board publishes them. Always fetched fresh."
    )]
    async fn get_job(&self, p: Parameters<GetJobArgs>) -> Result<CallToolResult, ErrorData> {
        let target = match handle::decode(&p.0.id) {
            Ok(target) => target,
            Err(e) => return Err(ErrorData::invalid_params(e.to_string(), None)),
        };
        let job = match ats::detail(&self.http, &target).await {
            Ok(job) => job,
            Err(e) => return Err(ErrorData::internal_error(e.to_string(), None)),
        };
        let rendered = format!(
            "{} | {} | {}\n{}\n\n{}",
            job.company,
            job.title,
            job.url,
            place_line(&job.location),
            job.description
        );
        Ok(CallToolResult {
            content: vec![Content::text(rendered)],
            structured_content: Some(json!(job)),
            is_error: None,
            meta: None,
        })
    }

    #[tool(
        name = "list_companies",
        description = "List the employers this server can reach, optionally narrowed by a needle matched against company name and ATS tenant slug. Coverage is a fixed built-in table, so this is how you learn whether an employer is reachable before searching."
    )]
    async fn list_companies(
        &self,
        p: Parameters<ListCompaniesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut lines: Vec<String> = Vec::new();
        let mut matched: usize = 0;
        for table in TABLES {
            for tenant in table {
                if !named(&p.0.needle, tenant.company, tenant.slug) {
                    continue;
                }
                matched += 1;
                if lines.len() < p.0.limit {
                    lines.push(format!(
                        "{} | {} | {}",
                        tenant.company,
                        tenant.ats.label(),
                        tenant.slug
                    ));
                }
            }
        }
        let shown = lines.len();
        let mut rendered = lines.join("\n");
        if matched > shown {
            rendered = format!(
                "showing {} of {} employers — give a needle to narrow, or raise limit\n{}",
                shown, matched, rendered
            );
        }
        Ok(CallToolResult {
            content: vec![Content::text(rendered)],
            structured_content: Some(
                json!({ "matched": matched, "shown": shown, "companies": lines }),
            ),
            is_error: None,
            meta: None,
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for HuntServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: self.config.name.into(),
                title: Some(self.config.title.into()),
                version: self.config.version.into(),
                website_url: None,
                icons: None,
            },
            instructions: Some(INSTRUCTIONS.into()),
        }
    }
}

fn query_of(args: SearchJobsArgs) -> Result<Query, ErrorData> {
    let needles = Needles {
        title: args.title.clone(),
        term: args.term.clone(),
        company: args.company.clone(),
    };
    let needle = match filter::needle_of(&needles) {
        Ok(needle) => needle,
        Err(e) => return Err(ErrorData::invalid_params(e.to_string(), None)),
    };
    let platforms = platforms_of(&args.ats)?;
    Ok(Query {
        needle,
        title: args.title,
        term: args.term,
        exclude: args.exclude,
        country: args.country,
        remote: args.remote,
        company: args.company,
        platforms,
        fresher_than: freshness_of(args.posted_within_days),
        limit: args.limit,
        reach: args.reach,
    })
}

fn platforms_of(names: &Option<Vec<String>>) -> Result<Vec<Ats>, ErrorData> {
    let mut wanted: Vec<Ats> = Vec::new();
    let Some(list) = names else {
        return Ok(wanted);
    };
    for name in list {
        match Ats::parse(name) {
            Ok(found) => wanted.push(found),
            Err(e) => return Err(ErrorData::invalid_params(e.to_string(), None)),
        }
    }
    Ok(wanted)
}

fn freshness_of(days: Option<u32>) -> Option<String> {
    let Some(found) = days else {
        return None;
    };
    Some(filter::cutoff(found))
}

fn named(needle: &Option<String>, company: &str, slug: &str) -> bool {
    let Some(wanted) = needle else {
        return true;
    };
    let lowered = wanted.to_lowercase();
    if company.to_lowercase().contains(&lowered) {
        return true;
    }
    slug.to_lowercase().contains(&lowered)
}

fn place_line(location: &Option<String>) -> String {
    let Some(found) = location else {
        return String::new();
    };
    found.clone()
}

fn summarise(sweep: &Sweep) -> String {
    let mut lines: Vec<String> = Vec::new();
    if sweep.scanned < sweep.known {
        lines.push(format!(
            "this is a sample: {} of the {} employers covered were asked, and which ones is not ranked by anything meaningful. For an answer you can trust, name a company, or narrow with ats and raise reach so the budget is spent where you care",
            sweep.scanned, sweep.known
        ));
    }
    if sweep.found > sweep.hits.len() {
        lines.push(format!(
            "showing {} of {} matches, newest first — postings whose board publishes no date sort last and are cut first, so narrow with country, company or ats rather than raising limit",
            sweep.hits.len(),
            sweep.found
        ));
    }
    for hit in &sweep.hits {
        lines.push(format!(
            "{} | {} | {} | {} | {} | {}",
            hit.id,
            hit.ats.label(),
            hit.company,
            hit.title,
            place_line(&hit.location),
            hit.url
        ));
    }
    lines.join("\n")
}

fn record(path: &Path, sweep: &Sweep) -> Result<(), std::io::Error> {
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other("sweep path has no parent directory"));
    };
    std::fs::create_dir_all(parent)?;
    let body = match serde_json::to_string_pretty(sweep) {
        Ok(text) => text,
        Err(e) => return Err(std::io::Error::other(e)),
    };
    std::fs::write(path, body)
}
