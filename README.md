# corpHuntingMCP

Reverse head-hunting. Instead of recruiters searching for you, an MCP server that
searches every ATS board at once for you.

One `search_jobs` call fans out over a table of some 20 000 live employer boards across
ten ATS platforms — Greenhouse, Lever, Ashby, Workable, Recruitee, SmartRecruiters,
Workday, BambooHR, Teamtailor and Jobvite — filters the answers, and hands back postings
with an opaque id. `get_job(id)` fetches the full posting fresh.

No API keys, no accounts, no database, no config file. Nine of the ten platforms publish
a public JSON board API and that is all this reads; Jobvite alone has no JSON surface, so
its listing is parsed out of the career page and its detail out of the page's own
schema.org block.

## Install

Needs a stable Rust toolchain recent enough for edition 2024.

```sh
cargo install --git https://github.com/ZmoleCristian/corpHuntingMCP
claude mcp add corphuntingmcp -- corphuntingmcp
```

Any other MCP client: it is a stdio server, the command is `corphuntingmcp`, no arguments.

```json
{ "mcpServers": { "corphuntingmcp": { "command": "corphuntingmcp" } } }
```

Then ask for what you want: *"remote Rust jobs I could take from Romania, posted this
month"*, *"what does 1Password have open"*.

Logging goes to stderr: `RUST_LOG=debug corphuntingmcp`.

## Tools

| tool | what it does |
|---|---|
| `search_jobs` | sweep the boards, return matching postings, each with an id |
| `search_jobs_async` | the same sweep in the background, full result written to a JSON file |
| `get_job` | the full posting for an id: description, department, employment type, compensation |
| `list_companies` | which employers are covered, optionally narrowed by a needle |

`search_jobs` and `search_jobs_async` take the same arguments:

| argument | meaning |
|---|---|
| `title` | matched against the job title |
| `term` | list of words or phrases, all of which must appear in the title or posting body |
| `company` | matched against the employer name; enough on its own |
| `exclude` | list; a hit naming any entry in its title, body or location is dropped |
| `country` | ISO-2 code: `RO`, `DE`, `US` |
| `remote` | `true` keeps remote postings only; beside `country` it means *in that country or remote anywhere* |
| `ats` | list of platforms to restrict the sweep to |
| `posted_within_days` | freshness cutoff |
| `limit` | how many hits to return |
| `reach` | how many employers per platform to ask; `0` for all of them |

## The two-step

```
search_jobs(term: ["rust"], country: "RO", posted_within_days: 30)
  -> 14 hits, each with an id

get_job(id: "Z3JlZW5ob3VzZQlzdXBlcgk0OTE4ODIwMTAx")
  -> title, location, department, employment type, compensation, full description
```

`title` matches the job title. `term` is a list, each entry matched against the title
*plus* the posting body, and every entry must be present — `term: ["rust", "benchmarking"]`
returns only postings that mention both. A one-word entry matches whole words, so `rust`
never matches `trust`; an entry with spaces matches as a phrase. `exclude` is its mirror:
a list whose entries, found in the title, body *or* location, drop the hit —
`exclude: ["United States"]` clears the American postings out of a remote search.

Give at least one of `title`, `term` or `company`. `company` alone is a legitimate
search and the most common one: it lists everything that employer has open, and it also
lifts the reach cap, so naming an employer always reaches them however deep they sit.

Every hit carries `remote` as its own field, because location strings conflate two facts:
`"Remote; Argentina"` is both remote and Argentinian. `country` on its own is strict, so
that posting fails `country: "RO"` — but `remote: true` beside a country widens the test
to *in that country or remote anywhere*, which is what someone hunting from Romania
actually means.

## How wide it reaches, and why it is capped

There is no central index of job boards. A search without a company has to ask every
employer's board in turn, and the table is large, so `reach` caps how many employers per
platform get asked — 50 by default, which is about a minute on a cold cache. Most MCP
clients abandon a call at two minutes and roughly 100 per platform is where a cold sweep
crosses that line, so raise `reach` together with `ats`, not on its own. Boards are
cached for ten minutes, so the first search of a session costs about a minute and every
search after it is instant.

**A default search is a sample, not an answer.** It asks 500 of some 20 000 employers,
and which 500 is discovery order, not a ranking — only the Recruitee and Teamtailor
tables are sorted by how much each board is hiring. So a blind sweep tells you what is
out there, not everything that is out there.

To get an answer you can trust: pass `company` to go straight to one employer however
deep it sits, narrow with `ats` and raise `reach` so the whole budget lands on the
platform you care about, or set `reach: 0` to reach every employer covered and wait for
it. `search_jobs` always reports how many employers it actually asked, so a sampled
sweep never reads as a complete one.

A sweep that wide will outlive the client's two-minute ceiling, which is what
`search_jobs_async` is for: same arguments, returns a path immediately, and the full
result appears there as JSON when the sweep finishes. The file lands in `$CORPHUNT_OUT`,
defaulting to a `corphuntingmcp/` folder inside the platform temp directory — nothing to
configure on any OS, and nothing left behind that matters. The background sweep lives in
the server process, so it dies if the client shuts the server down.

This is the honest shape of the problem: there is no index, 20 000 boards cannot be
swept per query at a rate the platforms tolerate, and pretending otherwise would just
mean lying about coverage.

The pacing — four boards at a time per platform, a second between batches — is not
politeness, it is the operating limit. Harvesting this table earned a 24-hour IP block
from Workable at roughly 850 requests and a 403 wall from Teamtailor at 24-way
concurrency. A job hunter locked out of an ATS cannot apply, so the tool stays under the
rate that was measured safe. Boards that fail are cached as failing and not retried for
the rest of the window, which keeps a struggling host from being hammered. Tenants that
fail are logged to stderr and skipped; a dead board never kills a search.

Two honest limits. `term` only reaches the posting body on boards that publish it in
their listing — Greenhouse, Lever, Ashby, Recruitee, Teamtailor, Workable — and falls
back to the title alone on SmartRecruiters, Workday, BambooHR and Jobvite, because
reaching the body there would cost one extra request per posting. And
`posted_within_days` drops any hit whose board publishes no date in its listing, which
today means all of BambooHR and Jobvite.

## Coverage

Coverage is a fixed table compiled into the binary, under `src/tenants/`, one module per
platform. Every row in it was probed live during the harvest and answered with at least
one open posting. Boards die and new ones appear, so the table drifts; a dead row costs
one failed request per window and nothing else.

Adding an employer is a line of data. Adding a *platform* is one file under `src/ats/`
implementing two functions; `doc/ADAPTER_CONTRACT.md` is the whole contract, and
`CLAUDE.md` walks the architecture.

The Recruitee and Teamtailor tables are ordered by how much each board is hiring; the
rest are in discovery order, which is why the default `reach` is a sample rather than a
shortlist. Sorting the remaining eight by live job count is the obvious next improvement.

## The law

The code under `src/` is written to [lawkeeper](https://github.com/ZmoleCristian/lawkeeper)'s
syntax-only rules, with nothing disabled and an empty `[exempt]`: no `unwrap`, no `as`,
no erased error types, no wildcard match arms, no comments. `law.toml` holds the layer
map; an import that points up the graph fails the judge. `.claude/` wires the judge into
Claude Code as a hook on every edit.

Known debt: lawkeeper has since grown a rule against free constants outside `config.rs`,
and the adapters and tenant tables still carry theirs.

## License

0BSD. Not affiliated with any of the platforms it reads.
