# corpHuntingMCP

An MCP server that searches job postings across every known ATS platform at once.
Rust, `rmcp` 0.8, stdio transport. Two real tools, an async twin of the first, plus a
roster tool. No database, no config file, no API keys — coverage is a `const` table in
the binary.

## Layout

```
src/
  main.rs        bin root: deputy attrs, argv, tracing, rmcp stdio serve
  config.rs      THE SANCTUM — every default in the program lives here, and nowhere else
  error.rs       AtsError / HandleError / QueryError / AppError
  model.rs       Ats, Tenant, Country, JobHit, JobDetail
  cache.rs       BoardCache — per-board TTL memo, hits and failures alike
  tenants/       the coverage table, one module per platform, plus TABLES
  handle.rs      the opaque job id: base64url(ats \t tenant \t job_id)
  filter.rs      Query + every predicate; one place, applied once
  place.rs       location text -> ISO-2 country, remote detection
  text.rs        HTML (entity-encoded or not) -> plain text
  http.rs        get_json / post_json / get_text, mapping status/decode/parse to AtsError
  ats/
    mod.rs       wiring only
    registry.rs  fan-out, concurrency, per-tenant error isolation, dispatch by enum
    <ten adapters, one file each>
  mcp/
    mod.rs       wiring only
    server.rs    rmcp tool definitions and argument plumbing
```

The import graph is declared in `law.toml` under `[layers]` and enforced. It is a DAG:
`error`/`config`/`text` at the bottom, then `model`, then
`place`/`handle`/`filter`/`http`/`tenants`/`cache`, then `ats`, then `mcp`, then `main`.

## The shape

`search_jobs` builds a `Query`, `ats::search_all` groups the surviving tenants into one
`Plan` per platform and sweeps the plans concurrently. Inside a plan the boards go
`config.concurrency` at a time with `config.batch_pause` between batches — platforms run
in parallel, tenants within a platform do not. Each adapter fetches and normalises;
`filter::keeps` is the single authority on what matches. A tenant that fails is a
`tracing::warn!` and a skip, never a fatal.

`search_jobs_async` is the same call with the result written to a file instead of
returned: it builds the `Query`, computes `config::result_file(config::stamp())`,
`tokio::spawn`s the sweep and returns the path at once. The path is derived before the
work starts, so nothing has to be polled or looked up — the filesystem is the ticket and
the server keeps no state for it. The file goes under `config::out_dir()`, which is
`$CORPHUNT_OUT` or `std::env::temp_dir().join("corphuntingmcp")` — `temp_dir` is the one
directory API that is already correct on Linux, macOS and Windows without a `cfg`. It is
not XDG on purpose: a sweep result is regenerable and short-lived, so it is neither
config nor data. The spawned task lives in the server process and dies with it.

`filter::needle_of` accepts `company` as a needle of its own, returning an empty string
that `coarse` treats as "everything". Company-only search is the most common question
there is ("what does X have open"), and it is cheap because `plans_for` filters the
tenant table by company *before* any fetch — one board, not twenty thousand.

`remote` on a `JobHit` is derived in `registry::harvest`, and on a `JobDetail` in
`registry::detail` — adapters set `false` and are told so by the contract. It comes from
`location` alone, so one derivation site beats ten copies. `country` stays adapter-owned
because several boards publish an explicit code that beats any parse of the label.

The two are separate facts about one string: `"Remote; Argentina"` is remote *and* AR, so
a strict `country` test loses it. `filter::country_ok` therefore passes any hit where
`remote_wins` holds — the caller asked `remote: true` and the hit is remote — which makes
`country: "RO", remote: true` read as "in Romania or remote anywhere".

## Rate limits are the binding constraint

This is the thing to understand before touching `config.rs`. Every platform meters by
client IP across all its boards — including the ones that give each tenant its own
subdomain, because those sit behind one shared edge. Measured the hard way while building
the tenant table:

| profile | outcome |
|---|---|
| 4 concurrent, 1s between batches | 7 700 requests to Recruitee, 7 500 to Teamtailor, clean |
| 6 concurrent, 400ms | Recruitee starts returning 429 |
| 24 concurrent, no pause | Teamtailor 403s 1 701 of 2 773 boards in 78 seconds |
| ~850 requests, any pacing | Workable returns a 24-hour IP block |

Workable meters a cumulative daily budget, not a rate. The defaults in `config.rs` are
the profile that survived; raising them is a decision to lock the user out of the boards
they are trying to apply through, which is worse than any latency.

