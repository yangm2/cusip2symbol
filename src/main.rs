mod cache;
mod display;
mod edgar;
mod figi;
mod portfolio;
pub mod sec_client;

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::BTreeMap;

#[derive(Parser)]
#[command(
    name = "sectool",
    about = "Security lookup, tax breakdown, and portfolio exposure tools"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Look up security information by CUSIP or ticker
    Lookup {
        /// CUSIP or ticker symbol (e.g. 037833100 or AAPL)
        identifier: String,

        /// Show all exchange listings instead of a summary
        #[arg(short, long)]
        all: bool,
    },

    /// Show tax-relevant holdings breakdown from SEC N-PORT filings
    Tax {
        /// Fund ticker (e.g. AGG, MUB, IEF)
        ticker: String,

        /// Show municipal breakdown for a specific state (e.g. OR, CA, NY)
        #[arg(short, long)]
        state: Option<String>,

        /// List municipal holdings with unknown state identification
        #[arg(short, long)]
        debug: bool,
    },

    /// Compute portfolio exposure to a target security
    Exposure {
        /// Target security ticker or CUSIP (e.g. NVDA or 67066G104)
        target: String,

        /// Portfolio TOML file
        portfolio: String,
    },

    /// Manage N-PORT filing cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Subcommand)]
enum CacheAction {
    /// Show cached N-PORT filings
    Status,
    /// Clear all cached filings
    Clear,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Lookup { identifier, all } => cmd_lookup(&identifier, all).await,
        Command::Tax {
            ticker,
            state,
            debug,
        } => cmd_tax(&ticker, state.as_deref(), debug).await,
        Command::Exposure { target, portfolio } => cmd_exposure(&target, &portfolio).await,
        Command::Cache { action } => cmd_cache(action),
    }
}

async fn cmd_lookup(identifier: &str, all: bool) -> Result<(), Box<dyn std::error::Error>> {
    let http = build_client();

    let is_cusip = looks_like_cusip(identifier);

    let data = if is_cusip {
        figi::lookup_cusip(&http, identifier).await?
    } else {
        figi::lookup_ticker(&http, identifier).await?
    };

    match data {
        Some(data) if !data.is_empty() => {
            if all {
                for item in &data {
                    display::print_item(item);
                    println!("---");
                }
            } else {
                let cusip = if is_cusip { Some(identifier) } else { None };
                display::print_summary(cusip, &data);
            }
        }
        _ => {
            if is_cusip {
                eprintln!("No results found for CUSIP: {identifier}");
            } else {
                eprintln!("No results found for ticker: {}", identifier.to_uppercase());
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn cmd_tax(
    ticker: &str,
    state: Option<&str>,
    debug: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let sec = sec_client::SecClient::new()?;

    let sp = ProgressBar::new_spinner().with_message(format!(
        "Looking up N-PORT for {}...",
        ticker.to_uppercase()
    ));
    sp.enable_steady_tick(std::time::Duration::from_millis(100));

    match edgar::lookup_tax_breakdown(&sec, ticker).await {
        Ok(breakdown) => {
            sp.finish_and_clear();
            let state_upper = state.map(|s| s.to_uppercase());
            display::print_tax_breakdown(&breakdown, state_upper.as_deref());
            if debug {
                display::print_unknown_munis(&breakdown);
            }
        }
        Err(e) => {
            sp.finish_and_clear();
            eprintln!("Tax breakdown unavailable: {e}");
            eprintln!("(N-PORT data is only available for mutual funds and ETFs)");
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn cmd_exposure(
    target: &str,
    portfolio_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let sec = sec_client::SecClient::new()?;

    let is_cusip = looks_like_cusip(target);
    let target_display = target.to_uppercase();

    let toml_str = std::fs::read_to_string(portfolio_path)
        .map_err(|e| format!("Failed to read portfolio file '{portfolio_path}': {e}"))?;
    let port = portfolio::parse_portfolio(&toml_str)?;
    let leaves = port.leaf_tickers();

    let pb = ProgressBar::new(leaves.len() as u64);
    pb.set_style(
        // Compile-time constant template; invalid template is a programmer error
        ProgressStyle::with_template("{bar:30.cyan/dim} {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("##-"),
    );

    let mut holding_pcts: BTreeMap<String, (String, f64)> = BTreeMap::new();

    for (fund_ticker, _weight) in &leaves {
        pb.set_message(fund_ticker.clone());
        if let Ok(xml) = edgar::download_nport_xml(&sec, fund_ticker).await {
            let field = if is_cusip {
                edgar::HoldingField::Cusip
            } else {
                edgar::HoldingField::Ticker
            };
            if let Ok(Some((name, pct))) = edgar::find_holding(&xml, field, target) {
                holding_pcts.insert(fund_ticker.clone(), (name, pct));
            }
        }
        pb.inc(1);
    }

    pb.finish_and_clear();
    let tree = portfolio::build_exposure_tree(&port.root, &port, &holding_pcts);
    display::print_exposure(&target_display, &tree);

    Ok(())
}

fn cmd_cache(action: CacheAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        CacheAction::Status => {
            let entries = cache::list_entries();
            if entries.is_empty() {
                println!("No cached N-PORT filings.");
            } else {
                println!("{:<8} {:<24} REPORT DATE", "TICKER", "ACCESSION");
                for (ticker, accession, date) in &entries {
                    println!("{:<8} {:<24} {}", ticker, accession, date);
                }
                println!("\n{} cached filing(s)", entries.len());
            }
        }
        CacheAction::Clear => {
            cache::clear()?;
            println!("Cache cleared.");
        }
    }
    Ok(())
}

fn looks_like_cusip(id: &str) -> bool {
    (id.len() == 8 || id.len() == 9)
        && id.chars().all(|c| c.is_ascii_alphanumeric())
        && id.chars().any(|c| c.is_ascii_digit())
}

pub fn build_client() -> reqwest::Client {
    reqwest::Client::new()
}
