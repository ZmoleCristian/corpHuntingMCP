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

const PLATFORM: &str = "smartrecruiters";
const BASE: &str = "https://api.smartrecruiters.com/v1/companies";
const BOARD: &str = "https://jobs.smartrecruiters.com";
const PAGE: usize = 100;
const ISO_DATE_LEN: usize = 10;
const DASH_AT_YEAR: usize = 4;
const DASH_AT_MONTH: usize = 7;
const ISO_CODE_LEN: usize = 2;
const UNRESERVED: [char; 4] = ['-', '_', '.', '~'];

#[derive(Debug, Deserialize)]
struct Page {
    content: Vec<Posting>,
    #[serde(rename = "totalFound")]
    total_found: usize,
}

#[derive(Debug, Deserialize)]
struct Posting {
    id: String,
    name: String,
    location: Option<Spot>,
    department: Option<Labelled>,
    #[serde(rename = "typeOfEmployment")]
    type_of_employment: Option<Labelled>,
    #[serde(rename = "releasedDate")]
    released_date: Option<String>,
    #[serde(rename = "createdOn")]
    created_on: Option<String>,
    compensation: Option<Money>,
    #[serde(rename = "jobAd")]
    job_ad: Option<JobAd>,
}

#[derive(Debug, Deserialize)]
struct Spot {
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Labelled {
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Money {
    min: Option<f64>,
    max: Option<f64>,
    currency: Option<String>,
    period: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JobAd {
    sections: Option<Sections>,
}

#[derive(Debug, Deserialize)]
struct Sections {
    #[serde(rename = "companyDescription")]
    company_description: Option<Section>,
    #[serde(rename = "jobDescription")]
    job_description: Option<Section>,
    qualifications: Option<Section>,
    #[serde(rename = "additionalInformation")]
    additional_information: Option<Section>,
}

#[derive(Debug, Deserialize)]
struct Section {
    text: Option<String>,
}

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let ceiling = Config::new().per_tenant_cap;
    let mut hits: Vec<JobHit> = Vec::new();
    let mut offset: usize = 0;
    loop {
        let url = page_url(tenant.slug, query, offset);
        let page: Page = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
        let seen = page.content.len();
        for posting in &page.content {
            if hits.len() >= ceiling {
                break;
            }
            if !filter::coarse(&query.needle, &posting.name) {
                continue;
            }
            hits.push(to_hit(tenant, posting));
        }
        offset += seen;
        let done = seen == 0 || hits.len() >= ceiling || offset >= page.total_found;
        if done {
            return Ok(hits);
        }
    }
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let url = format!("{}/{}/postings/{}", BASE, tenant.slug, encoded(job));
    let posting: Posting = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let Some(body) = sections_of(&posting) else {
        return Err(missing(tenant, "jobAd.sections"));
    };
    let description = text::plain(&body);
    if description.is_empty() {
        return Err(missing(tenant, "jobAd.sections.text"));
    }
    let where_at = spot_of(&posting);
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &posting.id),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: posting.name.clone(),
        location: where_at.clone(),
        country: country_for(&posting, &where_at),
        url: format!("{}/{}/{}", BOARD, tenant.slug, posting.id),
        posted: stamp_of(&posting),
        department: label_of(&posting.department),
        employment_type: label_of(&posting.type_of_employment),
        compensation: pay_of(&posting),
        description,
    })
}

fn to_hit(tenant: &Tenant, posting: &Posting) -> JobHit {
    let where_at = spot_of(posting);
    JobHit {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &posting.id),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: posting.name.clone(),
        location: where_at.clone(),
        country: country_for(posting, &where_at),
        url: format!("{}/{}/{}", BOARD, tenant.slug, posting.id),
        posted: stamp_of(posting),
        text: None,
    }
}

fn page_url(slug: &str, query: &Query, offset: usize) -> String {
    let mut url = format!("{}/{}/postings?limit={}&offset={}", BASE, slug, PAGE, offset);
    let needle = query.needle.trim();
    if !needle.is_empty() {
        url.push_str(&format!("&q={}", encoded(needle)));
    }
    let Some(code) = wanted_country(&query.country) else {
        return url;
    };
    url.push_str(&format!("&country={}", code));
    url
}

fn wanted_country(wanted: &Option<String>) -> Option<String> {
    let Some(code) = iso_two(wanted) else {
        return None;
    };
    Some(code.to_lowercase())
}

