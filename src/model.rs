use serde::{Serialize, Serializer};

use crate::error::QueryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ats {
    Greenhouse,
    Lever,
    Ashby,
    Workable,
    Recruitee,
    SmartRecruiters,
    Workday,
    BambooHr,
    Teamtailor,
    Jobvite,
}

pub const PLATFORMS: [Ats; 10] = [
    Ats::Greenhouse,
    Ats::Lever,
    Ats::Ashby,
    Ats::Workable,
    Ats::Recruitee,
    Ats::SmartRecruiters,
    Ats::Workday,
    Ats::BambooHr,
    Ats::Teamtailor,
    Ats::Jobvite,
];

impl Ats {
    pub fn label(self) -> &'static str {
        match self {
            Ats::Greenhouse => "greenhouse",
            Ats::Lever => "lever",
            Ats::Ashby => "ashby",
            Ats::Workable => "workable",
            Ats::Recruitee => "recruitee",
            Ats::SmartRecruiters => "smartrecruiters",
            Ats::Workday => "workday",
            Ats::BambooHr => "bamboohr",
            Ats::Teamtailor => "teamtailor",
            Ats::Jobvite => "jobvite",
        }
    }

    pub fn parse(text: &str) -> Result<Ats, QueryError> {
        for platform in PLATFORMS {
            if platform.label().eq_ignore_ascii_case(text) {
                return Ok(platform);
            }
        }
        Err(QueryError::Platform {
            platform: text.to_string(),
        })
    }
}

impl Serialize for Ats {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.label())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Tenant {
    pub ats: Ats,
    pub slug: &'static str,
    pub company: &'static str,
    pub host: &'static str,
    pub site: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Country {
    Known(String),
    Unknown,
}

impl Serialize for Country {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Country::Known(code) => serializer.serialize_str(code),
            Country::Unknown => serializer.serialize_none(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JobHit {
    pub id: String,
    pub ats: Ats,
    pub company: String,
    pub title: String,
    pub location: Option<String>,
    pub country: Country,
    pub remote: bool,
    pub url: String,
    pub posted: Option<String>,
    #[serde(skip_serializing)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sweep {
    pub found: usize,
    pub scanned: usize,
    pub known: usize,
    pub hits: Vec<JobHit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobDetail {
    pub id: String,
    pub ats: Ats,
    pub company: String,
    pub title: String,
    pub location: Option<String>,
    pub country: Country,
    pub remote: bool,
    pub url: String,
    pub posted: Option<String>,
    pub department: Option<String>,
    pub employment_type: Option<String>,
    pub compensation: Option<String>,
    pub description: String,
}
