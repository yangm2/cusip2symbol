mod nport;
mod states;

pub use nport::TaxBreakdown;

use nport::parse_nport_xml;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct MfTickers {
    #[allow(dead_code)]
    fields: Vec<String>,
    data: Vec<(u64, String, String, String)>, // cik, seriesId, classId, symbol
}

#[derive(Debug)]
pub struct FundInfo {
    pub cik: u64,
    pub series_id: String,
}

pub async fn lookup_fund_info(
    client: &reqwest::Client,
    ticker: &str,
) -> Result<FundInfo, Box<dyn std::error::Error>> {
    let resp = client
        .get("https://www.sec.gov/files/company_tickers_mf.json")
        .header("Accept-Encoding", "gzip, deflate")
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(format!("SEC tickers API failed: {}", resp.status()).into());
    }

    let mf: MfTickers = resp.json().await?;
    let ticker_upper = ticker.to_uppercase();

    for (cik, series_id, _class_id, symbol) in &mf.data {
        if symbol.to_uppercase() == ticker_upper {
            return Ok(FundInfo {
                cik: *cik,
                series_id: series_id.clone(),
            });
        }
    }

    Err(format!("Ticker '{ticker}' not found in SEC mutual fund/ETF list").into())
}

async fn find_latest_nport(
    client: &reqwest::Client,
    fund: &FundInfo,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let url = format!(
        "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={}&type=NPORT-P&count=5&output=atom",
        fund.series_id
    );

    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("EDGAR browse failed: {}", resp.status()).into());
    }

    let text = resp.text().await?;

    let accession = parse_atom_accession(&text)?;
    let accession_no_dashes = accession.replace('-', "");
    let xml_url = format!(
        "https://www.sec.gov/Archives/edgar/data/{}/{}/primary_doc.xml",
        fund.cik, accession_no_dashes
    );

    Ok((accession, xml_url))
}

fn parse_atom_accession(xml: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut reader = Reader::from_str(xml);
    let mut in_entry = false;
    let mut in_accession = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == "entry" {
                    in_entry = true;
                } else if in_entry && local == "accession-number" {
                    in_accession = true;
                }
            }
            Ok(Event::Text(ref e)) if in_accession => {
                return Ok(e.decode()?.to_string());
            }
            Ok(Event::End(ref e)) => {
                let local = local_name(e.name().as_ref());
                if local == "entry" {
                    in_entry = false;
                } else if local == "accession-number" {
                    in_accession = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {e}").into()),
            _ => {}
        }
        buf.clear();
    }

    Err("No N-PORT filing found".into())
}

async fn download_nport(
    client: &reqwest::Client,
    xml_url: &str,
) -> Result<TaxBreakdown, Box<dyn std::error::Error>> {
    let resp = client.get(xml_url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("Failed to download N-PORT: {}", resp.status()).into());
    }

    let xml = resp.text().await?;
    parse_nport_xml(&xml)
}

pub async fn lookup_tax_breakdown(
    client: &reqwest::Client,
    ticker: &str,
) -> Result<TaxBreakdown, Box<dyn std::error::Error>> {
    eprintln!("Looking up fund info for {ticker}...");
    let fund = lookup_fund_info(client, ticker).await?;
    eprintln!(
        "Found CIK {} series {} — finding latest N-PORT...",
        fund.cik, fund.series_id
    );

    let (_accession, xml_url) = find_latest_nport(client, &fund).await?;
    eprintln!("Downloading and parsing N-PORT...");

    download_nport(client, &xml_url).await
}

/// Extract the local name from a potentially namespaced XML tag
pub(crate) fn local_name(full: &[u8]) -> String {
    let s = std::str::from_utf8(full).unwrap_or("");
    match s.rfind(':') {
        Some(i) => s[i + 1..].to_string(),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_client;

    #[test]
    fn test_parse_atom_accession_valid() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
          <entry>
            <accession-number>0001234567-26-000123</accession-number>
          </entry>
        </feed>"#;
        let result = parse_atom_accession(xml).unwrap();
        assert_eq!(result, "0001234567-26-000123");
    }

    #[test]
    fn test_parse_atom_accession_empty() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <feed xmlns="http://www.w3.org/2005/Atom">
        </feed>"#;
        assert!(parse_atom_accession(xml).is_err());
    }

    #[tokio::test]
    async fn test_fund_info_lookup() {
        let client = build_client();
        let fund = lookup_fund_info(&client, "IVV")
            .await
            .expect("Fund lookup failed");
        assert_eq!(fund.cik, 1100663);
        assert_eq!(fund.series_id, "S000004310");
    }

    #[tokio::test]
    async fn test_fund_info_not_found() {
        let client = build_client();
        let result = lookup_fund_info(&client, "ZZZZNOTREAL").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tax_breakdown_treasury_fund() {
        let client = build_client();
        let b = lookup_tax_breakdown(&client, "IEF")
            .await
            .expect("Tax breakdown failed");
        assert!(b.us_gov_total_pct > 90.0, "IEF should be >90% US Gov't");
        assert!(b.us_treasury_pct > 90.0, "IEF should be >90% Treasury");
    }

    #[tokio::test]
    async fn test_tax_breakdown_muni_fund() {
        let client = build_client();
        let b = lookup_tax_breakdown(&client, "MUB")
            .await
            .expect("Tax breakdown failed");
        assert!(b.municipal_pct > 90.0, "MUB should be >90% municipal");
        assert!(
            b.municipal_by_state.contains_key("CA"),
            "MUB should have California munis"
        );
        assert!(
            b.municipal_by_state.contains_key("NY"),
            "MUB should have New York munis"
        );
    }
}
