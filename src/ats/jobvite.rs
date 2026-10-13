use reqwest::Client;
use serde::Deserialize;

use crate::config::Config;
use crate::error::AtsError;
use crate::filter::{self, Query};
use crate::handle;
use crate::http;
use crate::model::{Country, JobDetail, JobHit, Tenant};
use crate::place;
use crate::text;

const PLATFORM: &str = "jobvite";
const BOARD: &str = "https://jobs.jobvite.com";
const PAGE: usize = 50;
const NAME_MARK: &str = "jv-job-list-name";
const SPOT_MARK: &str = "jv-job-list-location";
const ANCHOR_END: &str = "</a>";
const DIV_OPEN: &str = "<div";
const DIV_CLOSE: &str = "</div>";
const CELL_CLOSE: &str = "</td>";
const LD_MARK: &str = "application/ld+json";
const LD_END: &str = "</script>";
const POSTING_MARK: &str = "JobPosting";
const TELECOMMUTE: &str = "TELECOMMUTE";
const REMOTE: &str = "Remote";
const ISO_DATE_LEN: usize = 10;
const DASH_AT_YEAR: usize = 4;
const DASH_AT_MONTH: usize = 7;
const UNRESERVED: [char; 4] = ['-', '_', '.', '~'];

#[derive(Debug, Deserialize)]
struct Posting {
    title: Option<String>,
    description: Option<String>,
    industry: Option<String>,
    #[serde(rename = "datePosted")]
    date_posted: Option<String>,
    #[serde(rename = "employmentType")]
    employment_type: Option<String>,
    #[serde(rename = "jobLocation")]
    job_location: Option<Where>,
    #[serde(rename = "jobLocationType")]
    job_location_type: Option<String>,
    #[serde(rename = "baseSalary")]
    base_salary: Option<Money>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Where {
    Many(Vec<Spot>),
    One(Spot),
}

#[derive(Debug, Deserialize)]
struct Spot {
    address: Option<Address>,
}

#[derive(Debug, Deserialize)]
struct Address {
    #[serde(rename = "addressLocality")]
    locality: Option<String>,
    #[serde(rename = "addressRegion")]
    region: Option<String>,
    #[serde(rename = "addressCountry")]
    country: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Money {
    currency: Option<String>,
    value: Option<Amount>,
}

#[derive(Debug, Deserialize)]
struct Amount {
    #[serde(rename = "minValue")]
    min: Option<serde_json::Value>,
    #[serde(rename = "maxValue")]
    max: Option<serde_json::Value>,
}

struct Row {
    job: String,
    title: String,
    spot: Option<String>,
}

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let ceiling = Config::new().per_tenant_cap;
    let mut hits: Vec<JobHit> = Vec::new();
    let mut scanned: usize = 0;
    let mut page: usize = 0;
    loop {
        let url = page_url(tenant.slug, query, page);
        let body = http::get_text(client, PLATFORM, tenant.slug, &url).await?;
        let rows = rows_of(tenant.slug, &body);
        let seen = rows.len();
        for row in &rows {
            if hits.len() >= ceiling {
                break;
            }
            if !filter::coarse(&query.needle, &blob(row)) {
                continue;
            }
            hits.push(to_hit(tenant, row));
        }
        scanned += seen;
        if seen < PAGE || hits.len() >= ceiling || scanned >= ceiling {
            return Ok(hits);
        }
        page += 1;
    }
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let url = job_url(tenant.slug, job);
    let body = http::get_text(client, PLATFORM, tenant.slug, &url).await?;
    let Some(block) = ld_block(&body) else {
        return Err(AtsError::NoSuchJob {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            job: job.to_string(),
        });
    };
    let posting: Posting = match serde_json::from_str::<Posting>(block) {
        Ok(posting) => posting,
        Err(source) => {
            return Err(AtsError::Parse {
                platform: PLATFORM,
                tenant: tenant.slug.to_string(),
                source,
            });
        }
    };
    let Some(raw) = &posting.description else {
        return Err(missing(tenant, "description"));
    };
    let description = text::plain(raw);
    if description.is_empty() {
        return Err(missing(tenant, "description"));
    }
    let Some(title) = &posting.title else {
        return Err(missing(tenant, "title"));
    };
    let where_at = spot_from(&posting);
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: title.clone(),
        location: where_at.clone(),
        country: country_of(&where_at),
        url,
        posted: stamp_of(&posting),
        department: trimmed(&posting.industry),
        employment_type: trimmed(&posting.employment_type),
        compensation: pay_of(&posting),
        description,
    })
}

