use reqwest::Client;
use serde::Deserialize;

use crate::error::AtsError;
use crate::filter::{self, Query};
use crate::handle;
use crate::http;
use crate::model::{Country, JobDetail, JobHit, Tenant};
use crate::place;
use crate::text;

const PLATFORM: &str = "greenhouse";
const BASE: &str = "https://boards-api.greenhouse.io/v1/boards";

#[derive(Debug, Deserialize)]
struct Board {
    jobs: Vec<Posting>,
}

#[derive(Debug, Deserialize)]
struct Posting {
    id: i64,
    title: String,
    absolute_url: String,
    location: Option<Spot>,
    content: Option<String>,
    first_published: Option<String>,
    updated_at: Option<String>,
    metadata: Option<Vec<Meta>>,
    departments: Option<Vec<Named>>,
}

#[derive(Debug, Deserialize)]
struct Spot {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Meta {
    name: Option<String>,
    value: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct Named {
    name: Option<String>,
}

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let url = format!("{}/{}/jobs?content=true", BASE, tenant.slug);
    let board: Board = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let mut hits: Vec<JobHit> = Vec::new();
    for posting in board.jobs {
        if !filter::coarse(&query.needle, &blob(&posting)) {
            continue;
        }
        hits.push(to_hit(tenant, &posting));
    }
    Ok(hits)
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let url = format!("{}/{}/jobs/{}", BASE, tenant.slug, job);
    let posting: Posting = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let Some(raw) = &posting.content else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "content",
        });
    };
    let where_at = spot_of(&posting);
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: posting.title.clone(),
        location: where_at.clone(),
        country: country_of(&where_at),
        url: posting.absolute_url.clone(),
        posted: stamp_of(&posting),
        department: named_first(&posting.departments),
        employment_type: meta_of(&posting, "Employment Type"),
        compensation: meta_of(&posting, "Salary Range"),
        description: text::plain(raw),
    })
}

fn to_hit(tenant: &Tenant, posting: &Posting) -> JobHit {
    let where_at = spot_of(posting);
    JobHit {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &posting.id.to_string()),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: posting.title.clone(),
        location: where_at.clone(),
        country: country_of(&where_at),
        url: posting.absolute_url.clone(),
        posted: stamp_of(posting),
        text: body_of(posting),
    }
}

fn blob(posting: &Posting) -> String {
    let Some(raw) = &posting.content else {
        return posting.title.clone();
    };
    format!("{} {}", posting.title, raw)
}

fn spot_of(posting: &Posting) -> Option<String> {
    let Some(spot) = &posting.location else {
        return None;
    };
    spot.name.clone()
}

fn country_of(where_at: &Option<String>) -> Country {
    let Some(found) = where_at else {
        return Country::Unknown;
    };
    place::country_of(found)
}

fn stamp_of(posting: &Posting) -> Option<String> {
    let Some(first) = &posting.first_published else {
        return posting.updated_at.clone();
    };
    Some(first.clone())
}

fn body_of(posting: &Posting) -> Option<String> {
    let Some(raw) = &posting.content else {
        return None;
    };
    Some(text::plain(raw))
}

fn named_first(items: &Option<Vec<Named>>) -> Option<String> {
    let Some(list) = items else {
        return None;
    };
    let Some(head) = list.first() else {
        return None;
    };
    head.name.clone()
}

fn meta_of(posting: &Posting, wanted: &str) -> Option<String> {
    let Some(items) = &posting.metadata else {
        return None;
    };
    for item in items {
        let Some(name) = &item.name else {
            continue;
        };
        if !name.eq_ignore_ascii_case(wanted) {
            continue;
        }
        let Some(value) = &item.value else {
            continue;
        };
        let Some(found) = value.as_str() else {
            continue;
        };
        return Some(found.to_string());
    }
    None
}
