# Development

## Project Structure

```
src/
  main.rs          CLI argument parsing, orchestration, HTTP client setup
  figi.rs          OpenFIGI API client
  display.rs       Terminal output formatting
  edgar/
    mod.rs         SEC EDGAR fund lookup and filing discovery
    nport.rs       N-PORT XML parser and TaxBreakdown computation
    states.rs      Heuristic state identification from municipal bond issuer names
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
| `<issuerCat>` | Issuer category: `UST` (Treasury), `USGA` (Gov Agency), `USGSE` (Gov Sponsored Enterprise), `MUN` (Municipal), `CORP` (Corporate), `NUSS` (Non-US Sovereign), `RF` (Registered Fund) |
| `<issuerConditional>` | Used when issuerCat is `OTHER`; the `desc` attribute contains the sub-type (e.g., "REIT") |
| `<pctVal>` | Percentage of net asset value (already in percent, e.g., 0.69 means 0.69%) |

**How we compute the tax breakdown:**
- Sum `pctVal` by `issuerCat` to get US Government (UST + USGA + USGSE), Municipal, Corporate, and Other percentages
- For municipal holdings, infer the state from the `<name>` field using heuristics (see below)

**Limitations:**
- `pctVal` is the fund's own calculation; rounding across thousands of holdings means totals may not sum to exactly 100%
- No state-of-issuer field for municipal bonds — we infer state from the issuer name
- Filed monthly but with a ~60 day delay, so data may be 1-3 months old

## State Identification for Municipal Bonds

N-PORT filings do not include a state field for municipal bond holdings. The CUSIP numbering system encodes state in the issuer prefix ranges, but these ranges are proprietary (owned by CUSIP Global Services / S&P Global) and not publicly available.

Instead, we use heuristic name matching on the `<name>` field, checked in priority order:

1. **"State of X" prefix** — e.g., "State of California" → CA
2. **State abbreviation as last word** — e.g., "Harris County TX" → TX
3. **State name anywhere in text** — e.g., "Oregon Health Authority" → OR
4. **Well-known city names** — e.g., "Chicago Board of Education" → IL

Holdings that don't match any pattern are categorized as "Unknown". For the iShares National Muni Bond ETF (MUB), roughly 26% of holdings fall into this category. These are typically county-level or special-purpose issuers whose names don't contain state identifiers (e.g., "Acalanes Union High School District").

## Testing

```sh
cargo test
```

Tests are co-located with their modules. Unit tests (N-PORT XML parsing, state guessing, Atom parsing) run without network access. Integration tests hit the live OpenFIGI and SEC EDGAR APIs.

## SEC Rate Limiting

The SEC enforces a rate limit of 10 requests per second and requires a `User-Agent` header identifying the caller with a contact email. Repeated violations result in temporary IP blocks. The tool makes at most 3 SEC requests per invocation (tickers JSON, Atom feed, N-PORT XML).
