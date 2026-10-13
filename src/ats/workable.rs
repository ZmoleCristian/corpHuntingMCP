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

const PLATFORM: &str = "workable";
const WIDGET: &str = "https://apply.workable.com/api/v1/widget/accounts";
const BOARD: &str = "https://apply.workable.com/api/v1/accounts";
const APPLY: &str = "https://apply.workable.com";
const REMOTE_LABEL: &str = "Remote";
const PLACE_JOIN: &str = "; ";
const PART_JOIN: &str = ", ";
const ISO_LEN: usize = 10;
const DASH_AT_YEAR: usize = 4;
const DASH_AT_MONTH: usize = 7;

const KINDS: [(&str, &str); 6] = [
    ("full", "Full-time"),
    ("part", "Part-time"),
    ("contract", "Contract"),
    ("temporary", "Temporary"),
    ("internship", "Internship"),
    ("other", "Other"),
];

#[derive(Debug, Deserialize)]
struct Account {
    jobs: Vec<Row>,
}

#[derive(Debug, Deserialize)]
struct Row {
    shortcode: String,
    title: String,
    description: Option<String>,
    published_on: Option<String>,
    created_at: Option<String>,
    city: Option<String>,
    country: Option<String>,
    telecommuting: Option<bool>,
    locations: Option<Vec<Spot>>,
}

#[derive(Debug, Deserialize)]
struct Spot {
    city: Option<String>,
    country: Option<String>,
    #[serde(rename = "countryCode")]
    country_code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Posting {
    title: String,
    description: Option<String>,
    requirements: Option<String>,
    benefits: Option<String>,
    published: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    department: Option<Vec<String>>,
    remote: Option<bool>,
    location: Option<Spot>,
    locations: Option<Vec<Spot>>,
}

struct Group {
    shortcode: String,
    title: String,
    body: Option<String>,
    posted: Option<String>,
    labels: Vec<String>,
    codes: Vec<String>,
}

pub async fn search(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    let url = format!("{}/{}?details=true", WIDGET, tenant.slug);
    let account: Account = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let ceiling = Config::new().per_tenant_cap;
    let mut groups = group(account.jobs);
    groups.sort_by(|left, right| right.posted.cmp(&left.posted));
    let mut hits: Vec<JobHit> = Vec::new();
    for found in groups {
        if hits.len() >= ceiling {
            break;
        }
        let body = plain_body(&found);
        if !filter::coarse(&query.needle, &blob(&found, &body)) {
            continue;
        }
        hits.push(to_hit(tenant, query, &found, body));
    }
    Ok(hits)
}

pub async fn detail(client: &Client, tenant: &Tenant, job: &str) -> Result<JobDetail, AtsError> {
    let url = format!("{}/{}/jobs/{}", BOARD, tenant.slug, job);
    let posting: Posting = http::get_json(client, PLATFORM, tenant.slug, &url).await?;
    let Some(description) = body_of(&posting) else {
        return Err(AtsError::Malformed {
            platform: PLATFORM,
            tenant: tenant.slug.to_string(),
            field: "description",
        });
    };
    let where_at = joined(&labels_of(&posting));
    Ok(JobDetail {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, job),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: posting.title.clone(),
        location: where_at.clone(),
        country: country_of(&codes_of(&posting), &where_at),
        url: link_of(tenant.slug, job),
        posted: iso_day(&trimmed(&posting.published)),
        department: department_of(&posting),
        employment_type: kind_of(&posting),
        compensation: None,
        description,
    })
}

fn group(rows: Vec<Row>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for row in rows {
        let Some(shortcode) = clean(&row.shortcode) else {
            tracing::warn!(
                title = row.title.as_str(),
                "workable posting carries no shortcode"
            );
            continue;
        };
        let Some(at) = seen_at(&groups, &shortcode) else {
            groups.push(fresh(shortcode, &row));
            continue;
        };
        absorb(&mut groups[at], &row);
    }
    groups
}

fn seen_at(groups: &[Group], shortcode: &str) -> Option<usize> {
    for (at, found) in groups.iter().enumerate() {
        if found.shortcode == shortcode {
            return Some(at);
        }
    }
    None
}

fn fresh(shortcode: String, row: &Row) -> Group {
    let mut found = Group {
        shortcode,
        title: row.title.clone(),
        body: row.description.clone(),
        posted: iso_day(&stamp_of(row)),
        labels: Vec::new(),
        codes: Vec::new(),
    };
    push_unique(&mut found.labels, remote_label(row.telecommuting));
    absorb(&mut found, row);
    found
}

fn absorb(found: &mut Group, row: &Row) {
    push_unique(&mut found.labels, label_of(&row.city, &row.country));
    for spots in row.locations.iter() {
        for spot in spots {
            push_unique(&mut found.labels, label_of(&spot.city, &spot.country));
            push_unique(&mut found.codes, code_of(&spot.country_code));
        }
    }
}

fn to_hit(tenant: &Tenant, query: &Query, found: &Group, body: Option<String>) -> JobHit {
    let where_at = joined(&found.labels);
    JobHit {
        remote: false,
        id: handle::encode(tenant.ats, tenant.slug, &found.shortcode),
        ats: tenant.ats,
        company: tenant.company.to_string(),
        title: found.title.clone(),
        location: where_at.clone(),
        country: asked_country(&found.codes, &where_at, &query.country),
        url: link_of(tenant.slug, &found.shortcode),
        posted: found.posted.clone(),
        text: body,
    }
}

