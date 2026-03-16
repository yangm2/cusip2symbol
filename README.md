# cusip2symbol

A command-line tool to look up security information by CUSIP or ticker symbol, with tax-relevant holdings breakdowns for mutual funds and ETFs.

## Installation

```sh
cargo install --path .
```

## Usage

### Basic lookup by CUSIP

```sh
cusip2symbol 037833100
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

### Lookup by ticker

```sh
cusip2symbol AAPL
```

### Tax-relevant holdings breakdown

Use `--tax` to fetch the fund's latest SEC N-PORT filing and compute a breakdown by issuer category:

```sh
cusip2symbol --tax AGG
```

```
Ticker:        AGG
Looking up fund info for AGG...
Found CIK 1100663 series S000004362 — finding latest N-PORT...
Downloading and parsing N-PORT...
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

### State-specific municipal summary

Use `--state` to summarize municipal holdings for a specific state (implies `--tax`):

```sh
cusip2symbol --state OR MUB
```

```
--- Municipal Holdings by State ---
  OR                     0.38%  (  0.4% of muni)
  Other states          72.85%  ( 73.1% of muni)
  Unknown               26.48%  ( 26.6% of muni)
```

### All exchange listings

```sh
cusip2symbol --all 037833100
```

## Options

| Flag | Description |
|------|-------------|
| `-a`, `--all` | Show all exchange listings instead of a summary |
| `-t`, `--tax` | Show tax-relevant holdings breakdown from SEC N-PORT filings |
| `-s`, `--state <ST>` | State abbreviation for municipal tax summary (e.g. OR, CA, NY); implies `--tax` |

## Data Sources

- **[OpenFIGI](https://www.openfigi.com/)** — CUSIP-to-security mapping (free, no API key required)
- **[SEC EDGAR](https://www.sec.gov/edgar)** — Fund metadata and N-PORT portfolio holdings filings
