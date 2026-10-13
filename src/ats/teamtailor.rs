use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::config::Config;
use crate::error::AtsError;
use crate::filter::{self, Query};
use crate::handle;
use crate::http;
use crate::model::{Country, JobDetail, JobHit, Tenant};
use crate::place;
use crate::text;

const PLATFORM: &str = "teamtailor";
const HOST: &str = "teamtailor.com";
const FIRST_PAGE: usize = 1;
const PART_JOIN: &str = ", ";
const SPOT_JOIN: &str = " | ";
const ISO_DATE_LEN: usize = 10;
const DASH_AT_YEAR: usize = 4;
const DASH_AT_MONTH: usize = 7;

#[derive(Debug, Deserialize)]
struct Feed {
    items: Vec<Item>,
}

#[derive(Debug, Deserialize)]
struct Item {
    id: Option<String>,
    title: Option<String>,
    url: Option<String>,
    date_published: Option<String>,
    content_html: Option<String>,
    #[serde(rename = "_jobposting")]
    posting: Option<Posting>,
}

#[derive(Debug, Deserialize)]
struct Posting {
    title: Option<String>,
    description: Option<String>,
    identifier: Option<Identifier>,
    #[serde(rename = "datePosted")]
    date_posted: Option<String>,
    #[serde(rename = "jobLocation")]
    job_location: Option<Vec<Spot>>,
}

#[derive(Debug, Deserialize)]
struct Identifier {
    value: Option<Value>,
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

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let ceiling = Config::new().per_tenant_cap;
    let mut hits: Vec<JobHit> = Vec::new();
    let mut page = FIRST_PAGE;
    while page <= ceiling {
        let feed = page_of(client, tenant, page).await?;
        if feed.items.is_empty() {
            break;
        }
        gather(tenant, query, &feed.items, &mut hits);
        if hits.len() >= ceiling {
            break;
        }
        page += 1;
    }
    hits.truncate(ceiling);
    Ok(hits)
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let ceiling = Config::new().per_tenant_cap;
    let mut page = FIRST_PAGE;
    while page <= ceiling {
        let feed = page_of(client, tenant, page).await?;
        if feed.items.is_empty() {
            break;
        }
        let Some(item) = find(&feed.items, job) else {
            page += 1;
            continue;
        };
        return to_detail(tenant, item, job);
    }
    Err(AtsError::NoSuchJob {
        platform: PLATFORM,
        tenant: tenant.slug.to_string(),
        job: job.to_string(),
    })
}

async fn page_of(client: &Client, tenant: &Tenant, page: usize) -> Result<Feed, AtsError> {
    let url = format!("https://{}.{}/jobs.json?page={}", tenant.slug, HOST, page);
    http::get_json(client, PLATFORM, tenant.slug, &url).await
}

fn gather(tenant: &Tenant, query: &Query, items: &[Item], hits: &mut Vec<JobHit>) {
    for item in items {
        let body = body_of(item);
        if !filter::coarse(&query.needle, &blob(item, &body)) {
            continue;
        }
        match to_hit(tenant, item, body) {
            Ok(hit) => hits.push(hit),
            Err(e) => tracing::warn!(
                error = %e,
                platform = PLATFORM,
                tenant = tenant.slug,
                "skipping unusable teamtailor posting"
            ),
        }
    }
}

fn to_hit(tenant: &Tenant, item: &Item, body: Option<String>) -> Result<JobHit, AtsError> {
    let job = job_of(tenant, item)?;
    let title = title_of(tenant, item)?;
    let link = link_of(tenant, item)?;
    let where_at = spot_of(item);
    Ok(JobHit {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title,
        location: where_at.clone(),
        country: country_of(&where_at),
        url: link,
        posted: stamp_of(item),
        text: body,
    })
}

fn to_detail(tenant: &Tenant, item: &Item, job: &str) -> Result<JobDetail, AtsError> {
    let title = title_of(tenant, item)?;
    let link = link_of(tenant, item)?;
    let Some(body) = body_of(item) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "content_html",
        });
    };
    let where_at = spot_of(item);
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title,
        location: where_at.clone(),
        country: country_of(&where_at),
        url: link,
        posted: stamp_of(item),
        department: None,
        employment_type: None,
        compensation: None,
        description: body,
    })
}

fn find<'a>(items: &'a [Item], job: &str) -> Option<&'a Item> {
    for item in items {
        let Some(found) = id_of(item) else {
            continue;
        };
        if found == job {
            return Some(item);
        }
    }
    None
}

