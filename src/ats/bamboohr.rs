use reqwest::Client;
use serde::Deserialize;

use crate::error::AtsError;
use crate::filter::{self, Query};
use crate::handle;
use crate::http;
use crate::model::{Country, JobDetail, JobHit, Tenant};
use crate::place;
use crate::text;

const PLATFORM: &str = "bamboohr";
const HOST: &str = "bamboohr.com";
const ISO_LEN: usize = 10;
const DASH_AT_YEAR: usize = 4;
const DASH_AT_MONTH: usize = 7;
const FOOTNOTE: char = '*';

#[derive(Debug, Deserialize)]
struct Board {
    result: Vec<Listing>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Listing {
    id: Option<Scalar>,
    job_opening_name: Option<String>,
    location: Option<Site>,
    ats_location: Option<AtsSite>,
}

#[derive(Debug, Deserialize)]
struct Answer {
    result: Held,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Held {
    job_opening: Opening,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Opening {
    job_opening_name: Option<String>,
    description: Option<String>,
    date_posted: Option<String>,
    department_label: Option<String>,
    employment_status_label: Option<String>,
    compensation: Option<String>,
    location: Option<Site>,
    ats_location: Option<AtsSite>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Site {
    city: Option<String>,
    state: Option<String>,
    address_country: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AtsSite {
    city: Option<String>,
    state: Option<String>,
    province: Option<String>,
    country: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Scalar {
    Text(String),
    Number(i64),
}

#[derive(Debug)]
struct Spot {
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
}

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let url = format!("https://{}.{}/careers/list", tenant.slug, HOST);
    let board: Board = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let mut hits: Vec<JobHit> = Vec::new();
    for listing in board.result {
        let Some(title) = clean(&listing.job_opening_name) else {
            tracing::warn!(
                platform = PLATFORM,
                tenant = tenant.slug,
                "listing carries no jobOpeningName, skipped"
            );
            continue;
        };
        if !filter::coarse(&query.needle, &title) {
            continue;
        }
        let Some(job) = id_of(&listing.id) else {
            tracing::warn!(
                platform = PLATFORM,
                tenant = tenant.slug,
                title = title.as_str(),
                "listing carries no id, skipped"
            );
            continue;
        };
        hits.push(to_hit(tenant, &listing, &title, &job));
    }
    Ok(hits)
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let url = format!("https://{}.{}/careers/{}/detail", tenant.slug, HOST, job);
    let answer: Answer = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let opening = answer.result.job_opening;
    let Some(title) = clean(&opening.job_opening_name) else {
        return Err(missing(tenant, "jobOpeningName"));
    };
    let Some(raw) = &opening.description else {
        return Err(missing(tenant, "description"));
    };
    let body = text::plain(raw);
    if body.is_empty() {
        return Err(missing(tenant, "description"));
    }
    let spot = spot_of(&opening.location, &opening.ats_location);
    let where_at = label_of(&spot);
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title,
        location: where_at.clone(),
        country: country_of(&spot, &where_at),
        url: page_url(tenant.slug, job),
        posted: stamp_of(&opening.date_posted),
        department: clean(&opening.department_label),
        employment_type: employment_of(&opening.employment_status_label),
        compensation: clean(&opening.compensation),
        description: body,
    })
}

fn to_hit(tenant: &Tenant, listing: &Listing, title: &str, job: &str) -> JobHit {
    let spot = spot_of(&listing.location, &listing.ats_location);
    let where_at = label_of(&spot);
    JobHit {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: title.to_string(),
        location: where_at.clone(),
        country: country_of(&spot, &where_at),
        url: page_url(tenant.slug, job),
        posted: None,
        text: None,
    }
}

fn page_url(slug: &str, job: &str) -> String {
    format!("https://{}.{}/careers/{}", slug, HOST, job)
}

fn missing(tenant: &Tenant, field: &'static str) -> AtsError {
    AtsError::Malformed {
        platform: PLATFORM,
        tenant: tenant.slug.to_string(),
        field,
    }
}

fn spot_of(site: &Option<Site>, ats: &Option<AtsSite>) -> Spot {
    let near = from_site(site);
    let wide = from_ats(ats);
    Spot {
        city: either(near.city, wide.city),
        region: either(near.region, wide.region),
        country: either(near.country, wide.country),
    }
}

fn from_site(site: &Option<Site>) -> Spot {
    let Some(found) = site else {
        return nowhere();
    };
    Spot {
        city: clean(&found.city),
        region: clean(&found.state),
        country: clean(&found.address_country),
    }
}

fn from_ats(ats: &Option<AtsSite>) -> Spot {
    let Some(found) = ats else {
        return nowhere();
    };
    Spot {
        city: clean(&found.city),
        region: either(clean(&found.state), clean(&found.province)),
        country: clean(&found.country),
    }
}

fn nowhere() -> Spot {
    Spot {
        city: None,
        region: None,
        country: None,
    }
}

fn either(primary: Option<String>, fallback: Option<String>) -> Option<String> {
    let Some(found) = primary else {
        return fallback;
    };
    Some(found)
}

fn label_of(spot: &Spot) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    push_part(&mut parts, &spot.city);
    push_part(&mut parts, &spot.region);
    push_part(&mut parts, &spot.country);
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(", "))
}

fn push_part(parts: &mut Vec<String>, value: &Option<String>) {
    let Some(text) = value else {
        return;
    };
    if !text.chars().any(|letter| letter.is_alphanumeric()) {
        return;
    }
    for held in &*parts {
        if held.eq_ignore_ascii_case(text) {
            return;
        }
    }
    parts.push(text.clone());
}

fn country_of(spot: &Spot, where_at: &Option<String>) -> Country {
    let Some(named) = &spot.country else {
        return from_label(where_at);
    };
    let Country::Known(code) = place::country_of(named) else {
        return from_label(where_at);
    };
    Country::Known(code)
}

fn from_label(where_at: &Option<String>) -> Country {
    let Some(found) = where_at else {
        return Country::Unknown;
    };
    place::country_of(found)
}

fn stamp_of(value: &Option<String>) -> Option<String> {
    let Some(text) = clean(value) else {
        return None;
    };
    let head: String = text.chars().take(ISO_LEN).collect();
    let bytes = head.as_bytes();
    if bytes.len() < ISO_LEN || bytes[DASH_AT_YEAR] != b'-' || bytes[DASH_AT_MONTH] != b'-' {
        tracing::warn!(
            platform = PLATFORM,
            stamp = text.as_str(),
            "datePosted is not an iso date, left absent"
        );
        return None;
    }
    Some(text)
}

fn employment_of(value: &Option<String>) -> Option<String> {
    let Some(text) = clean(value) else {
        return None;
    };
    let stripped = text.trim_end_matches(FOOTNOTE).trim();
    if stripped.is_empty() {
        return None;
    }
    Some(stripped.to_string())
}

fn id_of(value: &Option<Scalar>) -> Option<String> {
    let Some(found) = value else {
        return None;
    };
    let text = match found {
        Scalar::Text(raw) => raw.trim().to_string(),
        Scalar::Number(raw) => raw.to_string(),
    };
    if text.is_empty() {
        return None;
    }
    Some(text)
}

fn clean(value: &Option<String>) -> Option<String> {
    let Some(text) = value else {
        return None;
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}
