use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::error::AtsError;
use crate::filter::{self, Query};
use crate::handle;
use crate::http;
use crate::model::{Country, JobDetail, JobHit, Tenant};
use crate::place;
use crate::text;

const PLATFORM: &str = "recruitee";
const HOST: &str = "recruitee.com";
const PUBLISHED: &str = "published";
const WALL_CLOCK: &str = "%Y-%m-%d %H:%M:%S UTC";
const ISO_DATE_LEN: usize = 10;
const DASH_AT_YEAR: usize = 4;
const DASH_AT_MONTH: usize = 7;

#[derive(Debug, Deserialize)]
struct Board {
    offers: Vec<Offer>,
}

#[derive(Debug, Deserialize)]
struct Single {
    offer: Offer,
}

#[derive(Debug, Deserialize)]
struct Offer {
    id: i64,
    title: String,
    slug: Option<String>,
    status: Option<String>,
    location: Option<String>,
    city: Option<String>,
    country: Option<String>,
    country_code: Option<String>,
    department: Option<String>,
    employment_type_code: Option<String>,
    employment_type: Option<String>,
    description: Option<String>,
    requirements: Option<String>,
    published_at: Option<String>,
    created_at: Option<String>,
    careers_url: Option<String>,
    salary: Option<Salary>,
}

#[derive(Debug, Deserialize)]
struct Salary {
    currency: Option<String>,
    period: Option<String>,
    min: Option<Amount>,
    max: Option<Amount>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Amount {
    Whole(i64),
    Fraction(f64),
    Written(String),
}

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let url = format!("https://{}.{}/api/offers/", tenant.slug, HOST);
    let board: Board = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let mut hits: Vec<JobHit> = Vec::new();
    for offer in board.offers {
        if !live(&offer) {
            continue;
        }
        let body = body_of(&offer);
        if !filter::coarse(&query.needle, &blob(&offer, &body)) {
            continue;
        }
        hits.push(to_hit(tenant, &offer, body));
    }
    Ok(hits)
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let url = format!("https://{}.{}/api/offers/{}", tenant.slug, HOST, job);
    let carrier: Single = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let offer = carrier.offer;
    let Some(body) = body_of(&offer) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "description",
        });
    };
    let where_at = spot_of(&offer);
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &offer.id.to_string()),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: offer.title.clone(),
        location: where_at.clone(),
        country: country_of(&offer, &where_at),
        url: link_of(tenant, &offer),
        posted: stamp_of(&offer),
        department: trimmed(&offer.department),
        employment_type: kind_of(&offer),
        compensation: pay_of(&offer),
        description: body,
    })
}

fn to_hit(tenant: &Tenant, offer: &Offer, body: Option<String>) -> JobHit {
    let where_at = spot_of(offer);
    JobHit {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &offer.id.to_string()),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: offer.title.clone(),
        location: where_at.clone(),
        country: country_of(offer, &where_at),
        url: link_of(tenant, offer),
        posted: stamp_of(offer),
        text: body,
    }
}

fn live(offer: &Offer) -> bool {
    let Some(status) = trimmed(&offer.status) else {
        return true;
    };
    status.eq_ignore_ascii_case(PUBLISHED)
}

fn blob(offer: &Offer, body: &Option<String>) -> String {
    let Some(found) = body else {
        return offer.title.clone();
    };
    format!("{} {}", offer.title, found)
}

fn body_of(offer: &Offer) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    gather(&mut parts, &offer.description);
    gather(&mut parts, &offer.requirements);
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("\n"))
}

fn gather(parts: &mut Vec<String>, raw: &Option<String>) {
    let Some(found) = trimmed(raw) else {
        return;
    };
    let rendered = text::plain(&found);
    if rendered.is_empty() {
        return;
    }
    parts.push(rendered);
}

fn spot_of(offer: &Offer) -> Option<String> {
    let Some(where_at) = trimmed(&offer.location) else {
        return city_of(offer);
    };
    Some(where_at)
}

fn city_of(offer: &Offer) -> Option<String> {
    let Some(city) = trimmed(&offer.city) else {
        return trimmed(&offer.country);
    };
    let Some(land) = trimmed(&offer.country) else {
        return Some(city);
    };
    Some(format!("{}, {}", city, land))
}

