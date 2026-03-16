# Development

## Project Structure

```
src/
  main.rs          CLI subcommands (lookup, tax, exposure, cache)
  sec_client.rs    Rate-limited SEC HTTP client with ticker cache
  cache.rs         Disk cache for N-PORT filings (~/.cache/sectool/)
  figi.rs          OpenFIGI API client
  display.rs       Terminal output formatting (tax breakdown, exposure tree)
  portfolio.rs     Portfolio TOML parser and exposure calculation
  edgar/
    mod.rs         EDGAR filing discovery and N-PORT download orchestration
    nport.rs       N-PORT XML parser: TaxBreakdown, CUSIP/ticker search
    states.rs      Heuristic state identification from municipal bond issuer names
regions.toml       City/region to state mapping (compiled into binary)
build.rs           Tells cargo to rebuild when regions.toml changes
portfolio.example.toml  Example portfolio definition
```

## APIs Used

### OpenFIGI — Security Identification

**Endpoint:** `POST https://api.openfigi.com/v3/mapping`

**What it does:** Maps a CUSIP to security metadata (ticker, name, FIGI, exchange, security type).

**Why this API:** OpenFIGI is the only free, no-registration-required API for CUSIP-to-ticker resolution. Bloomberg operates it as a public utility. Alternatives like CUSIP Global Services or Refinitiv require paid subscriptions.

**Request format:**
```json
[{"idType": "ID_CUSIP", "idValue": "037833100"}]
```

**Key response fields used:** `ticker`, `name`, `figi`, `exchCode`, `marketSector`, `securityType`

**Limitations:**
- Returns multiple results per CUSIP (one per exchange listing), so we deduplicate in the summary view
- Does not return CUSIPs when queried by other identifiers (no reverse lookup)
- Rate limited to 25 requests/minute without an API key

---

### SEC EDGAR — Fund Ticker to CIK Mapping

**Endpoint:** `GET https://www.sec.gov/files/company_tickers_mf.json`

**What it does:** Returns a JSON file mapping every registered mutual fund and ETF share class to its SEC CIK number, series ID, and class ID.

**Why this API:** This is the authoritative source for mapping ticker symbols to SEC entity identifiers. The CIK and series ID are required to find the correct N-PORT filing, since many funds (e.g., all iShares ETFs) share a single CIK but have different series IDs.

**Response format:**
```json
{
  "fields": ["cik", "seriesId", "classId", "symbol"],
  "data": [
    [1100663, "S000004346", "C000012076", "IWF"],
    [1100663, "S000004362", "C000012092", "AGG"]
  ]
}
```

**Caching:** The tickers list (~4MB) is fetched once per process via `tokio::sync::OnceCell` in `SecClient` and reused across all fund lookups. This is important for `sectool exposure`, which may look up many funds in a single run.

**Limitations:**
- Only includes mutual funds and ETFs, not individual stocks
- Does not include CUSIPs (hence the need for OpenFIGI as a first step)
- SEC requires a `User-Agent` header with a contact email; requests without one get 403
- SEC requires `Accept-Encoding: gzip` (via the `reqwest` gzip feature); requests without it may get 403

---

### SEC EDGAR — Filing Discovery via Atom Feed

**Endpoint:** `GET https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={seriesId}&type=NPORT-P&count=5&output=atom`

**What it does:** Returns an Atom XML feed of recent filings for a specific fund series, filtered by filing type.

**Why this API:** The alternative is the submissions JSON endpoint (`data.sec.gov/submissions/CIK{cik}.json`), but that returns filings for ALL series under a CIK with no series filter. For iShares Trust (CIK 1100663), that's hundreds of funds mixed together. The Atom feed accepts a series ID directly, returning only filings for the specific fund we need.

**Key data extracted:** The `<accession-number>` from the first `<entry>` element, which identifies the most recent N-PORT filing.

**URL construction for the actual filing:**
```
https://www.sec.gov/Archives/edgar/data/{CIK}/{accession_no_dashes}/primary_doc.xml
```

---

### SEC EDGAR — N-PORT Filing (XML)

**Endpoint:** `GET https://www.sec.gov/Archives/edgar/data/{CIK}/{accession}/primary_doc.xml`

**What it does:** Returns the full N-PORT XML filing containing every holding in the fund's portfolio.

**Why this filing:** N-PORT is the only structured, machine-readable SEC filing that provides per-holding data with issuer categorization. The alternative (N-CSR shareholder reports) contains the official tax percentages but as unstructured HTML that varies by fund family and would require complex text extraction.

**Key XML elements used:**