fn iso_two(value: &Option<String>) -> Option<String> {
    let Some(code) = trimmed(value) else {
        return None;
    };
    if code.chars().count() != ISO_CODE_LEN {
        return None;
    }
    for letter in code.chars() {
        if !letter.is_ascii_alphabetic() {
            return None;
        }
    }
    Some(code)
}

fn spot_of(posting: &Posting) -> Option<String> {
    let Some(spot) = &posting.location else {
        return None;
    };
    let mut parts: Vec<String> = Vec::new();
    append(&mut parts, trimmed(&spot.city));
    append(&mut parts, region_of(spot));
    append(&mut parts, upper(&spot.country));
    if parts.is_empty() {
        return None;
    }
    Some(parts.join(", "))
}

fn region_of(spot: &Spot) -> Option<String> {
    let Some(region) = upper(&spot.region) else {
        return None;
    };
    let Some(country) = upper(&spot.country) else {
        return Some(region);
    };
    if region == country {
        return None;
    }
    Some(region)
}

fn country_for(posting: &Posting, where_at: &Option<String>) -> Country {
    let Some(spot) = &posting.location else {
        return fallback_country(where_at);
    };
    let Some(code) = iso_two(&spot.country) else {
        return fallback_country(where_at);
    };
    Country::Known(code.to_uppercase())
}

fn fallback_country(where_at: &Option<String>) -> Country {
    let Some(found) = where_at else {
        return Country::Unknown;
    };
    place::country_of(found)
}

fn stamp_of(posting: &Posting) -> Option<String> {
    let Some(raw) = newest_stamp(posting) else {
        return None;
    };
    let head: String = raw.chars().take(ISO_DATE_LEN).collect();
    if !looks_iso(&head) {
        tracing::debug!(
            stamp = raw.as_str(),
            platform = PLATFORM,
            "posting carries a stamp that is not iso"
        );
        return None;
    }
    Some(raw)
}

fn newest_stamp(posting: &Posting) -> Option<String> {
    let Some(released) = trimmed(&posting.released_date) else {
        return trimmed(&posting.created_on);
    };
    Some(released)
}

fn looks_iso(head: &str) -> bool {
    let bytes = head.as_bytes();
    if bytes.len() < ISO_DATE_LEN {
        return false;
    }
    bytes[DASH_AT_YEAR] == b'-' && bytes[DASH_AT_MONTH] == b'-'
}

fn sections_of(posting: &Posting) -> Option<String> {
    let Some(advert) = &posting.job_ad else {
        return None;
    };
    let Some(sections) = &advert.sections else {
        return None;
    };
    let mut parts: Vec<String> = Vec::new();
    append(&mut parts, body_of(&sections.company_description));
    append(&mut parts, body_of(&sections.job_description));
    append(&mut parts, body_of(&sections.qualifications));
    append(&mut parts, body_of(&sections.additional_information));
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("\n"))
}

fn body_of(section: &Option<Section>) -> Option<String> {
    let Some(found) = section else {
        return None;
    };
    trimmed(&found.text)
}

fn label_of(item: &Option<Labelled>) -> Option<String> {
    let Some(found) = item else {
        return None;
    };
    trimmed(&found.label)
}

fn pay_of(posting: &Posting) -> Option<String> {
    let Some(money) = &posting.compensation else {
        return None;
    };
    let Some(currency) = upper(&money.currency) else {
        return None;
    };
    let Some(period) = lower(&money.period) else {
        return None;
    };
    match (money.min, money.max) {
        (Some(low), Some(high)) => Some(format!("{}-{} {} {}", low, high, currency, period)),
        (Some(low), None) => Some(format!("from {} {} {}", low, currency, period)),
        (None, Some(high)) => Some(format!("up to {} {} {}", high, currency, period)),
        (None, None) => None,
    }
}

fn append(parts: &mut Vec<String>, value: Option<String>) {
    let Some(found) = value else {
        return;
    };
    parts.push(found);
}

fn trimmed(value: &Option<String>) -> Option<String> {
    let Some(text) = value else {
        return None;
    };
    let cut = text.trim();
    if cut.is_empty() {
        return None;
    }
    Some(cut.to_string())
}

fn upper(value: &Option<String>) -> Option<String> {
    let Some(text) = trimmed(value) else {
        return None;
    };
    Some(text.to_uppercase())
}

fn lower(value: &Option<String>) -> Option<String> {
    let Some(text) = trimmed(value) else {
        return None;
    };
    Some(text.to_lowercase())
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
