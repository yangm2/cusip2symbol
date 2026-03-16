mod cache;
mod display;
mod edgar;
mod figi;
mod portfolio;
pub mod sec_client;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::BTreeMap;

const SEC_USER_AGENT: &str = "cusip2symbol support@example.com";

#[derive(Parser)]
#[command(name = "cusip2symbol", about = "Look up security information by CUSIP or ticker")]
struct Cli {
    /// CUSIP identifier or ticker symbol (e.g. 037833100 or AAPL)
    identifier: Option<String>,

    /// Show all exchange listings instead of a summary
    #[arg(short, long)]
    all: bool,

    /// Show tax-relevant holdings breakdown from SEC N-PORT filings
    #[arg(short, long)]
    tax: bool,

    /// State abbreviation for municipal tax summary (e.g. OR, CA, NY)
    #[arg(short, long)]
    state: Option<String>,

    /// List municipal holdings with unknown state identification
    #[arg(short, long)]
    debug: bool,

    /// Compute portfolio exposure to a target security (provide target ticker/CUSIP and portfolio TOML file)
    #[arg(short, long, value_names = ["TOML_FILE"])]
    exposure: Option<String>,

    /// Show cached N-PORT filings
    #[arg(long)]
    cache_status: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    if cli.cache_status {
        let entries = cache::list_entries();
        if entries.is_empty() {
            println!("No cached N-PORT filings.");
        } else {
            println!("{:<8} {:<24} {}", "TICKER", "ACCESSION", "REPORT DATE");
            for (ticker, accession, date) in &entries {
                println!("{:<8} {:<24} {}", ticker, accession, date);
            }
            println!("\n{} cached filing(s)", entries.len());
        }
        return Ok(());
    }

    let id = cli.identifier.as_deref().unwrap_or_else(|| {
        eprintln!("Error: an identifier (CUSIP or ticker) is required");
        std::process::exit(1);
    });
    let http = build_client();
    let sec = sec_client::SecClient::new(http.clone());

    // Detect if input looks like a CUSIP (8-9 chars, alphanumeric with digits) or a ticker
    let is_cusip = (id.len() == 8 || id.len() == 9)
        && id.chars().all(|c| c.is_ascii_alphanumeric())
        && id.chars().any(|c| c.is_ascii_digit());

    let ticker: String;

    if is_cusip {
        let data = figi::lookup_cusip(&http, id).await?;
        match data {
            Some(data) if !data.is_empty() => {
                if cli.all {
                    for item in &data {
                        display::print_item(item);
                        println!("---");
                    }
                } else {
                    display::print_summary(id, &data);
                }
                ticker = data[0].ticker.clone().unwrap_or_default();
            }
            _ => {
                eprintln!("No results found for CUSIP: {id}");
                std::process::exit(1);
            }
        }
    } else {
        ticker = id.to_uppercase();
        if cli.exposure.is_none() {
            println!("Ticker:        {ticker}");
        }
    }

    let state = cli.state.as_deref().map(|s| s.to_uppercase());
    let show_tax = cli.tax || state.is_some() || cli.debug;

    if show_tax {
        if ticker.is_empty() {
            eprintln!("Cannot look up tax info: no ticker found");
            std::process::exit(1);
        }
        if is_cusip {
            println!();
        }
        let sp = ProgressBar::new_spinner()
            .with_message(format!("Looking up N-PORT for {ticker}..."));
        sp.enable_steady_tick(std::time::Duration::from_millis(100));
        match edgar::lookup_tax_breakdown(&sec, &ticker).await {
            Ok(breakdown) => {
                sp.finish_and_clear();
                display::print_tax_breakdown(&breakdown, state.as_deref());
                if cli.debug {
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
    }

    if let Some(toml_path) = &cli.exposure {
        let target_display = id.to_uppercase();

        let toml_str = std::fs::read_to_string(toml_path)
            .map_err(|e| format!("Failed to read portfolio file '{toml_path}': {e}"))?;
        let port = portfolio::parse_portfolio(&toml_str)?;
        let leaves = port.leaf_tickers();

        let pb = ProgressBar::new(leaves.len() as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "{bar:30.cyan/dim} {pos}/{len} {msg}",
            )
            .unwrap()
            .progress_chars("##-"),
        );

        let mut holding_pcts: BTreeMap<String, (String, f64)> = BTreeMap::new();

        for (fund_ticker, _weight) in &leaves {
            pb.set_message(fund_ticker.clone());
            match edgar::download_nport_xml(&sec, fund_ticker).await {
                Ok(xml) => {
                    let result = if is_cusip {
                        edgar::find_holding_pct(&xml, id)
                    } else {
                        edgar::find_holding_by_ticker(&xml, id)
                    };
                    if let Ok(Some((name, pct))) = result {
                        holding_pcts.insert(fund_ticker.clone(), (name, pct));
                    }
                }
                Err(_) => {}
            }
            pb.inc(1);
        }

        pb.finish_and_clear();
        let tree = portfolio::build_exposure_tree(&port.root, &port, &holding_pcts);
        display::print_exposure(&target_display, &tree);
    }

    Ok(())
}

pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(SEC_USER_AGENT)
        .build()
        .expect("failed to build HTTP client")
}