| Element | Purpose |
|---------|---------|
| `<seriesName>` | Fund name |
| `<repPdDate>` | Report period end date |
| `<invstOrSec>` | Individual holding container |
| `<name>` | Issuer name (used for state identification of municipal bonds) |
| `<cusip>` | CUSIP identifier (used for `sectool exposure` CUSIP-based search) |
| `<ticker>` | Ticker symbol (used for `sectool exposure` ticker-based search) |
| `<issuerCat>` | Issuer category: `UST` (Treasury), `USGA` (Gov Agency), `USGSE` (Gov Sponsored Enterprise), `MUN` (Municipal), `CORP` (Corporate), `NUSS` (Non-US Sovereign), `RF` (Registered Fund) |
| `<issuerConditional>` | Used when issuerCat is `OTHER`; the `desc` attribute contains the sub-type (e.g., "REIT") |
| `<pctVal>` | Percentage of net asset value (already in percent, e.g., 0.69 means 0.69%) |

**How we compute the tax breakdown:**
- Sum `pctVal` by `issuerCat` to get US Government (UST + USGA + USGSE), Municipal, Corporate, and Other percentages
- For municipal holdings, infer the state from the `<name>` field using heuristics (see below)

**How we compute exposure:**
- For each fund in the portfolio, search its N-PORT for the target security by CUSIP or ticker
- Sum `pctVal` across all matching holdings within each fund
- Multiply by the fund's portfolio weight to get effective exposure

**Filing frequency:**
- N-PORTs are filed monthly but only quarterly filings (March, June, September, December period ends) are made public
- There is a ~60 day delay between the report period end and public availability
- Data may be 1-3 months old at any given time

**Limitations:**
- `pctVal` is the fund's own calculation; rounding across thousands of holdings means totals may not sum to exactly 100%
- No state-of-issuer field for municipal bonds — we infer state from the issuer name
- Not all holdings have `<ticker>` fields; CUSIP-based search is more reliable for `sectool exposure`

## Caching

N-PORT XML files are cached on disk at `~/.cache/sectool/{TICKER}.json`. Each cache entry stores:

- **accession** — the SEC filing accession number, used as the cache key
- **report_date** — the report period end date, for display in `sectool cache status`
- **xml** — the full N-PORT XML

**Cache invalidation:** On each lookup, the Atom feed is checked for the latest accession number. If it matches the cached accession, the cached XML is returned without downloading. If a newer filing exists, it replaces the cached entry automatically. This means the Atom feed is always hit (1 SEC request), but the much larger XML download is skipped when the filing hasn't changed.

**Cache impact on SEC requests per operation:**
| Operation | Cache miss | Cache hit |
|-----------|-----------|-----------|
| `sectool tax` | 3 requests (tickers, Atom, XML) | 2 requests (tickers, Atom) |
| `sectool exposure` (N funds) | 1 + 2N requests (tickers once, then Atom + XML per fund) | 1 + N requests (tickers once, Atom per fund) |

## Rate Limiting

The SEC enforces a limit of 10 requests per second and requires a `User-Agent` header with a contact email. Repeated violations result in temporary IP blocks.

`SecClient` implements a sliding-window rate limiter that caps SEC requests to 8/sec (staying safely under the 10/sec limit). All SEC HTTP requests go through `SecClient::get()`, which tracks timestamps and sleeps when the window is full. Non-SEC requests (e.g., OpenFIGI) bypass the rate limiter.

## State Identification for Municipal Bonds

N-PORT filings do not include a state field for municipal bond holdings. The CUSIP numbering system encodes state in the issuer prefix ranges, but these ranges are proprietary (owned by CUSIP Global Services / S&P Global) and not publicly available.

Instead, we use heuristic name matching on the `<name>` field, checked in priority order:

1. **"State of X" prefix** — e.g., "State of California" -> CA
2. **State abbreviation as last word** — e.g., "Harris County TX" -> TX
3. **State name anywhere in text** — e.g., "Oregon Health Authority" -> OR
4. **Well-known region names** from `regions.toml` — e.g., "Chicago Board of Education" -> IL

Holdings that don't match any pattern are categorized as "Unknown". Use `sectool tax --debug` to list these holdings (sorted by count) and identify candidates for adding to `regions.toml`.

### Adding region mappings

Edit `regions.toml` to add city, county, or other regional identifiers:

```toml
[regions]
"SEATTLE" = "WA"
"COUNTY OF MULTNOMAH" = "OR"
```

Keys are matched case-insensitively against the full issuer name. Longer keys are matched first to avoid false positives. The file is compiled into the binary via `include_str!`, with `build.rs` ensuring recompilation when the file changes.

## Portfolio Exposure Analysis

The `sectool exposure` command computes how much of a user's portfolio is exposed to a specific security through their fund holdings.

### Portfolio TOML format

The TOML file defines a tree hierarchy:
- **Interior nodes** (TOML tables) define groups for intermediate rollups
- **Leaf values** (strings) are fund tickers with allocations: `"$N"` for USD or `"N%"` for percentage

USD allocations are normalized to portfolio weights at runtime. The tree structure is preserved in the output so users can see exposure broken down by account type, asset class, or any custom grouping.

### Calculation