fn country_of(offer: &Offer, where_at: &Option<String>) -> Country {
    let Some(code) = trimmed(&offer.country_code) else {
        return named_country(offer, where_at);
    };
    Country::Known(code.to_ascii_uppercase())
}

fn named_country(offer: &Offer, where_at: &Option<String>) -> Country {
    let Some(land) = trimmed(&offer.country) else {
        return spot_country(where_at);
    };
    let Country::Known(code) = place::country_of(&land) else {
        return spot_country(where_at);
    };
    Country::Known(code)
}

fn spot_country(where_at: &Option<String>) -> Country {
    let Some(found) = where_at else {
        return Country::Unknown;
    };
    place::country_of(found)
}

fn kind_of(offer: &Offer) -> Option<String> {
    let Some(code) = trimmed(&offer.employment_type_code) else {
        return trimmed(&offer.employment_type);
    };
    Some(code)
}

fn link_of(tenant: &Tenant, offer: &Offer) -> String {
    let Some(url) = trimmed(&offer.careers_url) else {
        return format!("https://{}.{}/o/{}", tenant.slug, HOST, path_of(offer));
    };
    url
}

fn path_of(offer: &Offer) -> String {
    let Some(path) = trimmed(&offer.slug) else {
        return offer.id.to_string();
    };
    path
}

fn stamp_of(offer: &Offer) -> Option<String> {
    let Some(raw) = trimmed(&offer.published_at) else {
        let Some(born) = trimmed(&offer.created_at) else {
            return None;
        };
        return iso_of(&born);
    };
    iso_of(&raw)
}

fn iso_of(raw: &str) -> Option<String> {
    match NaiveDateTime::parse_from_str(raw, WALL_CLOCK) {
        Ok(naive) => Some(Utc.from_utc_datetime(&naive).to_rfc3339()),
        Err(e) => {
            tracing::debug!(error = %e, stamp = raw, "recruitee stamp is not a utc wall clock");
            offset_of(raw)
        }
    }
}

fn offset_of(raw: &str) -> Option<String> {
    match DateTime::parse_from_rfc3339(raw) {
        Ok(moment) => Some(moment.with_timezone(&Utc).to_rfc3339()),
        Err(e) => {
            tracing::warn!(error = %e, stamp = raw, "recruitee stamp is not a timestamp");
            day_of(raw)
        }
    }
}

fn day_of(raw: &str) -> Option<String> {
    let head: String = raw.chars().take(ISO_DATE_LEN).collect();
    let bytes = head.as_bytes();
    if bytes.len() < ISO_DATE_LEN {
        return None;
    }
    if bytes[DASH_AT_YEAR] != b'-' || bytes[DASH_AT_MONTH] != b'-' {
        return None;
    }
    Some(head)
}

fn pay_of(offer: &Offer) -> Option<String> {
    let Some(salary) = &offer.salary else {
        return None;
    };
    let Some(currency) = trimmed(&salary.currency) else {
        return None;
    };
    let Some(range) = range_of(salary) else {
        return None;
    };
    let money = format!("{} {}", range, currency.to_ascii_uppercase());
    let Some(period) = trimmed(&salary.period) else {
        return Some(money);
    };
    Some(format!("{} / {}", money, period))
}

fn range_of(salary: &Salary) -> Option<String> {
    let Some(low) = amount_of(&salary.min) else {
        let Some(high) = amount_of(&salary.max) else {
            return None;
        };
        return Some(format!("up to {}", high));
    };
    let Some(high) = amount_of(&salary.max) else {
        return Some(format!("from {}", low));
    };
    Some(format!("{}-{}", low, high))
}

fn amount_of(value: &Option<Amount>) -> Option<String> {
    let Some(found) = value else {
        return None;
    };
    match found {
        Amount::Whole(number) => Some(number.to_string()),
        Amount::Fraction(number) => Some(number.to_string()),
        Amount::Written(written) => {
            let cut = written.trim();
            if cut.is_empty() {
                return None;
            }
            Some(cut.to_string())
        }
    }
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
