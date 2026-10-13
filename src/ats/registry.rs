use futures::future::join_all;
use reqwest::Client;
use tokio::time::sleep;

use crate::ats::{
    ashby, bamboohr, greenhouse, jobvite, lever, recruitee, smartrecruiters, teamtailor, workable,
    workday,
};
use crate::cache::{self, Board, BoardCache};
use crate::config::Config;
use crate::error::AtsError;
use crate::filter::{self, Query};
use crate::handle::Handle;
use crate::model::{Ats, JobDetail, JobHit, PLATFORMS, Sweep, Tenant};
use crate::place;
use crate::tenants::TABLES;

pub async fn search_all(
    client: &Client,
    query: &Query,
    config: &Config,
    boards: &BoardCache,
) -> Sweep {
    let plans = plans_for(query);
    let mut scanned: usize = 0;
    let mut known: usize = 0;
    let mut runs = Vec::new();
    for plan in plans {
        known += plan.known;
        scanned += plan.tenants.len();
        runs.push(sweep_one(client, plan, query, config, boards));
    }
    tracing::info!(scanned, known, needle = query.needle.as_str(), "fanning out");

    let mut kept: Vec<JobHit> = Vec::new();
    for mut batch in join_all(runs).await {
        kept.append(&mut batch);
    }
    kept.sort_by(|left, right| right.posted.cmp(&left.posted));
    let found = kept.len();
    let ceiling = match query.limit < config.hit_cap {
        true => query.limit,
        false => config.hit_cap,
    };
    kept.truncate(ceiling);
    Sweep {
        found,
        scanned,
        known,
        hits: kept,
    }
}

struct Plan {
    tenants: Vec<Tenant>,
    known: usize,
}

async fn sweep_one(
    client: &Client,
    plan: Plan,
    query: &Query,
    config: &Config,
    boards: &BoardCache,
) -> Vec<JobHit> {
    let mut kept: Vec<JobHit> = Vec::new();
    let mut cold: Vec<Tenant> = Vec::new();
    for tenant in &plan.tenants {
        let slot = cache::key(tenant.ats, tenant.slug, &query.needle);
        match boards.read(&slot) {
            Board::Fresh(hits) => harvest(query, hits, &mut kept),
            Board::Failed => tracing::debug!(tenant = tenant.slug, "board still failing, not retried"),
            Board::Cold => cold.push(*tenant),
        }
    }
    for (at, chunk) in cold.chunks(config.concurrency).enumerate() {
        if at > 0 {
            sleep(config.batch_pause).await;
        }
        let work = chunk
            .iter()
            .map(|tenant| ask(client, tenant, query, boards));
        for (tenant, outcome) in join_all(work).await {
            match outcome {
                Ok(found) => harvest(query, found, &mut kept),
                Err(e) => tracing::warn!(
                    error = %e,
                    platform = tenant.ats.label(),
                    tenant = tenant.slug,
                    "tenant search failed"
                ),
            }
        }
    }
    kept
}

async fn ask(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
    boards: &BoardCache,
) -> (Tenant, Result<Vec<JobHit>, AtsError>) {
    let slot = cache::key(tenant.ats, tenant.slug, &query.needle);
    let outcome = search_one(client, tenant, query).await;
    match &outcome {
        Ok(hits) => boards.write(slot, hits.clone()),
        Err(e) => {
            tracing::debug!(error = %e, tenant = tenant.slug, "board marked as failing");
            boards.write_miss(slot);
        }
    }
    (*tenant, outcome)
}

pub async fn detail(client: &Client, target: &Handle) -> Result<JobDetail, AtsError> {
    let mut job = fetched(client, target).await?;
    job.remote = remote_of(&job.location);
    Ok(job)
}

