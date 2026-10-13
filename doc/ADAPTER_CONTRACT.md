# ATS adapter contract

One file per platform: `src/ats/<platform>.rs`. **Touch nothing else.** Every shared
type, helper and the dispatch table already exist and are final.

## The two functions your file must expose

```rust
pub async fn search(client: &Client, tenant: &Tenant, query: &Query) -> Result<Vec<JobHit>, AtsError>
pub async fn detail(client: &Client, tenant: &Tenant, job: &str)     -> Result<JobDetail, AtsError>
```

Nothing else in your file is `pub`.

## What you are given

`crate::model`
```rust
pub struct Tenant { pub ats: Ats, pub slug: &'static str, pub company: &'static str,
                    pub host: &'static str, pub site: &'static str }
pub enum Country { Known(String), Unknown }

pub struct JobHit {
    pub id: String, pub ats: Ats, pub company: String, pub title: String,
    pub location: Option<String>, pub country: Country, pub remote: bool,
    pub url: String, pub posted: Option<String>, pub text: Option<String>,
}
pub struct JobDetail {
    pub id: String, pub ats: Ats, pub company: String, pub title: String,
    pub location: Option<String>, pub country: Country, pub remote: bool,
    pub url: String, pub posted: Option<String>, pub department: Option<String>,
    pub employment_type: Option<String>, pub compensation: Option<String>,
    pub description: String,
}
```

`crate::http` — every HTTP call goes through these; they map status/decode/parse to `AtsError`:
```rust
http::get_json::<T>(client, PLATFORM, tenant.slug, &url).await?
http::post_json::<B, T>(client, PLATFORM, tenant.slug, &url, &body).await?
http::get_text(client, PLATFORM, tenant.slug, &url).await?
```

`crate::handle::encode(tenant.ats, tenant.slug, job_id) -> String` — the opaque id.
`crate::place::country_of(text) -> Country` — location string to ISO-2.
`crate::text::plain(html) -> String` — HTML (entity-encoded or not) to plain text.
`crate::filter::coarse(&query.needle, haystack) -> bool` — cheap pre-filter.

`crate::error::AtsError` variants you may raise: `Malformed { platform, tenant, field }`
and `NoSuchJob { platform, tenant, job }`. The rest are raised by `http` for you.

## Hard rules of the contract

1. `id` is always `handle::encode(tenant.ats, tenant.slug, <the platform's own job id>)`.
   `detail` must be able to fetch the job from that id alone.
2. `company` is always `tenant.company.to_string()` — never the board's own name field.
   The tenant table is the single source of truth for who the employer is.
3. `posted` is **ISO**: `YYYY-MM-DD` or a full RFC3339 stamp starting with it, or `None`.
   Only the first ten characters are ever compared. If the board hands you
   `"Posted 3 Days Ago"` or a unix epoch, convert it. `chrono` is a dependency.
4. `text` on a `JobHit` is the plain-text posting body when the list endpoint carries it,
   `None` when it does not. It is what `term=` searches; never put HTML in it.
5. `description` on a `JobDetail` is required. If the board gives you no body, return
   `AtsError::Malformed { field: "..." }`.
6. `remote` is not yours to fill. Set it `false` and let `ats::registry` derive it from
   `location`, so the rule lives in one place instead of ten. `country` is yours, because
   several boards publish an explicit code that beats anything parsed out of the label.
7. `search` must honour `query.needle` — push it to the server as a search parameter where
   the API supports one, and always run `filter::coarse(&query.needle, ...)` before building
   a `JobHit`. All the other filters are applied for you afterwards; do not reimplement them.
8. Paginate if the API pages. Cap what you pull at `crate::config::Config::per_tenant_cap`
   if you need a ceiling — take it as data, do not invent a number outside `config.rs`.
9. Never swallow a failure. One bad posting in a list is skipped with
   `tracing::warn!(error = %e, ...)` and a `continue`; a failed HTTP call propagates.
10. **One `search` call is one board fetch, or as few as the API allows.** The registry
   already paces you — four boards at a time per platform with a second between batches —
   and memoises the result, hits and failures alike. Do not add a cache, do not add a
   sleep, and above all do not fan out per posting: a detail call per row would multiply
   the whole platform's request count by fifty and earn the user an IP block. Every
   platform meters by client IP across all its boards, including the ones that give each
   tenant its own subdomain. Workable hands out a 24-hour block at roughly 850 requests.
   If the only way to fill a field is a second request per posting, leave the field
   `None` and say so in your report.

## The law

`lawkeeper` judges every `.rs` file under `src/` and **blocks the edit** on violation.
Run `lawkeeper src/ats/<yours>.rs` until it is silent. `lawkeeper --rules` prints them all.
The ones that bite hardest here:

- **No comments.** Not `//`, not `///`, not `/* */`. Name things instead.
- **No** `unwrap` `expect` `unwrap_or` `unwrap_or_default` `.ok()` `.err()` `is_some()`
  `is_none()` `is_ok()` `matches!` `let _ =` `if let Some/Ok` `while let Some/Ok`.
  Use `?`, a `match` whose every arm binds and discharges, or
  `let Some(x) = expr else { return Err(...) };`
- **No** `_` as a match arm on an Option/Result, no `_` in a closure parameter list,
  no `Ok(_)` / `Err(_)` / `Some(_)`.
- **No** `.flatten()` `.filter_map()` `.flat_map()` anywhere. Write the `for` loop.
- **No** `as` casts. `i64::from(x)`, or `u32::try_from(x)` + `match`.
- **No** `wrapping_*` `saturating_*` `to_string_lossy` `from_utf8_lossy`.
- **No** `Option` in the arguments or return type of a `pub fn`. Private fns are exempt,
  struct fields are exempt.
- **No** glob imports, no `todo!`/`unimplemented!`/`unreachable!`, no `println!`/`dbg!`,
  no `#[test]` or `#[cfg(test)]` under `src/`, no `#[allow(...)]`.
- **No** `crate::`/`self::`/`super::` path outside a `use` line.
- An absence must never become a value outside `config.rs`. `let Some(x) = e else { return None };`
  in a private fn is the legal shape; `x.unwrap_or(y)` never is.
- File ≤ 800 lines, function ≤ 80 lines.

`src/ats/greenhouse.rs` is the worked reference. Copy its shape.
