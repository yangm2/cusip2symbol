use serde::Deserialize;
use std::sync::Mutex;
use tokio::time::{Duration, Instant};

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

/// Rate-limited SEC client that caches the mutual fund tickers list.
pub struct SecClient {
    http: reqwest::Client,
    /// Timestamps of recent SEC requests for rate limiting (max 8/sec to stay safely under 10/sec)
    recent_requests: Mutex<Vec<Instant>>,
    /// Cached mutual fund tickers data
    mf_tickers: tokio::sync::OnceCell<Vec<(u64, String, String, String)>>,
}

const SEC_RATE_LIMIT: usize = 8; // stay under SEC's 10/sec limit
const SEC_RATE_WINDOW: Duration = Duration::from_secs(1);

impl SecClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let email = std::env::var("SEC_CONTACT_EMAIL").map_err(|_| {
            "SEC_CONTACT_EMAIL environment variable is required.\n\
             Set it to your email address (SEC fair access policy):\n  \
             export SEC_CONTACT_EMAIL=you@example.com"
        })?;
        let user_agent = format!("sectool {email}");
        let http = reqwest::Client::builder().user_agent(user_agent).build()?;
        Ok(Self {
            http,
            recent_requests: Mutex::new(Vec::new()),
            mf_tickers: tokio::sync::OnceCell::new(),
        })
    }

    /// Rate-limited GET request to SEC.
    pub async fn get(&self, url: &str) -> reqwest::Result<reqwest::Response> {
        loop {
            let wait = {
                // Lock is held briefly with no panicking code inside; poisoning can't happen
                let mut recent = self.recent_requests.lock().unwrap();
                let now = Instant::now();
                recent.retain(|&t| now.duration_since(t) < SEC_RATE_WINDOW);
                if recent.len() < SEC_RATE_LIMIT {
                    recent.push(now);
                    None
                } else {
                    Some(SEC_RATE_WINDOW - now.duration_since(recent[0]))
                }
            };
            if let Some(delay) = wait {
                tokio::time::sleep(delay).await;
            } else {
                break;
            }
        }

        self.http.get(url).send().await
    }

    /// Look up fund info (CIK + series ID) by ticker. Caches the tickers list.
    pub async fn lookup_fund_info(
        &self,
        ticker: &str,
    ) -> Result<FundInfo, Box<dyn std::error::Error>> {
        let data = self.load_mf_tickers().await?;
        let ticker_upper = ticker.to_uppercase();

        for (cik, series_id, _class_id, symbol) in data {
            if symbol.to_uppercase() == ticker_upper {
                return Ok(FundInfo {
                    cik: *cik,
                    series_id: series_id.clone(),
                });
            }
        }

        Err(format!("Ticker '{ticker}' not found in SEC mutual fund/ETF list").into())
    }

    async fn load_mf_tickers(
        &self,
    ) -> Result<&[(u64, String, String, String)], Box<dyn std::error::Error>> {
        let data = self
            .mf_tickers
            .get_or_try_init(|| async {
                let resp = self
                    .get("https://www.sec.gov/files/company_tickers_mf.json")
                    .await?;
                if !resp.status().is_success() {
                    return Err(format!("SEC tickers API failed: {}", resp.status()).into());
                }
                let mf: MfTickers = resp.json().await?;
                Ok::<_, Box<dyn std::error::Error>>(mf.data)
            })
            .await?;
        Ok(data.as_slice())
    }
}
