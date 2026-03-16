mod display;
mod edgar;
mod figi;

use clap::Parser;

const SEC_USER_AGENT: &str = "cusip2symbol support@example.com";

#[derive(Parser)]
#[command(name = "cusip2symbol", about = "Look up security information by CUSIP or ticker")]
struct Cli {
    /// CUSIP identifier or ticker symbol (e.g. 037833100 or AAPL)
    identifier: String,

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let id = &cli.identifier;
    let client = build_client();

    // Detect if input looks like a CUSIP (8-9 chars, alphanumeric with digits) or a ticker
    let is_cusip = (id.len() == 8 || id.len() == 9)
        && id.chars().all(|c| c.is_ascii_alphanumeric())
        && id.chars().any(|c| c.is_ascii_digit());

    let ticker: String;

    if is_cusip {
        let data = figi::lookup_cusip(&client, id).await?;
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
        println!("Ticker:        {ticker}");
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
        match edgar::lookup_tax_breakdown(&client, &ticker).await {
            Ok(breakdown) => {
                display::print_tax_breakdown(&breakdown, state.as_deref());
                if cli.debug {
                    display::print_unknown_munis(&breakdown);
                }
            }
            Err(e) => {
                eprintln!("Tax breakdown unavailable: {e}");
                eprintln!("(N-PORT data is only available for mutual funds and ETFs)");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(SEC_USER_AGENT)
        .build()
        .expect("failed to build HTTP client")
}
