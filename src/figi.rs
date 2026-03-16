use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FigiResult {
    pub figi: Option<String>,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub exch_code: Option<String>,
    pub market_sector: Option<String>,
    pub security_type: Option<String>,
    pub security_type2: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FigiResponse {
    Success {
        data: Vec<FigiResult>,
    },
    Warning {
        #[allow(dead_code)]
        warning: String,
    },
    Error {
        error: String,
    },
}

pub async fn lookup_cusip(
    client: &reqwest::Client,
    cusip: &str,
) -> Result<Option<Vec<FigiResult>>, Box<dyn std::error::Error>> {
    figi_lookup(client, "ID_CUSIP", cusip).await
}

pub async fn lookup_ticker(
    client: &reqwest::Client,
    ticker: &str,
) -> Result<Option<Vec<FigiResult>>, Box<dyn std::error::Error>> {
    figi_lookup(client, "TICKER", &ticker.to_uppercase()).await
}

async fn figi_lookup(
    client: &reqwest::Client,
    id_type: &str,
    id_value: &str,
) -> Result<Option<Vec<FigiResult>>, Box<dyn std::error::Error>> {
    let body = serde_json::json!([{"idType": id_type, "idValue": id_value}]);

    let resp = client
        .post("https://api.openfigi.com/v3/mapping")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(format!("OpenFIGI API request failed: {}", resp.status()).into());
    }

    let results: Vec<FigiResponse> = resp.json().await?;

    match results.into_iter().next() {
        Some(FigiResponse::Success { data }) => Ok(Some(data)),
        Some(FigiResponse::Warning { .. }) => Ok(None),
        Some(FigiResponse::Error { error }) => Err(error.into()),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_client;

    #[tokio::test]
    async fn test_aapl_cusip() {
        let client = build_client();
        let data = lookup_cusip(&client, "037833100")
            .await
            .expect("API call failed");
        let data = data.expect("No results returned");

        assert!(!data.is_empty(), "Expected at least one result");

        let first = &data[0];
        assert_eq!(first.ticker.as_deref(), Some("AAPL"));
        assert_eq!(first.name.as_deref(), Some("APPLE INC"));
        assert_eq!(first.market_sector.as_deref(), Some("Equity"));
        assert_eq!(first.security_type.as_deref(), Some("Common Stock"));
    }

    #[tokio::test]
    async fn test_invalid_cusip_returns_none() {
        let client = build_client();
        let data = lookup_cusip(&client, "000000000")
            .await
            .expect("API call failed");
        assert!(data.is_none(), "Expected no results for invalid CUSIP");
    }
}