fn job_of(tenant: &Tenant, item: &Item) -> Result<String, AtsError> {
    let Some(found) = id_of(item) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "identifier",
        });
    };
    Ok(found)
}

fn id_of(item: &Item) -> Option<String> {
    let Some(found) = stated_id(item) else {
        return clean(&item.id);
    };
    Some(found)
}

fn stated_id(item: &Item) -> Option<String> {
    let Some(posting) = &item.posting else {
        return None;
    };
    let Some(identifier) = &posting.identifier else {
        return None;
    };
    let Some(value) = &identifier.value else {
        return None;
    };
    let Some(number) = value.as_i64() else {
        let Some(spelled) = value.as_str() else {
            return None;
        };
        return trimmed(spelled);
    };
    Some(number.to_string())
}

fn title_of(tenant: &Tenant, item: &Item) -> Result<String, AtsError> {
    let Some(found) = named(item) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "title",
        });
    };
    Ok(found)
}

fn named(item: &Item) -> Option<String> {
    let Some(found) = clean(&item.title) else {
        let Some(posting) = &item.posting else {
            return None;
        };
        return clean(&posting.title);
    };
    Some(found)
}

fn link_of(tenant: &Tenant, item: &Item) -> Result<String, AtsError> {
    let Some(found) = clean(&item.url) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "url",
        });
    };
    Ok(found)
}

fn blob(item: &Item, body: &Option<String>) -> String {
    let Some(title) = named(item) else {
        let Some(found) = body else {
            return String::new();
        };
        return found.clone();
    };
    let Some(found) = body else {
        return title;
    };
    format!("{} {}", title, found)
}

fn body_of(item: &Item) -> Option<String> {
    let Some(raw) = raw_body(item) else {
        return None;
    };
    let spoken = text::plain(&raw);
    trimmed(&spoken)
}

fn raw_body(item: &Item) -> Option<String> {
    let Some(found) = clean(&item.content_html) else {
        let Some(posting) = &item.posting else {
            return None;
        };
        return clean(&posting.description);
    };
    Some(found)
}

fn stamp_of(item: &Item) -> Option<String> {
    let Some(raw) = published(item) else {
        return None;
    };
    if !looks_iso(&raw) {
        tracing::warn!(
            platform = PLATFORM,
            stamp = raw.as_str(),
            "teamtailor publish date is not iso, leaving it absent"
        );
        return None;
    }
    Some(raw)
}

fn published(item: &Item) -> Option<String> {
    let Some(found) = clean(&item.date_published) else {
        let Some(posting) = &item.posting else {
            return None;
        };
        return clean(&posting.date_posted);
    };
    Some(found)
}

fn looks_iso(raw: &str) -> bool {
    let head: String = raw.chars().take(ISO_DATE_LEN).collect();
    let bytes = head.as_bytes();
    if bytes.len() < ISO_DATE_LEN {
        return false;
    }
    bytes[DASH_AT_YEAR] == b'-' && bytes[DASH_AT_MONTH] == b'-'
}

fn spot_of(item: &Item) -> Option<String> {
    let Some(posting) = &item.posting else {
        return None;
    };
    let Some(places) = &posting.job_location else {
        return None;
    };
    let mut spots: Vec<String> = Vec::new();
    for place_at in places {
        let Some(address) = &place_at.address else {
            continue;
        };
        let spelled = address_of(address);
        if spelled.is_empty() {
            continue;
        }
        if spots.contains(&spelled) {
            continue;
        }
        spots.push(spelled);
    }
    if spots.is_empty() {
        return None;
    }
    Some(spots.join(SPOT_JOIN))
}

fn address_of(address: &Address) -> String {
    let mut parts: Vec<String> = Vec::new();
    for field in [settlement(address), clean(&address.country)] {
        let Some(found) = field else {
            continue;
        };
        if parts.contains(&found) {
            continue;
        }
        parts.push(found);
    }
    parts.join(PART_JOIN)
}

fn settlement(address: &Address) -> Option<String> {
    let Some(found) = clean(&address.locality) else {
        return clean(&address.region);
    };
    Some(found)
}

fn country_of(where_at: &Option<String>) -> Country {
    let Some(found) = where_at else {
        return Country::Unknown;
    };
    place::country_of(found)
}

fn clean(value: &Option<String>) -> Option<String> {
    let Some(found) = value else {
        return None;
    };
    trimmed(found)
}

fn trimmed(raw: &str) -> Option<String> {
    let cut = raw.trim();
    if cut.is_empty() {
        return None;
    }
    Some(cut.to_string())
}
