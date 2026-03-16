use crate::edgar::TaxBreakdown;
use crate::figi::FigiResult;
use crate::portfolio::{ExposureNode, ExposureResult};
use std::collections::BTreeSet;

pub fn print_summary(cusip: &str, data: &[FigiResult]) {
    let first = &data[0];

    let exchanges: BTreeSet<&str> = data.iter().filter_map(|r| r.exch_code.as_deref()).collect();

    println!("CUSIP:         {cusip}");
    println!(
        "Ticker:        {}",
        first.ticker.as_deref().unwrap_or("N/A")
    );
    println!("Name:          {}", first.name.as_deref().unwrap_or("N/A"));
    println!("FIGI:          {}", first.figi.as_deref().unwrap_or("N/A"));
    println!(
        "Market Sector: {}",
        first.market_sector.as_deref().unwrap_or("N/A")
    );
    println!(
        "Security Type: {}",
        first.security_type.as_deref().unwrap_or("N/A")
    );
    if !exchanges.is_empty() {
        let list: Vec<&str> = exchanges.into_iter().collect();
        println!("Exchanges:     {}", list.join(", "));
    }
}

pub fn print_item(item: &FigiResult) {
    println!("Ticker:        {}", item.ticker.as_deref().unwrap_or("N/A"));
    println!("Name:          {}", item.name.as_deref().unwrap_or("N/A"));
    println!("FIGI:          {}", item.figi.as_deref().unwrap_or("N/A"));
    println!(
        "Exchange:      {}",
        item.exch_code.as_deref().unwrap_or("N/A")
    );
    println!(
        "Market Sector: {}",
        item.market_sector.as_deref().unwrap_or("N/A")
    );
    println!(
        "Security Type: {}",
        item.security_type.as_deref().unwrap_or("N/A")
    );
    if let Some(st2) = &item.security_type2 {
        println!("Security Type2:{st2}");
    }
}

pub fn print_tax_breakdown(b: &TaxBreakdown, selected_state: Option<&str>) {
    println!("=== Tax-Relevant Holdings Breakdown ===");
    println!("Fund:            {}", b.fund_name);
    println!("Report Date:     {}", b.report_date);
    println!("Total Holdings:  {}", b.total_holdings);
    println!();
    println!(
        "US Government:   {:6.2}%  (Treasury {:5.2}%, Agency {:5.2}%, GSE {:5.2}%)",
        b.us_gov_total_pct,
        b.us_treasury_pct,
        b.us_gov_agency_pct,
        b.us_gov_gse_pct
    );
    println!("Municipal:       {:6.2}%", b.municipal_pct);
    println!("Corporate:       {:6.2}%", b.corporate_pct);
    println!("Other:           {:6.2}%", b.other_pct);

    if !b.municipal_by_state.is_empty() && b.municipal_pct > 0.0 {
        println!();
        if let Some(state) = selected_state {
            let state_pct = b.municipal_by_state.get(state).copied().unwrap_or(0.0);
            let unknown_pct = b.municipal_by_state.get("Unknown").copied().unwrap_or(0.0);
            let other_pct = b.municipal_pct - state_pct - unknown_pct;

            println!("--- Municipal Holdings by State ---");
            println!(
                "  {:<20} {:6.2}%  ({:5.1}% of muni)",
                state, state_pct, state_pct / b.municipal_pct * 100.0
            );
            println!(
                "  {:<20} {:6.2}%  ({:5.1}% of muni)",
                "Other states", other_pct, other_pct / b.municipal_pct * 100.0
            );
            println!(
                "  {:<20} {:6.2}%  ({:5.1}% of muni)",
                "Unknown", unknown_pct, unknown_pct / b.municipal_pct * 100.0
            );
        } else {
            println!("--- Municipal Holdings by State ---");
            let mut sorted: Vec<(&String, &f64)> = b.municipal_by_state.iter().collect();
            sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap());
            for (state, pct) in &sorted {
                if **pct >= 0.01 {
                    println!(
                        "  {:<20} {:6.2}%  ({:5.1}% of muni)",
                        state,
                        **pct,
                        **pct / b.municipal_pct * 100.0
                    );
                }
            }
        }
    }
}

pub fn print_exposure(target: &str, results: &[ExposureResult]) {
    let total: f64 = results
        .iter()
        .map(|r| match r {
            ExposureResult::Group(g) => g.exposure_pct,
            ExposureResult::Leaf(l) => l.exposure_pct,
        })
        .sum();

    println!("=== Portfolio Exposure to {target} ===");
    println!();
    for result in results {
        print_exposure_node(result, 0);
    }
    println!();
    println!("Total Exposure:  {total:6.3}%");
}

fn print_exposure_node(result: &ExposureResult, depth: usize) {
    let indent = "  ".repeat(depth);
    match result {
        ExposureResult::Group(ExposureNode {
            name,
            exposure_pct,
            children,
        }) => {
            println!("{indent}{name:<30} {:>8.3}%", exposure_pct);
            for child in children {
                print_exposure_node(child, depth + 1);
            }
        }
        ExposureResult::Leaf(h) => {
            let weight_pct = h.portfolio_weight * 100.0;
            match h.holding_pct {
                Some(pct) => {
                    println!(
                        "{indent}{:<30} {:>8.3}%  (wt {:5.1}% x hold {:5.2}%)",
                        h.ticker, h.exposure_pct, weight_pct, pct
                    );
                }
                None => {
                    println!(
                        "{indent}{:<30} {:>8.3}%  (wt {:5.1}%, no N-PORT)",
                        h.ticker, h.exposure_pct, weight_pct
                    );
                }
            }
        }
    }
}

pub fn print_unknown_munis(b: &TaxBreakdown) {
    if b.unknown_munis.is_empty() {
        println!("\nNo unknown municipal holdings.");
        return;
    }
    let total: usize = b.unknown_munis.iter().map(|(_, c)| c).sum();
    println!(
        "\n--- Unknown Municipal Issuers ({} unique, {} holdings) ---",
        b.unknown_munis.len(),
        total
    );
    for (name, count) in &b.unknown_munis {
        println!("  {:4}x  {}", count, name);
    }
}
