# sectool

[![CI](https://github.com/yangm2/cusip2symbol/actions/workflows/ci.yml/badge.svg)](https://github.com/yangm2/cusip2symbol/actions/workflows/ci.yml)

Security lookup, tax breakdown, and portfolio exposure tools for the command line.

## Installation

```sh
cargo install --path .
```

## Setup

Commands that access SEC EDGAR (`tax`, `exposure`) require a contact email per SEC's [fair access policy](https://www.sec.gov/os/webmaster-faq#developers):

```sh
export SEC_CONTACT_EMAIL=you@example.com
```

The `lookup` and `cache` commands do not require this.

## Commands

### `sectool lookup` — Security information

Look up a security by CUSIP or ticker symbol via OpenFIGI.

```sh
sectool lookup 037833100
```

```
CUSIP:         037833100
Ticker:        AAPL
Name:          APPLE INC
FIGI:          BBG000B9XRY4
Market Sector: Equity
Security Type: Common Stock
Exchanges:     UA, UB, UC, UD, ...
```

```sh
sectool lookup AAPL
```

```
Ticker:        AAPL
Name:          APPLE INC
FIGI:          BBG000B9XRY4
...
```

Use `--all` to show every exchange listing:

```sh
sectool lookup --all 037833100
```

### `sectool tax` — Holdings breakdown

Fetch a fund's latest SEC N-PORT filing and compute a tax-relevant breakdown by issuer category.

```sh
sectool tax AGG
```

```
=== Tax-Relevant Holdings Breakdown ===
Fund:            iShares Core U.S. Aggregate Bond ETF
Report Date:     2025-11-30
Total Holdings:  12954

US Government:    69.90%  (Treasury 45.15%, Agency  5.76%, GSE 18.98%)
Municipal:         0.43%
Corporate:        24.71%
Other:             6.56%

--- Municipal Holdings by State ---
  CA                     0.10%  ( 24.2% of muni)
  TX                     0.05%  ( 12.3% of muni)
  ...
```

Filter municipal holdings for a specific state:

```sh
sectool tax MUB --state OR
```

```
--- Municipal Holdings by State ---
  OR                     0.38%  (  0.4% of muni)
  Other states          72.85%  ( 73.1% of muni)
  Unknown               26.48%  ( 26.6% of muni)
```

List municipal holdings that couldn't be mapped to a state:

```sh
sectool tax MUB --debug
```

### `sectool exposure` — Portfolio exposure

Compute how much of your portfolio is exposed to a specific security across all your funds.

```sh
sectool exposure NVDA portfolio.toml
```

```
=== Portfolio Exposure to NVDA ===

Tax-Deferred (401k)                  2.840%
  US Equity                          2.840%
    VTI                              1.200%  (wt  24.0% x hold  5.00%)
    QQQ                              1.640%  (wt   8.0% x hold 20.50%)
  International                      0.000%  (wt   6.0%, no N-PORT)
  Fixed Income                       0.000%
    AGG                              0.000%  (wt  12.0% x hold  0.00%)
Taxable Brokerage                    1.520%
  Core                               1.520%
    IVV                              1.520%  (wt  16.0% x hold  9.50%)
    VXUS                             0.000%  (wt   4.0%, no N-PORT)
  ...

Total Exposure:  4.360%
```

The target can be a ticker (searches N-PORT `<ticker>` fields) or a CUSIP (searches `<cusip>` fields).

#### Portfolio TOML format

The portfolio file uses a tree hierarchy. Top-level groups represent account types with different tax treatments; subtrees provide intermediate exposure rollups. Leaf values are `"$N"` for USD amounts or `"N%"` for percentages:

```toml
["Tax-Deferred (401k)"."US Equity"]
VTI  = "$120000"
QQQ  = "$40000"

["Tax-Deferred (401k)"."Fixed Income"]
AGG  = "$60000"

["Tax-Exempt (Roth IRA)"."Growth"]
VUG  = "$35000"

["Taxable Brokerage"."Core"]
IVV  = "$80000"

["Taxable Brokerage"."Municipal Bonds"]
MUB  = "$50000"
```

See `portfolio.example.toml` for a complete example.

### `sectool cache` — Cache management

N-PORT filings are cached locally in `~/.cache/sectool/` to avoid redundant SEC downloads. The cache is keyed by filing accession number and automatically refreshed when a newer filing is available.

```sh
sectool cache status
```

```
TICKER   ACCESSION                REPORT DATE
AGG      0002071691-26-001295     2025-11-30
IEF      0002071691-26-001299     2025-11-30
MUB      0002071691-26-001301     2025-11-30

3 cached filing(s)
```

```sh
sectool cache clear
```

## Rate Limiting

SEC EDGAR enforces a rate limit of 10 requests/second. This tool throttles all SEC requests to 8/sec and caches N-PORT filings locally to minimize API calls. The mutual fund tickers list (`company_tickers_mf.json`) is fetched once per session and reused across lookups.

## Data Sources

- **[OpenFIGI](https://www.openfigi.com/)** -- CUSIP-to-security mapping (free, no API key required)
- **[SEC EDGAR](https://www.sec.gov/edgar)** -- Fund metadata and N-PORT portfolio holdings filings