For each leaf fund:
1. Download its latest N-PORT filing (via cache when possible)
2. Search all holdings for the target security by CUSIP or ticker
3. Sum `pctVal` across matching holdings to get the fund's allocation percentage
4. Multiply by the fund's portfolio weight to get effective exposure
5. Roll up exposure through the tree hierarchy for group subtotals

## Testing

```sh
cargo test
```

Tests are co-located with their modules. Unit tests (N-PORT XML parsing, state guessing, Atom parsing, portfolio parsing, exposure calculation) run without network access. Integration tests hit the live OpenFIGI and SEC EDGAR APIs.

## AI Development Context

Notes for future AI-assisted coding sessions. This section captures non-obvious decisions and pitfalls that aren't derivable from the code alone.

### History

This project was originally called `cusip2symbol` — a single-purpose CUSIP-to-ticker lookup tool. It evolved into `sectool` with four subcommands (`lookup`, `tax`, `exposure`, `cache`). The git repo directory is still named `cusip2symbol` but the Cargo package is `sectool`.

### Architectural Decisions and Rationale

- **`SecClient::get()` returns `reqwest::Result<Response>`, not `Box<dyn Error>`**. Using `Box<dyn Error>` caused cascading type inference failures when chaining `.text().await?` on the response. The concrete error type keeps the async chain ergonomic.

- **`SecClient` was extracted from `edgar/mod.rs`** to break a circular dependency: both `edgar` (for filing downloads) and `main` (for tickers lookup) needed rate-limited SEC access. It lives at the crate root (`sec_client.rs`) rather than inside `edgar/` so that `main.rs` can use it without reaching into the edgar module.

- **Atom feed over submissions JSON for filing discovery**. The submissions endpoint (`data.sec.gov/submissions/CIK{cik}.json`) returns filings for ALL series under a CIK. For large trusts like iShares (CIK 1100663), that's hundreds of funds. The Atom feed accepts a series ID directly, returning only the target fund's filings.

- **`extract_report_date()` uses string search, not XML parsing**. The full XML parse via `parse_nport_xml()` is expensive for large filings. Since we only need the report date for cache metadata, a simple `xml.find("repPdDate>")` is sufficient and avoids parsing twice (once for cache, once for the actual data).

- **OpenFIGI does not support reverse CUSIP lookup**. You can look up a CUSIP to get a ticker, but not a ticker to get a CUSIP. This is why `sectool exposure` has two separate N-PORT search paths: `find_holding_pct()` (by CUSIP) and `find_holding_by_ticker()` (by ticker field). The ticker path is less reliable since not all N-PORT holdings include `<ticker>`.

- **Heuristic state identification instead of CUSIP prefix ranges**. The CUSIP numbering system encodes state in the issuer prefix, but those ranges are proprietary (CUSIP Global Services / S&P Global). We use name-matching heuristics instead, with `regions.toml` as an extensible fallback.

### Known Pitfalls

- **SEC 403 errors**: SEC requires both a `User-Agent` header with a contact email AND `Accept-Encoding: gzip`. Missing either causes 403. The `reqwest` `gzip` feature handles the encoding; `SEC_USER_AGENT` in `main.rs` handles the user agent.

- **N-PORT XML entity references**: The `quick-xml` parser emits `GeneralRef` events for XML entity references (e.g., `&amp;`) inside text fields. The XML parsing loops in `nport.rs` must handle these events alongside regular `Text` events, or issuer names containing `&` will be silently truncated.

- **`pctVal` is already in percent**: A value of `0.69` means 0.69%, not 69%. No multiplication by 100 needed.

- **N-PORT filing lag**: Public N-PORT filings are quarterly only (March/June/September/December period ends) with ~60 day delay. Data can be 1-3 months stale.

### Known Technical Debt

- **nport.rs parses all fields on every pass**: The unified `parse_nport()` collects all holding fields (name, cusip, ticker, issuerCat, pctVal) even when a caller only needs a subset. This is negligible overhead (extra string copies, not extra I/O) but could be optimized with a visitor pattern if profiling shows it matters.

- **`SecClient::new()` reads `SEC_CONTACT_EMAIL` at construction time**, so commands that don't hit SEC (`lookup`, `cache`) work without it. Tests set a placeholder via `unsafe { set_var(...) }` — acceptable since integration tests hit live APIs sequentially anyway.

### CI / Release Actions

GitHub Actions workflows live in `.github/workflows/`. The actions used (e.g., `actions/checkout`, `actions/upload-artifact`) must be kept on Node.js 24+ versions — GitHub deprecates older Node.js runtimes on a ~yearly cycle. When you see deprecation warnings in CI logs, bump the action major versions in both `ci.yml` and `release.yml`.

### Build and Run

```sh
cargo build                         # debug build
cargo install --path .              # install to ~/.cargo/bin/
sectool lookup AAPL                 # smoke test
sectool tax AGG                     # requires network (SEC EDGAR)
cargo test                          # unit tests (offline) + integration tests (online)
```
