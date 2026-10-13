use std::time::Duration;

use moka::sync::Cache;

use crate::model::{Ats, JobHit};

const SEPARATOR: char = '\t';

pub enum Board {
    Fresh(Vec<JobHit>),
    Failed,
    Cold,
}

#[derive(Clone)]
enum Entry {
    Hits(Vec<JobHit>),
    Miss,
}

#[derive(Clone)]
pub struct BoardCache {
    inner: Cache<String, Entry>,
}

impl BoardCache {
    pub fn new(ttl: Duration) -> BoardCache {
        BoardCache {
            inner: Cache::builder().time_to_live(ttl).build(),
        }
    }

    pub fn read(&self, key: &str) -> Board {
        let Some(found) = self.inner.get(key) else {
            return Board::Cold;
        };
        match found {
            Entry::Hits(hits) => Board::Fresh(hits),
            Entry::Miss => Board::Failed,
        }
    }

    pub fn write(&self, key: String, hits: Vec<JobHit>) {
        self.inner.insert(key, Entry::Hits(hits));
    }

    pub fn write_miss(&self, key: String) {
        self.inner.insert(key, Entry::Miss);
    }
}

pub fn key(ats: Ats, tenant: &str, needle: &str) -> String {
    format!(
        "{}{}{}{}{}",
        ats.label(),
        SEPARATOR,
        tenant,
        SEPARATOR,
        needle
    )
}
