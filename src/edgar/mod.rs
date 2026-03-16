mod nport;
mod states;

pub use nport::HoldingField;
pub use nport::TaxBreakdown;
pub use nport::find_holding;

use crate::sec_client::{FundInfo, SecClient};
use nport::parse_nport_xml;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

/// Download the latest N-PORT XML for a fund ticker, using cache when possible.
pub async fn download_nport_xml(
    client: &SecClient,
    ticker: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let fund = client.lookup_fund_info(ticker).await?;
    let (accession, xml_url) = find_latest_nport(client, &fund).await?;

    if let Some(xml) = crate::cache::get(ticker, &accession) {
        return Ok(xml);
    }

    let resp = client.get(&xml_url).await?;
    if !resp.status().is_success() {
        return Err(format!("Failed to download N-PORT: {}", resp.status()).into());
    }
    let xml = resp.text().await?;

    // Quick extract of report date for cache metadata (avoid full parse)
    let report_date = extract_report_date(&xml);
    crate::cache::put(ticker, &accession, &report_date, &xml);

    Ok(xml)
}

/// Download and parse the latest N-PORT into a tax breakdown.
pub async fn lookup_tax_breakdown(
    client: &SecClient,
    ticker: &str,
) -> Result<TaxBreakdown, Box<dyn std::error::Error>> {
    let xml = download_nport_xml(client, ticker).await?;
    parse_nport_xml(&xml)
}

async fn find_latest_nport(
    client: &SecClient,
    fund: &FundInfo,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let url = format!(
        "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={}&type=NPORT-P&count=5&output=atom",
        fund.series_id
    );

    let resp = client.get(&url).await?;
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

/// Quick string-level extraction of repPdDate (avoids full XML parse).
fn extract_report_date(xml: &str) -> String {
    // Look for <repPdDate>YYYY-MM-DD</repPdDate> or namespaced variant
    if let Some(start) = xml.find("repPdDate>") {
        let after = &xml[start + "repPdDate>".len()..];
        if let Some(end) = after.find('<') {
            return after[..end].trim().to_string();
        }
    }
    String::new()
}

/// Extract the local name from a potentially namespaced XML tag.
pub(super) fn local_name(full: &[u8]) -> String {
    let s = std::str::from_utf8(full).unwrap_or("");
    match s.rfind(':') {
        Some(i) => s[i + 1..].to_string(),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sec_client() -> SecClient {
        // Tests need a contact email; use a placeholder if not set
        if std::env::var("SEC_CONTACT_EMAIL").is_err() {
            // SAFETY: tests are run with --test-threads=1 for SEC rate limiting anyway
            unsafe { std::env::set_var("SEC_CONTACT_EMAIL", "sectool-test@example.com") };
        }
        SecClient::new().expect("Failed to build SecClient")
    }

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
        let client = test_sec_client();
        let fund = client
            .lookup_fund_info("IVV")
            .await
            .expect("Fund lookup failed");
        assert_eq!(fund.cik, 1100663);
        assert_eq!(fund.series_id, "S000004310");
    }

    #[tokio::test]
    async fn test_fund_info_not_found() {
        let client = test_sec_client();
        let result = client.lookup_fund_info("ZZZZNOTREAL").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tax_breakdown_treasury_fund() {
        let client = test_sec_client();
        let b = lookup_tax_breakdown(&client, "IEF")
            .await
            .expect("Tax breakdown failed");
        assert!(b.us_gov_total_pct > 90.0, "IEF should be >90% US Gov't");
        assert!(b.us_treasury_pct > 90.0, "IEF should be >90% Treasury");
    }

    #[tokio::test]
    async fn test_tax_breakdown_muni_fund() {
        let client = test_sec_client();
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