`reach` (default 50 per platform) is the other half of the same guard: it bounds how
many boards a single blind search can touch. It is also bounded from above by the client:
a cold sweep runs about a second per employer per platform, and MCP clients give up at two
minutes, which is what pinned the default at 50 rather than 100. A `company` filter bypasses it — naming an
employer always reaches them.

Be honest about what that costs. The table holds ~20 000 employers, so a default search
asks 2.5% of it, and the slice it asks is whichever rows landed first in the harvest, not a
ranking — only `recruitee` and `teamtailor` are sorted by live job count. A blind sweep
is therefore a sample, and both the tool description and the result line say so. Sorting
the other eight tables by job count is the obvious next improvement, and it needs a
one-off probe of every board to collect the counts, not a change in code.

## The cache

`BoardCache` memoises per `(platform, tenant, needle)` for `config.board_ttl`, and
memoises **failures** as well as hits. Failures matter more: without them a 403 storm is
re-run on every search. Cached tenants skip the pacing entirely, so a warm search is
~2s against ~70s cold.

It is a latency and request-count optimisation, not protocol state — `get_job` still
needs nothing but the id.

Adapters never filter and never cache. They fetch, normalise, and hand back `JobHit`s.
Server-side search pushdown (Workday's `searchText`, SmartRecruiters' `q=`) is an
optimisation on top of `query.needle`; `filter::keeps` still decides.

State is the id and nothing else. `handle::encode` puts the platform, tenant and job id
into an opaque base64 string, so `get_job` needs no session, no cache and no memory of
the search that produced it.

## The law

`lawkeeper` judges every edit under `src/` through a `PostToolUse` hook and blocks on
violation. `law.toml` has `disabled = []` and an empty `[exempt]` — no rule is off, no
file is pardoned. Run `lawkeeper` for the project, `lawkeeper --rules` for every rule.

Known debt: the rule against free constants outside the sanctum landed after this code
was written, and the adapters and tenant tables still carry theirs.

The ones that shape this codebase: no comments anywhere; no `unwrap`/`unwrap_or`/`ok()`/
`is_some()`/`if let Some`; no `_` arms on an Option or Result; no `.filter_map()`/
`.flat_map()`/`.flatten()`; no `as` casts; no `Option` in a `pub fn` signature; no
default resolved outside `config.rs`. The legal shape for an absence is
`let Some(x) = expr else { return ... };`.

## What each platform actually gives you

| platform | body in listing | date in listing | server-side search | detail endpoint |
|---|---|---|---|---|
| greenhouse | yes (`content=true`) | yes | no | yes |
| lever | yes | yes (epoch ms) | no | yes |
| ashby | yes (`descriptionPlain`) | yes | no | **no** — board is refetched and scanned |
| workable | yes (widget) | yes | no | yes |
| recruitee | yes | yes | no | yes |
| smartrecruiters | **no** | yes | **yes** (`q=`, `country=` lowercase) | yes |
| workday | **no** | prose, parsed | **yes** (`searchText`) | yes |
| bamboohr | **no** | **no** | no | yes |
| teamtailor | yes | yes | lossy, unused | **no** — board is refetched and scanned |
| jobvite | **no** | **no** | yes | yes (schema.org block in the page) |

Two consequences the tool description states out loud: `term` falls back to title-only on
the four boards with no body in the listing, and `posted_within_days` drops every hit
from the two boards with no date. Undated hits also sort last, so a broad search cuts
them first — `search_jobs` reports `found` alongside `returned` so the caller can see it.

Jobvite is the one platform with no JSON surface at all: its listing is parsed out of the
career page markup and its detail out of the page's own schema.org `JobPosting` block.

## Adding an ATS platform

1. A variant on `Ats` in `model.rs`, plus its `label()` arm and a slot in `PLATFORMS`.
2. A module under `src/tenants/` and its slice in `tenants/table.rs`. Every row must have
   been probed live and answered with a posting — an unverified row burns a request slot
   on every search forever.
3. `src/ats/<platform>.rs` with `search` and `detail` — see `doc/ADAPTER_CONTRACT.md`,
   and copy the shape of `src/ats/greenhouse.rs`.
4. `mod` it in `ats/mod.rs` and add its two arms in `registry.rs`.

## Prior art in the neighbourhood

[getlyricsmcp](https://github.com/ZmoleCristian/getlyricsmcp) is the same two-step
search/fetch shape at a smaller scale, and this server borrows its structure. The
adapters were ported from `cautjob`, a Postgres-backed webapp, and stripped of Diesel.