async fn fetched(client: &Client, target: &Handle) -> Result<JobDetail, AtsError> {
    let tenant = tenant_of(target)?;
    match tenant.ats {
        Ats::Greenhouse => greenhouse::detail(client, &tenant, &target.job).await,
        Ats::Lever => lever::detail(client, &tenant, &target.job).await,
        Ats::Ashby => ashby::detail(client, &tenant, &target.job).await,
        Ats::Workable => workable::detail(client, &tenant, &target.job).await,
        Ats::Recruitee => recruitee::detail(client, &tenant, &target.job).await,
        Ats::SmartRecruiters => smartrecruiters::detail(client, &tenant, &target.job).await,
        Ats::Workday => workday::detail(client, &tenant, &target.job).await,
        Ats::BambooHr => bamboohr::detail(client, &tenant, &target.job).await,
        Ats::Teamtailor => teamtailor::detail(client, &tenant, &target.job).await,
        Ats::Jobvite => jobvite::detail(client, &tenant, &target.job).await,
    }
}

async fn search_one(
    client: &Client,
    tenant: &Tenant,
    query: &Query,
) -> Result<Vec<JobHit>, AtsError> {
    match tenant.ats {
        Ats::Greenhouse => greenhouse::search(client, tenant, query).await,
        Ats::Lever => lever::search(client, tenant, query).await,
        Ats::Ashby => ashby::search(client, tenant, query).await,
        Ats::Workable => workable::search(client, tenant, query).await,
        Ats::Recruitee => recruitee::search(client, tenant, query).await,
        Ats::SmartRecruiters => smartrecruiters::search(client, tenant, query).await,
        Ats::Workday => workday::search(client, tenant, query).await,
        Ats::BambooHr => bamboohr::search(client, tenant, query).await,
        Ats::Teamtailor => teamtailor::search(client, tenant, query).await,
        Ats::Jobvite => jobvite::search(client, tenant, query).await,
    }
}

fn harvest(query: &Query, found: Vec<JobHit>, kept: &mut Vec<JobHit>) {
    for mut hit in found {
        hit.remote = remote_of(&hit.location);
        if filter::keeps(query, &hit) {
            kept.push(hit);
        }
    }
}

fn remote_of(location: &Option<String>) -> bool {
    let Some(text) = location else {
        return false;
    };
    place::is_remote(text)
}

fn plans_for(query: &Query) -> Vec<Plan> {
    let mut plans: Vec<Plan> = Vec::new();
    for platform in PLATFORMS {
        if !platform_wanted(query, platform) {
            continue;
        }
        let mut picked: Vec<Tenant> = Vec::new();
        for table in TABLES {
            for tenant in table {
                if tenant.ats != platform {
                    continue;
                }
                if !company_wanted(query, tenant) {
                    continue;
                }
                picked.push(*tenant);
            }
        }
        if picked.is_empty() {
            continue;
        }
        let known = picked.len();
        if bounded(query) {
            picked.truncate(depth(query));
        }
        plans.push(Plan {
            tenants: picked,
            known,
        });
    }
    plans
}

fn depth(query: &Query) -> usize {
    match query.reach {
        0 => usize::MAX,
        asked => asked,
    }
}

fn bounded(query: &Query) -> bool {
    let Some(named) = &query.company else {
        return true;
    };
    named.trim().is_empty()
}

fn platform_wanted(query: &Query, platform: Ats) -> bool {
    if query.platforms.is_empty() {
        return true;
    }
    query.platforms.contains(&platform)
}

fn company_wanted(query: &Query, tenant: &Tenant) -> bool {
    let Some(wanted) = &query.company else {
        return true;
    };
    let needle = wanted.to_lowercase();
    if tenant.company.to_lowercase().contains(&needle) {
        return true;
    }
    tenant.slug.to_lowercase().contains(&needle)
}

fn tenant_of(target: &Handle) -> Result<Tenant, AtsError> {
    for table in TABLES {
        for tenant in table {
            if tenant.ats == target.ats && tenant.slug == target.tenant {
                return Ok(*tenant);
            }
        }
    }
    Err(AtsError::NoSuchTenant {
        platform: target.ats.label(),
        tenant: target.tenant.clone(),
    })
}
