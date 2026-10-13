use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::AtsError;
use crate::filter::{self, Query};
use crate::handle;
use crate::http;
use crate::model::{Country, JobDetail, JobHit, Tenant};
use crate::place;
use crate::text;

const PLATFORM: &str = "workday";
const PAGE: usize = 20;
const ISO_DAY: &str = "%Y-%m-%d";
const TODAY: &str = "today";
const YESTERDAY: &str = "yesterday";
const DAY: &str = "day";
const SAME_DAY: i64 = 0;
const ONE_DAY: i64 = 1;
const ISO_LEN: usize = 10;
const DECIMAL: u32 = 10;
const TENFOLD: i64 = 10;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Ask {
    applied_facets: BTreeMap<String, Vec<String>>,
    limit: usize,
    offset: usize,
    search_text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listing {
    #[serde(default)]
    total: Option<usize>,
    #[serde(default)]
    job_postings: Option<Vec<Posting>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Posting {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    external_path: Option<String>,
    #[serde(default)]
    locations_text: Option<String>,
    #[serde(default)]
    posted_on: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Detail {
    #[serde(default)]
    job_posting_info: Option<Info>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    job_description: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    posted_on: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    time_type: Option<String>,
    #[serde(default)]
    country: Option<Named>,
}

#[derive(Debug, Deserialize)]
struct Named {
    #[serde(default)]
    descriptor: Option<String>,
}

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let url = format!(
        "https://{}/wday/cxs/{}/{}/jobs",
        tenant.host, tenant.slug, tenant.site
    );
    let ceiling = Config::new().per_tenant_cap;
    let head = page_of(
        client,
        tenant,
        &url,
        &query.needle,
        0,
        usize::min(PAGE, ceiling),
    )
    .await?;
    let Some(total) = head.total else {
        return Err(missing(tenant, "total"));
    };
    let Some(postings) = head.job_postings else {
        return Err(missing(tenant, "jobPostings"));
    };
    let bound = usize::min(total, ceiling);
    let mut hits: Vec<JobHit> = Vec::new();
    let mut offset = postings.len();
    gather(tenant, query, postings, &mut hits);
    while offset < bound {
        let Some(room) = bound.checked_sub(offset) else {
            break;
        };
        let page = page_of(
            client,
            tenant,
            &url,
            &query.needle,
            offset,
            usize::min(PAGE, room),
        )
        .await?;
        let Some(postings) = page.job_postings else {
            break;
        };
        let counted = postings.len();
        gather(tenant, query, postings, &mut hits);
        if counted == 0 {
            break;
        }
        let Some(seen) = offset.checked_add(counted) else {
            break;
        };
        offset = seen;
    }
    Ok(hits)
}

async fn page_of(
    client: &Client,
    tenant: &Tenant,
    url: &str,
    needle: &str,
    offset: usize,
    limit: usize,
) -> Result<Listing, AtsError> {
    let ask = Ask {
        applied_facets: BTreeMap::new(),
        limit,
        offset,
        search_text: needle.to_string(),
    };
    http::post_json(client, PLATFORM, tenant.slug, url, &ask).await
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let url = format!(
        "https://{}/wday/cxs/{}/{}{}",
        tenant.host, tenant.slug, tenant.site, job
    );
    let payload: Detail = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let Some(info) = payload.job_posting_info else {
        return Err(missing(tenant, "jobPostingInfo"));
    };
    let Some(raw) = &info.job_description else {
        return Err(missing(tenant, "jobDescription"));
    };
    let Some(title) = &info.title else {
        return Err(missing(tenant, "title"));
    };
    let description = text::plain(raw);
    if description.trim().is_empty() {
        return Err(missing(tenant, "jobDescription"));
    }
    let where_at = info.location.clone();
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: title.clone(),
        location: where_at.clone(),
        country: located(&where_at, &info),
        url: seen_by_humans(tenant, job),
        posted: stamped(&info),
        department: None,
        employment_type: info.time_type.clone(),
        compensation: None,
        description,
    })
}

