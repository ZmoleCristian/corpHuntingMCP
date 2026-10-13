use chrono::Duration;
use chrono::Utc;

use crate::error::QueryError;
use crate::model::{Ats, Country, JobHit};

#[derive(Debug, Clone)]
pub struct Query {
    pub needle: String,
    pub title: Option<String>,
    pub term: Option<Vec<String>>,
    pub exclude: Option<Vec<String>>,
    pub country: Option<String>,
    pub remote: Option<bool>,
    pub company: Option<String>,
    pub platforms: Vec<Ats>,
    pub fresher_than: Option<String>,
    pub limit: usize,
    pub reach: usize,
}

#[derive(Debug, Clone)]
pub struct Needles {
    pub title: Option<String>,
    pub term: Option<Vec<String>>,
    pub company: Option<String>,
}

pub fn needle_of(needles: &Needles) -> Result<String, QueryError> {
    let Some(text) = &needles.title else {
        return widest(needles);
    };
    Ok(text.clone())
}

fn widest(needles: &Needles) -> Result<String, QueryError> {
    let Some(list) = &needles.term else {
        return blank(needles);
    };
    let Some(first) = list.first() else {
        return blank(needles);
    };
    Ok(first.clone())
}

fn blank(needles: &Needles) -> Result<String, QueryError> {
    let Some(named) = &needles.company else {
        return Err(QueryError::NoNeedle);
    };
    if named.trim().is_empty() {
        return Err(QueryError::NoNeedle);
    }
    Ok(String::new())
}

pub fn cutoff(days: u32) -> String {
    let moment = Utc::now() - Duration::days(i64::from(days));
    moment.format("%Y-%m-%d").to_string()
}

pub fn coarse(needle: &str, haystack: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

pub fn keeps(query: &Query, hit: &JobHit) -> bool {
    if !title_ok(query, hit) {
        return false;
    }
    if !term_ok(query, hit) {
        return false;
    }
    if !exclude_ok(query, hit) {
        return false;
    }
    if !country_ok(query, hit) {
        return false;
    }
    if !remote_ok(query, hit) {
        return false;
    }
    if !company_ok(query, hit) {
        return false;
    }
    fresh_ok(query, hit)
}

fn title_ok(query: &Query, hit: &JobHit) -> bool {
    let Some(wanted) = &query.title else {
        return true;
    };
    mentions(&hit.title, wanted)
}

fn term_ok(query: &Query, hit: &JobHit) -> bool {
    let Some(wanted) = &query.term else {
        return true;
    };
    for needle in wanted {
        if !anywhere(hit, needle) {
            return false;
        }
    }
    true
}

fn anywhere(hit: &JobHit, wanted: &str) -> bool {
    if mentions(&hit.title, wanted) {
        return true;
    }
    let Some(body) = &hit.text else {
        return false;
    };
    mentions(body, wanted)
}

fn country_ok(query: &Query, hit: &JobHit) -> bool {
    let Some(wanted) = &query.country else {
        return true;
    };
    if remote_wins(query, hit) {
        return true;
    }
    in_country(hit, wanted)
}

fn in_country(hit: &JobHit, wanted: &str) -> bool {
    let Country::Known(code) = &hit.country else {
        return false;
    };
    code.eq_ignore_ascii_case(wanted)
}

fn remote_wins(query: &Query, hit: &JobHit) -> bool {
    let Some(wanted) = query.remote else {
        return false;
    };
    wanted && hit.remote
}

fn remote_ok(query: &Query, hit: &JobHit) -> bool {
    let Some(wanted) = query.remote else {
        return true;
    };
    if hit.remote == wanted {
        return true;
    }
    let Some(country) = &query.country else {
        return false;
    };
    wanted && in_country(hit, country)
}

fn exclude_ok(query: &Query, hit: &JobHit) -> bool {
    let Some(unwanted) = &query.exclude else {
        return true;
    };
    for needle in unwanted {
        if surfaces(hit, needle) {
            return false;
        }
    }
    true
}

fn surfaces(hit: &JobHit, wanted: &str) -> bool {
    if anywhere(hit, wanted) {
        return true;
    }
    let Some(where_at) = &hit.location else {
        return false;
    };
    mentions(where_at, wanted)
}

fn company_ok(query: &Query, hit: &JobHit) -> bool {
    let Some(wanted) = &query.company else {
        return true;
    };
    hit.company.to_lowercase().contains(&wanted.to_lowercase())
}

fn fresh_ok(query: &Query, hit: &JobHit) -> bool {
    let Some(limit) = &query.fresher_than else {
        return true;
    };
    let Some(stamp) = &hit.posted else {
        return false;
    };
    let Some(head) = iso_head(stamp) else {
        return false;
    };
    head.as_str() >= limit.as_str()
}

fn iso_head(stamp: &str) -> Option<String> {
    const ISO_DATE_LEN: usize = 10;
    const DASH_AT_YEAR: usize = 4;
    const DASH_AT_MONTH: usize = 7;
    let head: String = stamp.chars().take(ISO_DATE_LEN).collect();
    let bytes = head.as_bytes();
    if bytes.len() < ISO_DATE_LEN {
        return None;
    }
    if bytes[DASH_AT_YEAR] != b'-' || bytes[DASH_AT_MONTH] != b'-' {
        return None;
    }
    Some(head)
}

fn mentions(haystack: &str, needle: &str) -> bool {
    let lowered = haystack.to_lowercase();
    let wanted = needle.trim().to_lowercase();
    if wanted.is_empty() {
        return true;
    }
    if wanted.contains(' ') {
        return lowered.contains(&wanted);
    }
    for chunk in lowered.split(|c: char| !c.is_ascii_alphanumeric() && c != '+' && c != '#') {
        if chunk == wanted {
            return true;
        }
    }
    false
}