fn blob(found: &Group, body: &Option<String>) -> String {
    let Some(said) = body else {
        return found.title.clone();
    };
    format!("{} {}", found.title, said)
}

fn plain_body(found: &Group) -> Option<String> {
    let Some(raw) = &found.body else {
        return None;
    };
    clean(&text::plain(raw))
}

fn body_of(posting: &Posting) -> Option<String> {
    let Some(raw) = &posting.description else {
        return None;
    };
    let mut body = text::plain(raw);
    append(&mut body, &posting.requirements);
    append(&mut body, &posting.benefits);
    clean(&body)
}

fn append(body: &mut String, extra: &Option<String>) {
    for raw in extra.iter() {
        let Some(said) = clean(&text::plain(raw)) else {
            continue;
        };
        body.push('\n');
        body.push_str(&said);
    }
}

fn labels_of(posting: &Posting) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    push_unique(&mut labels, remote_label(posting.remote));
    for spots in posting.locations.iter() {
        for spot in spots {
            push_unique(&mut labels, label_of(&spot.city, &spot.country));
        }
    }
    for spot in posting.location.iter() {
        push_unique(&mut labels, label_of(&spot.city, &spot.country));
    }
    labels
}

fn codes_of(posting: &Posting) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for spots in posting.locations.iter() {
        for spot in spots {
            push_unique(&mut codes, code_of(&spot.country_code));
        }
    }
    for spot in posting.location.iter() {
        push_unique(&mut codes, code_of(&spot.country_code));
    }
    codes
}

fn department_of(posting: &Posting) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    for listed in posting.department.iter() {
        for name in listed {
            push_unique(&mut names, clean(name));
        }
    }
    if names.is_empty() {
        return None;
    }
    Some(names.join(PART_JOIN))
}

fn kind_of(posting: &Posting) -> Option<String> {
    let Some(raw) = trimmed(&posting.kind) else {
        return None;
    };
    for pair in KINDS {
        if raw.eq_ignore_ascii_case(pair.0) {
            return Some(pair.1.to_string());
        }
    }
    Some(raw)
}

fn push_unique(into: &mut Vec<String>, value: Option<String>) {
    let Some(said) = value else {
        return;
    };
    if into.contains(&said) {
        return;
    }
    into.push(said);
}

fn label_of(city: &Option<String>, country: &Option<String>) -> Option<String> {
    let town = trimmed(city);
    let land = trimmed(country);
    let Some(here) = town else {
        return land;
    };
    let Some(nation) = land else {
        return Some(here);
    };
    Some(format!("{}{}{}", here, PART_JOIN, nation))
}

fn code_of(country_code: &Option<String>) -> Option<String> {
    let Some(code) = trimmed(country_code) else {
        return None;
    };
    Some(code.to_uppercase())
}

fn remote_label(telecommuting: Option<bool>) -> Option<String> {
    let Some(afar) = telecommuting else {
        return None;
    };
    if !afar {
        return None;
    }
    Some(REMOTE_LABEL.to_string())
}

fn joined(labels: &[String]) -> Option<String> {
    if labels.is_empty() {
        return None;
    }
    Some(labels.join(PLACE_JOIN))
}

fn asked_country(codes: &[String], where_at: &Option<String>, wanted: &Option<String>) -> Country {
    let Some(asked) = wanted else {
        return country_of(codes, where_at);
    };
    for code in codes {
        if code.eq_ignore_ascii_case(asked) {
            return Country::Known(code.clone());
        }
    }
    country_of(codes, where_at)
}

fn country_of(codes: &[String], where_at: &Option<String>) -> Country {
    let Some(code) = codes.first() else {
        let Some(said) = where_at else {
            return Country::Unknown;
        };
        return place::country_of(said);
    };
    Country::Known(code.clone())
}

fn link_of(slug: &str, shortcode: &str) -> String {
    format!("{}/{}/j/{}", APPLY, slug, shortcode)
}

fn stamp_of(row: &Row) -> Option<String> {
    let Some(published) = trimmed(&row.published_on) else {
        return trimmed(&row.created_at);
    };
    Some(published)
}

fn iso_day(stamp: &Option<String>) -> Option<String> {
    let Some(raw) = stamp else {
        return None;
    };
    let head: String = raw.chars().take(ISO_LEN).collect();
    if !looks_iso(&head) {
        tracing::warn!(stamp = raw.as_str(), "workable posting carries an unreadable date");
        return None;
    }
    Some(head)
}

fn looks_iso(head: &str) -> bool {
    let bytes = head.as_bytes();
    if bytes.len() != ISO_LEN {
        return false;
    }
    for (at, byte) in bytes.iter().enumerate() {
        let dashed = at == DASH_AT_YEAR || at == DASH_AT_MONTH;
        if dashed != (*byte == b'-') {
            return false;
        }
        if !dashed && !byte.is_ascii_digit() {
            return false;
        }
    }
    true
}

fn trimmed(value: &Option<String>) -> Option<String> {
    let Some(said) = value else {
        return None;
    };
    clean(said)
}

fn clean(said: &str) -> Option<String> {
    let cut = said.trim();
    if cut.is_empty() {
        return None;
    }
    Some(cut.to_string())
}