fn rows_of(slug: &str, body: &str) -> Vec<Row> {
    let mark = format!("href=\"/{}/job/", slug);
    let mut rows: Vec<Row> = Vec::new();
    for chunk in body.split(mark.as_str()).skip(1) {
        let Some(stop) = chunk.find('"') else {
            continue;
        };
        let job = &chunk[..stop];
        let segment = &chunk[stop..];
        let Some(title) = title_of(segment) else {
            tracing::warn!(
                platform = PLATFORM,
                tenant = slug,
                job = job,
                "listing row carries no title"
            );
            continue;
        };
        if seen_already(&rows, job) {
            continue;
        }
        rows.push(Row {
            job: job.to_string(),
            title,
            spot: spot_of(segment),
        });
    }
    rows
}

fn seen_already(rows: &[Row], job: &str) -> bool {
    for row in rows {
        if row.job == job {
            return true;
        }
    }
    false
}

fn title_of(segment: &str) -> Option<String> {
    let Some(name_at) = segment.find(NAME_MARK) else {
        return anchor_text(segment);
    };
    let Some(spot_at) = segment.find(SPOT_MARK) else {
        return cell_text(segment, name_at + NAME_MARK.len());
    };
    if name_at > spot_at {
        return anchor_text(segment);
    }
    cell_text(segment, name_at + NAME_MARK.len())
}

fn spot_of(segment: &str) -> Option<String> {
    let Some(spot_at) = segment.find(SPOT_MARK) else {
        return None;
    };
    cell_text(segment, spot_at + SPOT_MARK.len())
}

fn anchor_text(segment: &str) -> Option<String> {
    let Some(open) = segment.find('>') else {
        return None;
    };
    let inner = &segment[open + 1..];
    let Some(stop) = inner.find(ANCHOR_END) else {
        return None;
    };
    readable(&inner[..stop])
}

fn cell_text(segment: &str, from: usize) -> Option<String> {
    let tail = &segment[from..];
    let Some(open) = tail.find('>') else {
        return None;
    };
    let inner = &tail[open + 1..];
    let stop = cell_end(inner);
    readable(&inner[..stop])
}

fn cell_end(inner: &str) -> usize {
    let mut depth: usize = 0;
    let mut from: usize = 0;
    loop {
        let Some(ahead) = inner[from..].find('<') else {
            return inner.len();
        };
        let mark = from + ahead;
        let tail = &inner[mark..];
        if tail.starts_with(CELL_CLOSE) {
            return mark;
        }
        if tail.starts_with(DIV_CLOSE) {
            if depth == 0 {
                return mark;
            }
            depth -= 1;
        }
        if tail.starts_with(DIV_OPEN) {
            depth += 1;
        }
        from = mark + 1;
    }
}

fn readable(raw: &str) -> Option<String> {
    let flat = text::plain(raw);
    let words: Vec<&str> = flat.split_whitespace().collect();
    let found = words.join(" ").replace(" ,", ",");
    if found.is_empty() {
        return None;
    }
    Some(found)
}

fn blob(row: &Row) -> String {
    let Some(spot) = &row.spot else {
        return row.title.clone();
    };
    format!("{} {}", row.title, spot)
}

fn to_hit(tenant: &Tenant, row: &Row) -> JobHit {
    JobHit {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &row.job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: row.title.clone(),
        location: row.spot.clone(),
        country: country_of(&row.spot),
        url: job_url(tenant.slug, &row.job),
        posted: None,
        text: None,
    }
}

fn ld_block(body: &str) -> Option<&str> {
    for chunk in body.split(LD_MARK).skip(1) {
        let Some(open) = chunk.find('>') else {
            continue;
        };
        let inner = &chunk[open + 1..];
        let Some(stop) = inner.find(LD_END) else {
            continue;
        };
        let block = &inner[..stop];
        if !block.contains(POSTING_MARK) {
            continue;
        }
        return Some(block);
    }
    None
}

