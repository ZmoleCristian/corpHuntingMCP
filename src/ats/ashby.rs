use reqwest::Client;
use serde::Deserialize;
use serde_json::Number;

use crate::error::AtsError;
use crate::filter::{self, Query};
use crate::handle;
use crate::http;
use crate::model::{Country, JobDetail, JobHit, Tenant};
use crate::place;
use crate::text;

const PLATFORM: &str = "ashby";
const BASE: &str = "https://api.ashbyhq.com/posting-api/job-board";
const ISO_DATE_LEN: usize = 10;
const DASH_AT_YEAR: usize = 4;
const DASH_AT_MONTH: usize = 7;
const REMOTE: &str = "Remote";

#[derive(Debug, Deserialize)]
struct Board {
    jobs: Vec<Posting>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Posting {
    id: String,
    title: Option<String>,
    location: Option<String>,
    address: Option<Address>,
    secondary_locations: Option<Vec<Secondary>>,
    department: Option<String>,
    team: Option<String>,
    employment_type: Option<String>,
    published_at: Option<String>,
    is_listed: Option<bool>,
    is_remote: Option<bool>,
    job_url: Option<String>,
    apply_url: Option<String>,
    description_plain: Option<String>,
    description_html: Option<String>,
    compensation: Option<Compensation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Secondary {
    location: Option<String>,
    address: Option<Address>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Address {
    postal_address: Option<Postal>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Postal {
    address_country: Option<String>,
    address_region: Option<String>,
    address_locality: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Compensation {
    compensation_tier_summary: Option<String>,
    scrapeable_compensation_salary_summary: Option<String>,
    summary_components: Option<Vec<Component>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Component {
    compensation_type: Option<String>,
    currency_code: Option<String>,
    min_value: Option<Number>,
    max_value: Option<Number>,
}

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let board = board_of(client, tenant).await?;
    let mut hits: Vec<JobHit> = Vec::new();
    for posting in &board.jobs {
        if hidden(posting) {
            continue;
        }
        if !filter::coarse(&query.needle, &blob(posting)) {
            continue;
        }
        match to_hit(tenant, posting) {
            Ok(hit) => hits.push(hit),
            Err(e) => tracing::warn!(
                error = %e,
                platform = PLATFORM,
                tenant = tenant.slug,
                job = posting.id.as_str(),
                "posting skipped"
            ),
        }
    }
    Ok(hits)
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let board = board_of(client, tenant).await?;
    for posting in &board.jobs {
        if posting.id != job {
            continue;
        }
        return to_detail(tenant, posting);
    }
    Err(AtsError::NoSuchJob {
        platform: PLATFORM,
        tenant: tenant.slug.to_string(),
        job: job.to_string(),
    })
}

async fn board_of(client: &Client, tenant: &Tenant) -> Result<Board, AtsError> {
    let url = format!("{}/{}?includeCompensation=true", BASE, tenant.slug);
    http::get_json(client, PLATFORM, tenant.slug, &url).await
}

fn to_hit(tenant: &Tenant, posting: &Posting) -> Result<JobHit, AtsError> {
    let title = title_of(tenant, posting)?;
    let link = link_of(tenant, posting)?;
    Ok(JobHit {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &posting.id),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title,
        location: spot_of(posting),
        country: country_of(posting),
        url: link,
        posted: stamp_of(posting),
        text: body_of(posting),
    })
}

fn to_detail(tenant: &Tenant, posting: &Posting) -> Result<JobDetail, AtsError> {
    let title = title_of(tenant, posting)?;
    let link = link_of(tenant, posting)?;
    let Some(description) = body_of(posting) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "descriptionPlain",
        });
    };
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &posting.id),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title,
        location: spot_of(posting),
        country: country_of(posting),
        url: link,
        posted: stamp_of(posting),
        department: unit_of(posting),
        employment_type: trimmed(&posting.employment_type),
        compensation: pay_of(posting),
        description,
    })
}

fn hidden(posting: &Posting) -> bool {
    let Some(listed) = posting.is_listed else {
        return false;
    };
    !listed
}

fn blob(posting: &Posting) -> String {
    let mut parts: Vec<String> = Vec::new();
    push_text(&mut parts, &posting.title);
    push_text(&mut parts, &posting.department);
    push_text(&mut parts, &posting.team);
    push_text(&mut parts, &posting.location);
    push_text(&mut parts, &posting.description_plain);
    parts.join(" ")
}

fn title_of(tenant: &Tenant, posting: &Posting) -> Result<String, AtsError> {
    let Some(named) = trimmed(&posting.title) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "title",
        });
    };
    Ok(named)
}

fn link_of(tenant: &Tenant, posting: &Posting) -> Result<String, AtsError> {
    let Some(link) = trimmed(&posting.job_url) else {
        let Some(fallback) = trimmed(&posting.apply_url) else {
            return Err(AtsError::Malformed {
                platform: PLATFORM,
                tenant: tenant.slug.to_string(),
                field: "jobUrl",
            });
        };
        return Ok(fallback);
    };
    Ok(link)
}

fn body_of(posting: &Posting) -> Option<String> {
    let Some(plain) = trimmed(&posting.description_plain) else {
        return rendered_body(posting);
    };
    Some(plain)
}

fn rendered_body(posting: &Posting) -> Option<String> {
    let Some(raw) = trimmed(&posting.description_html) else {
        return None;
    };
    let rendered = text::plain(&raw);
    if rendered.trim().is_empty() {
        return None;
    }
    Some(rendered)
}

