use super::local_name;
use super::states::guess_state_from_name;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::BTreeMap;

// --- Raw parsed data ---

struct NportHeader {
    fund_name: String,
    report_date: String,
}

struct RawHolding {
    name: String,
    cusip: String,
    ticker: String,
    issuer_cat: String,
    pct_val: f64,
}

struct NportData {
    header: NportHeader,
    holdings: Vec<RawHolding>,
}

// --- Public types ---

#[derive(Debug)]
pub struct TaxBreakdown {
    pub report_date: String,
    pub fund_name: String,
    pub total_holdings: usize,
    pub us_treasury_pct: f64,
    pub us_gov_agency_pct: f64,
    pub us_gov_gse_pct: f64,
    pub us_gov_total_pct: f64,
    pub municipal_pct: f64,
    pub municipal_by_state: BTreeMap<String, f64>,
    pub corporate_pct: f64,
    pub other_pct: f64,
    pub unknown_munis: Vec<(String, usize)>,
}

pub enum HoldingField {
    Cusip,
    Ticker,
}

// --- Single XML parser ---

fn parse_nport(xml: &str) -> Result<NportData, Box<dyn std::error::Error>> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();

    let mut fund_name = String::new();
    let mut report_date = String::new();
    let mut holdings: Vec<RawHolding> = Vec::new();

    let mut in_gen_info = false;
    let mut in_invst = false;
    let mut current_tag = String::new();
    let mut text_accum = String::new();

    let mut h_name = String::new();
    let mut h_cusip = String::new();
    let mut h_ticker = String::new();
    let mut h_issuer_cat = String::new();
    let mut h_pct_val: f64 = 0.0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local = local_name(e.name().as_ref());
                current_tag = local.clone();
                text_accum.clear();

                if local == "genInfo" {
                    in_gen_info = true;
                } else if local == "invstOrSec" {
                    in_invst = true;
                    h_name.clear();
                    h_cusip.clear();
                    h_ticker.clear();
                    h_issuer_cat.clear();
                    h_pct_val = 0.0;
                } else if local == "issuerConditional" && in_invst {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"desc" {
                            h_issuer_cat = String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Ok(text) = e.decode() {
                    text_accum.push_str(&text);
                }
            }
            Ok(Event::GeneralRef(ref e)) => {
                let bytes: &[u8] = e.as_ref();
                match bytes {
                    b"amp" => text_accum.push('&'),
                    b"lt" => text_accum.push('<'),
                    b"gt" => text_accum.push('>'),
                    b"apos" => text_accum.push('\''),
                    b"quot" => text_accum.push('"'),
                    other => {
                        text_accum.push('&');
                        text_accum.push_str(&String::from_utf8_lossy(other));
                        text_accum.push(';');
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local = local_name(e.name().as_ref());

                if in_gen_info {
                    if current_tag == "seriesName" {
                        fund_name = std::mem::take(&mut text_accum);
                    } else if current_tag == "repPdDate" {
                        report_date = std::mem::take(&mut text_accum);
                    }
                } else if in_invst {
                    if current_tag == "name" {
                        h_name = std::mem::take(&mut text_accum);
                    } else if current_tag == "cusip" {
                        h_cusip = std::mem::take(&mut text_accum);
                    } else if current_tag == "ticker" {
                        h_ticker = std::mem::take(&mut text_accum);
                    } else if current_tag == "issuerCat" {
                        h_issuer_cat = std::mem::take(&mut text_accum);
                    } else if current_tag == "pctVal" {
                        h_pct_val = text_accum.parse().unwrap_or(0.0);
                    }
                }
                text_accum.clear();

                if local == "genInfo" {
                    in_gen_info = false;
                } else if local == "invstOrSec" {
                    in_invst = false;
                    holdings.push(RawHolding {
                        name: std::mem::take(&mut h_name),
                        cusip: std::mem::take(&mut h_cusip),
                        ticker: std::mem::take(&mut h_ticker),
                        issuer_cat: std::mem::take(&mut h_issuer_cat),
                        pct_val: h_pct_val,
                    });
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("N-PORT XML parse error: {e}").into()),
            _ => {}
        }
        buf.clear();
    }

    Ok(NportData {
        header: NportHeader {
            fund_name,
            report_date,
        },
        holdings,
    })
}

// --- Public API ---

pub fn parse_nport_xml(xml: &str) -> Result<TaxBreakdown, Box<dyn std::error::Error>> {
    let data = parse_nport(xml)?;

    let mut us_treasury_pct = 0.0;
    let mut us_gov_agency_pct = 0.0;
    let mut us_gov_gse_pct = 0.0;
    let mut municipal_pct = 0.0;
    let mut corporate_pct = 0.0;
    let mut other_pct = 0.0;
    let mut municipal_by_state: BTreeMap<String, f64> = BTreeMap::new();
    let mut unknown_muni_counts: BTreeMap<String, usize> = BTreeMap::new();

    for h in &data.holdings {
        match h.issuer_cat.as_str() {
            "UST" => us_treasury_pct += h.pct_val,
            "USGA" => us_gov_agency_pct += h.pct_val,
            "USGSE" => us_gov_gse_pct += h.pct_val,
            "MUN" => {
                municipal_pct += h.pct_val;
                let state = guess_state_from_name(&h.name);
                if state == "Unknown" {
                    *unknown_muni_counts.entry(h.name.clone()).or_default() += 1;
                }
                *municipal_by_state.entry(state).or_default() += h.pct_val;
            }
            "CORP" => corporate_pct += h.pct_val,
            _ => other_pct += h.pct_val,
        }
    }

    let mut unknown_munis: Vec<(String, usize)> = unknown_muni_counts.into_iter().collect();
    unknown_munis.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let us_gov_total_pct = us_treasury_pct + us_gov_agency_pct + us_gov_gse_pct;

    Ok(TaxBreakdown {
        report_date: data.header.report_date,
        fund_name: data.header.fund_name,
        total_holdings: data.holdings.len(),
        us_treasury_pct,
        us_gov_agency_pct,
        us_gov_gse_pct,
        us_gov_total_pct,
        municipal_pct,
        municipal_by_state,
        corporate_pct,
        other_pct,
        unknown_munis,
    })
}

/// Search an N-PORT XML for a holding by CUSIP or ticker.
/// Returns (name, total_pct) where total_pct is the sum of all matching holdings.
pub fn find_holding(
    xml: &str,
    field: HoldingField,
    target: &str,
) -> Result<Option<(String, f64)>, Box<dyn std::error::Error>> {
    let data = parse_nport(xml)?;
    let target_upper = target.to_uppercase();

    let mut total_pct = 0.0;
    let mut found_name = String::new();

    for h in &data.holdings {
        let value = match field {
            HoldingField::Cusip => &h.cusip,
            HoldingField::Ticker => &h.ticker,
        };
        if value.to_uppercase() == target_upper {
            total_pct += h.pct_val;
            if found_name.is_empty() {
                found_name = h.name.clone();
            }
        }
    }

    if total_pct > 0.0 {
        Ok(Some((found_name, total_pct)))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nport_xml_government() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <edgarSubmission xmlns="http://www.sec.gov/edgar/nport">
          <formData>
            <genInfo>
              <seriesName>Test Treasury Fund</seriesName>
              <repPdDate>2025-12-31</repPdDate>
            </genInfo>
            <invstOrSecs>
              <invstOrSec>
                <name>US Treasury Bond 2030</name>
                <issuerCat>UST</issuerCat>
                <pctVal>60.00</pctVal>
              </invstOrSec>
              <invstOrSec>
                <name>Fannie Mae MBS</name>
                <issuerCat>USGSE</issuerCat>
                <pctVal>25.00</pctVal>
              </invstOrSec>
              <invstOrSec>
                <name>Acme Corp Bond</name>
                <issuerCat>CORP</issuerCat>
                <pctVal>15.00</pctVal>
              </invstOrSec>
            </invstOrSecs>
          </formData>
        </edgarSubmission>"#;

        let b = parse_nport_xml(xml).unwrap();
        assert_eq!(b.fund_name, "Test Treasury Fund");
        assert_eq!(b.report_date, "2025-12-31");
        assert_eq!(b.total_holdings, 3);
        assert!((b.us_treasury_pct - 60.0).abs() < 0.01);
        assert!((b.us_gov_gse_pct - 25.0).abs() < 0.01);
        assert!((b.us_gov_total_pct - 85.0).abs() < 0.01);
        assert!((b.corporate_pct - 15.0).abs() < 0.01);
        assert!((b.municipal_pct - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_nport_xml_municipal_by_state() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <edgarSubmission xmlns="http://www.sec.gov/edgar/nport">
          <formData>
            <genInfo>
              <seriesName>Test Muni Fund</seriesName>
              <repPdDate>2025-11-30</repPdDate>
            </genInfo>
            <invstOrSecs>
              <invstOrSec>
                <name>State of California GO Bonds</name>
                <issuerCat>MUN</issuerCat>
                <pctVal>40.00</pctVal>
              </invstOrSec>
              <invstOrSec>
                <name>New York City Transitional Finance</name>
                <issuerCat>MUN</issuerCat>
                <pctVal>30.00</pctVal>
              </invstOrSec>
              <invstOrSec>
                <name>Oregon Health Authority Revenue</name>
                <issuerCat>MUN</issuerCat>
                <pctVal>20.00</pctVal>
              </invstOrSec>
              <invstOrSec>
                <name>Some Random Municipal Authority</name>
                <issuerCat>MUN</issuerCat>
                <pctVal>10.00</pctVal>
              </invstOrSec>
            </invstOrSecs>
          </formData>
        </edgarSubmission>"#;

        let b = parse_nport_xml(xml).unwrap();
        assert_eq!(b.total_holdings, 4);
        assert!((b.municipal_pct - 100.0).abs() < 0.01);
        assert!((b.municipal_by_state["CA"] - 40.0).abs() < 0.01);
        assert!((b.municipal_by_state["NY"] - 30.0).abs() < 0.01);
        assert!((b.municipal_by_state["OR"] - 20.0).abs() < 0.01);
        assert!((b.municipal_by_state["Unknown"] - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_nport_xml_entity_in_name() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <edgarSubmission xmlns="http://www.sec.gov/edgar/nport">
          <formData>
            <genInfo>
              <seriesName>iShares Core S&amp;P 500 ETF</seriesName>
              <repPdDate>2025-12-31</repPdDate>
            </genInfo>
            <invstOrSecs>
              <invstOrSec>
                <name>Apple Inc</name>
                <issuerCat>CORP</issuerCat>
                <pctVal>7.00</pctVal>
              </invstOrSec>
            </invstOrSecs>
          </formData>
        </edgarSubmission>"#;

        let b = parse_nport_xml(xml).unwrap();
        assert_eq!(b.fund_name, "iShares Core S&P 500 ETF");
    }

    #[test]
    fn test_parse_nport_xml_issuer_conditional() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <edgarSubmission xmlns="http://www.sec.gov/edgar/nport">
          <formData>
            <genInfo>
              <seriesName>Test Fund</seriesName>
              <repPdDate>2025-12-31</repPdDate>
            </genInfo>
            <invstOrSecs>
              <invstOrSec>
                <name>Some REIT</name>
                <issuerConditional desc="REIT" issuerCat="OTHER"/>
                <pctVal>5.00</pctVal>
              </invstOrSec>
            </invstOrSecs>
          </formData>
        </edgarSubmission>"#;

        let b = parse_nport_xml(xml).unwrap();
        assert_eq!(b.total_holdings, 1);
        assert!((b.other_pct - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_find_holding_by_cusip() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <edgarSubmission xmlns="http://www.sec.gov/edgar/nport">
          <formData>
            <genInfo>
              <seriesName>Test Fund</seriesName>
              <repPdDate>2025-12-31</repPdDate>
            </genInfo>
            <invstOrSecs>
              <invstOrSec>
                <name>APPLE INC</name>
                <cusip>037833100</cusip>
                <ticker>AAPL</ticker>
                <issuerCat>CORP</issuerCat>
                <pctVal>5.00</pctVal>
              </invstOrSec>
              <invstOrSec>
                <name>MICROSOFT CORP</name>
                <cusip>594918104</cusip>
                <ticker>MSFT</ticker>
                <issuerCat>CORP</issuerCat>
                <pctVal>4.00</pctVal>
              </invstOrSec>
            </invstOrSecs>
          </formData>
        </edgarSubmission>"#;

        let result = find_holding(xml, HoldingField::Cusip, "037833100").unwrap();
        let (name, pct) = result.unwrap();
        assert_eq!(name, "APPLE INC");
        assert!((pct - 5.0).abs() < 0.01);

        let result = find_holding(xml, HoldingField::Cusip, "999999999").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_holding_by_ticker() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <edgarSubmission xmlns="http://www.sec.gov/edgar/nport">
          <formData>
            <genInfo>
              <seriesName>Test Fund</seriesName>
              <repPdDate>2025-12-31</repPdDate>
            </genInfo>
            <invstOrSecs>
              <invstOrSec>
                <name>APPLE INC</name>
                <cusip>037833100</cusip>
                <ticker>AAPL</ticker>
                <issuerCat>CORP</issuerCat>
                <pctVal>5.00</pctVal>
              </invstOrSec>
            </invstOrSecs>
          </formData>
        </edgarSubmission>"#;

        let result = find_holding(xml, HoldingField::Ticker, "AAPL").unwrap();
        let (name, pct) = result.unwrap();
        assert_eq!(name, "APPLE INC");
        assert!((pct - 5.0).abs() < 0.01);

        let result = find_holding(xml, HoldingField::Ticker, "aapl").unwrap();
        assert!(result.is_some(), "should be case-insensitive");

        let result = find_holding(xml, HoldingField::Ticker, "ZZZZ").unwrap();
        assert!(result.is_none());
    }
}