fn spot_from(posting: &Posting) -> Option<String> {
    let Some(found) = &posting.job_location else {
        return remote_spot(posting);
    };
    let mut parts: Vec<String> = Vec::new();
    for spot in spots_of(found) {
        let Some(address) = &spot.address else {
            continue;
        };
        push_part(&mut parts, &address.locality);
        push_part(&mut parts, &address.region);
        push_part(&mut parts, &address.country);
    }
    if parts.is_empty() {
        return remote_spot(posting);
    }
    Some(parts.join(", "))
}

fn spots_of(found: &Where) -> Vec<&Spot> {
    match found {
        Where::Many(list) => {
            let mut out: Vec<&Spot> = Vec::new();
            for spot in list {
                out.push(spot);
            }
            out
        }
        Where::One(single) => vec![single],
    }
}

fn push_part(parts: &mut Vec<String>, value: &Option<String>) {
    let Some(found) = trimmed(value) else {
        return;
    };
    for seen in parts.as_slice() {
        if seen.eq_ignore_ascii_case(&found) {
            return;
        }
    }
    parts.push(found);
}

fn remote_spot(posting: &Posting) -> Option<String> {
    let Some(kind) = &posting.job_location_type else {
        return None;
    };
    if !kind.trim().eq_ignore_ascii_case(TELECOMMUTE) {
        return None;
    }
    Some(REMOTE.to_string())
}

fn country_of(where_at: &Option<String>) -> Country {
    let Some(found) = where_at else {
        return Country::Unknown;
    };
    place::country_of(found)
}

fn stamp_of(posting: &Posting) -> Option<String> {
    let Some(raw) = trimmed(&posting.date_posted) else {
        return None;
    };
    if !looks_iso(&raw) {
        tracing::debug!(
            platform = PLATFORM,
            value = raw.as_str(),
            "posted date is not iso, dropping it"
        );
        return None;
    }
    Some(raw)
}

fn looks_iso(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < ISO_DATE_LEN {
        return false;
    }
    if bytes[DASH_AT_YEAR] != b'-' || bytes[DASH_AT_MONTH] != b'-' {
        return false;
    }
    for index in 0..ISO_DATE_LEN {
        if index == DASH_AT_YEAR || index == DASH_AT_MONTH {
            continue;
        }
        if !bytes[index].is_ascii_digit() {
            return false;
        }
    }
    true
}

fn pay_of(posting: &Posting) -> Option<String> {
    let Some(money) = &posting.base_salary else {
        return None;
    };
    let Some(amount) = &money.value else {
        return None;
    };
    let Some(span) = span_of(amount) else {
        return None;
    };
    let Some(currency) = trimmed(&money.currency) else {
        return Some(span);
    };
    Some(format!("{} {}", currency, span))
}

fn span_of(amount: &Amount) -> Option<String> {
    match (scalar(&amount.min), scalar(&amount.max)) {
        (Some(low), Some(high)) => Some(format!("{}-{}", low, high)),
        (Some(low), None) => Some(low),
        (None, Some(high)) => Some(high),
        (None, None) => None,
    }
}

fn scalar(value: &Option<serde_json::Value>) -> Option<String> {
    let Some(found) = value else {
        return None;
    };
    let Some(written) = found.as_str() else {
        return number(found);
    };
    let cleaned = written.trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.to_string())
}

fn number(found: &serde_json::Value) -> Option<String> {
    let Some(count) = found.as_f64() else {
        return None;
    };
    Some(format!("{}", count))
}

fn trimmed(value: &Option<String>) -> Option<String> {
    let Some(found) = value else {
        return None;
    };
    let cleaned = found.trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned.to_string())
}

fn page_url(slug: &str, query: &Query, page: usize) -> String {
    let needle = query.needle.trim();
    if needle.is_empty() {
        return format!("{}/{}/search?p={}", BOARD, slug, page);
    }
    format!("{}/{}/search?q={}&p={}", BOARD, slug, encoded(needle), page)
}

fn job_url(slug: &str, job: &str) -> String {
    format!("{}/{}/job/{}", BOARD, slug, job)
}

fn encoded(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.bytes() {
        let letter = char::from(byte);
        if letter.is_ascii_alphanumeric() || UNRESERVED.contains(&letter) {
            out.push(letter);
            continue;
        }
        out.push_str(&format!("%{:02X}", byte));
    }
    out
}

fn missing(tenant: &Tenant, field: &'static str) -> AtsError {
    AtsError::Malformed {
        platform: PLATFORM,
        tenant: tenant.slug.to_string(),
        field,
    }
}