fn spot_of(posting: &Posting) -> Option<String> {
    let Some(here) = spot_at(&posting.location, &posting.address) else {
        let Some(there) = secondary_spot(posting) else {
            return remote_only(posting);
        };
        return Some(with_remote(posting, there));
    };
    Some(with_remote(posting, here))
}

fn remote_only(posting: &Posting) -> Option<String> {
    if !works_remote(posting) {
        return None;
    }
    Some(REMOTE.to_string())
}

fn with_remote(posting: &Posting, named: String) -> String {
    if !works_remote(posting) {
        return named;
    }
    if place::is_remote(&named) {
        return named;
    }
    format!("{}; {}", REMOTE, named)
}

fn works_remote(posting: &Posting) -> bool {
    let Some(flag) = posting.is_remote else {
        return false;
    };
    flag
}

fn secondary_spot(posting: &Posting) -> Option<String> {
    let Some(others) = &posting.secondary_locations else {
        return None;
    };
    for other in others {
        let Some(there) = spot_at(&other.location, &other.address) else {
            continue;
        };
        return Some(there);
    }
    None
}

fn spot_at(location: &Option<String>, address: &Option<Address>) -> Option<String> {
    let Some(named) = trimmed(location) else {
        return locality_of(address);
    };
    Some(named)
}

fn locality_of(address: &Option<Address>) -> Option<String> {
    let Some(found) = address else {
        return None;
    };
    let Some(postal) = &found.postal_address else {
        return None;
    };
    let Some(near) = trimmed(&postal.address_locality) else {
        return trimmed(&postal.address_country);
    };
    Some(near)
}

fn country_of(posting: &Posting) -> Country {
    let blob = region_blob(&posting.location, &posting.address);
    match place::country_of(&blob) {
        Country::Known(code) => Country::Known(code),
        Country::Unknown => secondary_country(posting),
    }
}

fn secondary_country(posting: &Posting) -> Country {
    let Some(others) = &posting.secondary_locations else {
        return Country::Unknown;
    };
    for other in others {
        let blob = region_blob(&other.location, &other.address);
        match place::country_of(&blob) {
            Country::Known(code) => return Country::Known(code),
            Country::Unknown => continue,
        }
    }
    Country::Unknown
}

fn region_blob(location: &Option<String>, address: &Option<Address>) -> String {
    let mut parts: Vec<String> = Vec::new();
    push_text(&mut parts, location);
    push_address(&mut parts, address);
    parts.join(" ")
}

fn push_address(parts: &mut Vec<String>, address: &Option<Address>) {
    let Some(found) = address else {
        return;
    };
    let Some(postal) = &found.postal_address else {
        return;
    };
    push_text(parts, &postal.address_country);
    push_text(parts, &postal.address_region);
    push_text(parts, &postal.address_locality);
}

fn push_text(parts: &mut Vec<String>, value: &Option<String>) {
    let Some(found) = trimmed(value) else {
        return;
    };
    parts.push(found);
}

fn stamp_of(posting: &Posting) -> Option<String> {
    let Some(raw) = trimmed(&posting.published_at) else {
        return None;
    };
    if looks_iso(&raw) {
        return Some(raw);
    }
    tracing::warn!(
        platform = PLATFORM,
        stamp = raw.as_str(),
        "publishedAt is not an iso stamp"
    );
    None
}

fn looks_iso(stamp: &str) -> bool {
    let bytes = stamp.as_bytes();
    if bytes.len() < ISO_DATE_LEN {
        return false;
    }
    if bytes[DASH_AT_YEAR] != b'-' {
        return false;
    }
    bytes[DASH_AT_MONTH] == b'-'
}

fn unit_of(posting: &Posting) -> Option<String> {
    let Some(named) = trimmed(&posting.department) else {
        return trimmed(&posting.team);
    };
    Some(named)
}

fn pay_of(posting: &Posting) -> Option<String> {
    let Some(block) = &posting.compensation else {
        return None;
    };
    let Some(summary) = trimmed(&block.compensation_tier_summary) else {
        return scraped_pay(block);
    };
    Some(summary)
}

fn scraped_pay(block: &Compensation) -> Option<String> {
    let Some(summary) = trimmed(&block.scrapeable_compensation_salary_summary) else {
        return built_pay(block);
    };
    Some(summary)
}

fn built_pay(block: &Compensation) -> Option<String> {
    let Some(parts) = &block.summary_components else {
        return None;
    };
    for part in parts {
        let Some(range) = range_of(part) else {
            continue;
        };
        return Some(range);
    }
    None
}

fn range_of(part: &Component) -> Option<String> {
    if !is_salary(part) {
        return None;
    }
    let span = span_of(part)?;
    let Some(currency) = trimmed(&part.currency_code) else {
        return Some(span);
    };
    Some(format!("{} {}", span, currency))
}

fn span_of(part: &Component) -> Option<String> {
    match (&part.min_value, &part.max_value) {
        (Some(least), Some(most)) => Some(format!("{} - {}", least, most)),
        (Some(least), None) => Some(format!("from {}", least)),
        (None, Some(most)) => Some(format!("up to {}", most)),
        (None, None) => None,
    }
}

fn is_salary(part: &Component) -> bool {
    let Some(kind) = &part.compensation_type else {
        return false;
    };
    kind.eq_ignore_ascii_case("salary")
}

fn trimmed(value: &Option<String>) -> Option<String> {
    let Some(found) = value else {
        return None;
    };
    let tidy = found.trim();
    if tidy.is_empty() {
        return None;
    }
    Some(tidy.to_string())
}