fn gather(tenant: &Tenant, query: &Query, postings: Vec<Posting>, hits: &mut Vec<JobHit>) {
    for posting in postings {
        let Some(path) = &posting.external_path else {
            tracing::warn!(
                platform = PLATFORM,
                tenant = tenant.slug,
                "posting without an externalPath skipped"
            );
            continue;
        };
        let Some(title) = &posting.title else {
            tracing::warn!(
                platform = PLATFORM,
                tenant = tenant.slug,
                path = path.as_str(),
                "posting without a title skipped"
            );
            continue;
        };
        let where_at = posting.locations_text.clone();
        if !filter::coarse(&query.needle, &blob(title, &where_at)) {
            continue;
        }
        hits.push(JobHit {
            remote: false,
            id: handle::encode(tenant.ats, tenant.slug, path),
            ats: tenant.ats,
            company: tenant.company.to_string(),
            title: title.clone(),
            location: where_at.clone(),
            country: country_of(&where_at),
            url: seen_by_humans(tenant, path),
            posted: from_prose(&posting.posted_on),
            text: None,
        });
    }
}

fn seen_by_humans(tenant: &Tenant, path: &str) -> String {
    format!("https://{}/{}{}", tenant.host, tenant.site, path)
}

fn missing(tenant: &Tenant, field: &'static str) -> AtsError {
    AtsError::Malformed {
        platform: PLATFORM,
        tenant: tenant.slug.to_string(),
        field,
    }
}

fn blob(title: &str, where_at: &Option<String>) -> String {
    let Some(found) = where_at else {
        return title.to_string();
    };
    format!("{} {}", title, found)
}

fn country_of(where_at: &Option<String>) -> Country {
    let Some(found) = where_at else {
        return Country::Unknown;
    };
    place::country_of(found)
}

fn located(where_at: &Option<String>, info: &Info) -> Country {
    match country_of(where_at) {
        Country::Known(code) => Country::Known(code),
        Country::Unknown => declared_country(info),
    }
}

fn declared_country(info: &Info) -> Country {
    let Some(named) = &info.country else {
        return Country::Unknown;
    };
    let Some(descriptor) = &named.descriptor else {
        return Country::Unknown;
    };
    place::country_of(descriptor)
}

fn stamped(info: &Info) -> Option<String> {
    let Some(start) = &info.start_date else {
        return from_prose(&info.posted_on);
    };
    let head: String = start.trim().chars().take(ISO_LEN).collect();
    match NaiveDate::parse_from_str(&head, ISO_DAY) {
        Ok(day) => Some(day.format(ISO_DAY).to_string()),
        Err(e) => {
            tracing::warn!(
                error = %e,
                platform = PLATFORM,
                start = start.as_str(),
                "startDate is not a date, reading the posted prose instead"
            );
            from_prose(&info.posted_on)
        }
    }
}

fn from_prose(prose: &Option<String>) -> Option<String> {
    let Some(said) = prose else {
        return None;
    };
    let Some(days) = days_ago(said) else {
        tracing::debug!(
            platform = PLATFORM,
            prose = said.as_str(),
            "postedOn carries no day count, posted left absent"
        );
        return None;
    };
    let Some(span) = Duration::try_days(days) else {
        return None;
    };
    let Some(moment) = Utc::now().checked_sub_signed(span) else {
        return None;
    };
    Some(moment.format(ISO_DAY).to_string())
}

fn days_ago(said: &str) -> Option<i64> {
    let lowered = said.to_lowercase();
    if lowered.contains(TODAY) {
        return Some(SAME_DAY);
    }
    if lowered.contains(YESTERDAY) {
        return Some(ONE_DAY);
    }
    if !lowered.contains(DAY) {
        return None;
    }
    for chunk in lowered.split_whitespace() {
        let Some(count) = leading_count(chunk) else {
            continue;
        };
        return Some(count);
    }
    None
}

fn leading_count(chunk: &str) -> Option<i64> {
    let mut count: i64 = 0;
    let mut digits = false;
    for ch in chunk.chars() {
        let Some(digit) = ch.to_digit(DECIMAL) else {
            break;
        };
        let Some(widened) = count.checked_mul(TENFOLD) else {
            return None;
        };
        let Some(grown) = widened.checked_add(i64::from(digit)) else {
            return None;
        };
        count = grown;
        digits = true;
    }
    match digits {
        true => Some(count),
        false => None,
    }
}
