use chrono::DateTime;
use reqwest::Client;
use serde::Deserialize;

use crate::error::AtsError;
use crate::filter::{self, Query};
use crate::handle;
use crate::http;
use crate::model::{Country, JobDetail, JobHit, Tenant};
use crate::place;
use crate::text;

const PLATFORM: &str = "lever";
const BASE: &str = "https://api.lever.co/v0/postings";
const MODE: &str = "mode=json";
const DAY: &str = "%Y-%m-%d";
const ISO_CODE_LEN: usize = 2;
const HARD_SPACE: char = '\u{a0}';
const REMOTE: &str = "Remote";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Posting {
    id: String,
    text: Option<String>,
    hosted_url: Option<String>,
    apply_url: Option<String>,
    created_at: Option<i64>,
    country: Option<String>,
    workplace_type: Option<String>,
    categories: Option<Categories>,
    description_plain: Option<String>,
    additional_plain: Option<String>,
    salary_description_plain: Option<String>,
    salary_range: Option<Pay>,
    lists: Option<Vec<Section>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Categories {
    location: Option<String>,
    department: Option<String>,
    team: Option<String>,
    commitment: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Section {
    text: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Pay {
    min: Option<serde_json::Number>,
    max: Option<serde_json::Number>,
    currency: Option<String>,
    interval: Option<String>,
}

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let url = format!("{}/{}?{}", BASE, tenant.slug, MODE);
    let postings: Vec<Posting> = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let mut hits: Vec<JobHit> = Vec::new();
    for posting in postings {
        let body = body_of(&posting);
        if !filter::coarse(&query.needle, &blob(&posting, &body)) {
            continue;
        }
        let hit = match to_hit(tenant, &posting, body) {
            Ok(hit) => hit,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    platform = PLATFORM,
                    tenant = tenant.slug,
                    job = posting.id.as_str(),
                    "posting skipped"
                );
                continue;
            }
        };
        hits.push(hit);
    }
    Ok(hits)
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let url = format!("{}/{}/{}?{}", BASE, tenant.slug, job, MODE);
    let posting: Posting = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let Some(description) = body_of(&posting) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "descriptionPlain",
        });
    };
    let where_at = spot_of(&posting);
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: title_of(tenant, &posting)?,
        location: where_at.clone(),
        country: country_of(&posting, &where_at),
        url: url_of(tenant, &posting)?,
        posted: stamp_of(&posting),
        department: unit_of(&posting),
        employment_type: commitment_of(&posting),
        compensation: pay_of(&posting),
        description,
    })
}

fn to_hit(tenant: &Tenant, posting: &Posting, body: Option<String>) -> Result<JobHit, AtsError> {
    let where_at = spot_of(posting);
    Ok(JobHit {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &posting.id),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: title_of(tenant, posting)?,
        location: where_at.clone(),
        country: country_of(posting, &where_at),
        url: url_of(tenant, posting)?,
        posted: stamp_of(posting),
        text: body,
    })
}

fn title_of(tenant: &Tenant, posting: &Posting) -> Result<String, AtsError> {
    let Some(found) = trimmed(&posting.text) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "text",
        });
    };
    Ok(found)
}

fn url_of(tenant: &Tenant, posting: &Posting) -> Result<String, AtsError> {
    let Some(hosted) = trimmed(&posting.hosted_url) else {
        let Some(apply) = trimmed(&posting.apply_url) else {
            return Err(AtsError::Malformed {
                platform: PLATFORM,
                tenant: tenant.slug.to_string(),
                field: "hostedUrl",
            });
        };
        return Ok(apply);
    };
    Ok(hosted)
}

fn blob(posting: &Posting, body: &Option<String>) -> String {
    let mut parts: Vec<String> = Vec::new();
    push_text(&mut parts, &posting.text);
    push_text(&mut parts, body);
    parts.join("\n")
}

fn body_of(posting: &Posting) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    push_text(&mut parts, &posting.description_plain);
    push_sections(&mut parts, &posting.lists);
    push_text(&mut parts, &posting.additional_plain);
    push_text(&mut parts, &posting.salary_description_plain);
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("\n"))
}

fn push_sections(parts: &mut Vec<String>, sections: &Option<Vec<Section>>) {
    let Some(found) = sections else {
        return;
    };
    for section in found {
        push_text(parts, &section.text);
        push_plain(parts, &section.content);
    }
}

fn push_text(parts: &mut Vec<String>, value: &Option<String>) {
    let Some(found) = trimmed(value) else {
        return;
    };
    parts.push(found.replace(HARD_SPACE, " "));
}

fn push_plain(parts: &mut Vec<String>, value: &Option<String>) {
    let Some(found) = trimmed(value) else {
        return;
    };
    let rendered = text::plain(&found);
    if rendered.is_empty() {
        return;
    }
    parts.push(rendered);
}

fn spot_of(posting: &Posting) -> Option<String> {
    let Some(found) = &posting.categories else {
        return remote_only(posting);
    };
    let Some(named) = trimmed(&found.location) else {
        return remote_only(posting);
    };
    Some(with_remote(posting, named))
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
    let Some(kind) = &posting.workplace_type else {
        return false;
    };
    kind.eq_ignore_ascii_case(REMOTE)
}

fn unit_of(posting: &Posting) -> Option<String> {
    let Some(found) = &posting.categories else {
        return None;
    };
    let Some(department) = trimmed(&found.department) else {
        return trimmed(&found.team);
    };
    Some(department)
}

fn commitment_of(posting: &Posting) -> Option<String> {
    let Some(found) = &posting.categories else {
        return None;
    };
    trimmed(&found.commitment)
}

fn country_of(posting: &Posting, where_at: &Option<String>) -> Country {
    let Some(code) = iso_of(&posting.country) else {
        let Some(found) = where_at else {
            return Country::Unknown;
        };
        return place::country_of(found);
    };
    Country::Known(code)
}

fn iso_of(value: &Option<String>) -> Option<String> {
    let Some(found) = trimmed(value) else {
        return None;
    };
    if found.len() != ISO_CODE_LEN {
        return None;
    }
    for letter in found.chars() {
        if !letter.is_ascii_alphabetic() {
            return None;
        }
    }
    Some(found.to_uppercase())
}

fn stamp_of(posting: &Posting) -> Option<String> {
    let Some(millis) = posting.created_at else {
        return None;
    };
    let Some(moment) = DateTime::from_timestamp_millis(millis) else {
        tracing::warn!(
            platform = PLATFORM,
            job = posting.id.as_str(),
            millis = millis,
            "createdAt is outside the representable range"
        );
        return None;
    };
    Some(moment.format(DAY).to_string())
}

fn pay_of(posting: &Posting) -> Option<String> {
    let Some(found) = range_of(&posting.salary_range) else {
        return trimmed(&posting.salary_description_plain);
    };
    Some(found)
}

fn range_of(pay: &Option<Pay>) -> Option<String> {
    let Some(found) = pay else {
        return None;
    };
    let Some(low) = &found.min else {
        return None;
    };
    let Some(high) = &found.max else {
        return None;
    };
    let Some(currency) = trimmed(&found.currency) else {
        return None;
    };
    let Some(interval) = trimmed(&found.interval) else {
        return Some(format!("{} - {} {}", low, high, currency));
    };
    Some(format!("{} - {} {} ({})", low, high, currency, interval))
}

fn trimmed(value: &Option<String>) -> Option<String> {
    let Some(found) = value else {
        return None;
    };
    let clean = found.trim();
    if clean.is_empty() {
        return None;
    }
    Some(clean.to_string())
}
