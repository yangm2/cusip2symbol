use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    accession: String,
    report_date: String,
    xml: String,
}

fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cache").join("cusip2symbol")
}

fn cache_path(ticker: &str) -> PathBuf {
    cache_dir().join(format!("{}.json", ticker.to_uppercase()))
}

/// Read cached N-PORT XML for a ticker if the accession matches.
/// Returns Some(xml) if the cache is valid for the given accession.
pub fn get(ticker: &str, accession: &str) -> Option<String> {
    let path = cache_path(ticker);
    let data = fs::read_to_string(&path).ok()?;
    let entry: CacheEntry = serde_json::from_str(&data).ok()?;
    if entry.accession == accession {
        Some(entry.xml)
    } else {
        None
    }
}

/// Store N-PORT XML in the cache.
pub fn put(ticker: &str, accession: &str, report_date: &str, xml: &str) {
    let dir = cache_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let entry = CacheEntry {
        accession: accession.to_string(),
        report_date: report_date.to_string(),
        xml: xml.to_string(),
    };
    if let Ok(json) = serde_json::to_string(&entry) {
        let _ = fs::write(cache_path(ticker), json);
    }
}

/// List all cached entries with their tickers, accessions, and report dates.
pub fn list_entries() -> Vec<(String, String, String)> {
    let dir = cache_dir();
    let mut entries = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(ticker) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(data) = fs::read_to_string(&path) {
                        if let Ok(ce) = serde_json::from_str::<CacheEntry>(&data) {
                            entries.push((
                                ticker.to_string(),
                                ce.accession,
                                ce.report_date,
                            ));
                        }
                    }
                }
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}
