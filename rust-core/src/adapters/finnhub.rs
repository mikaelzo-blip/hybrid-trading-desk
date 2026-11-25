use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::America;
use serde::Deserialize;

#[derive(Clone)]
pub struct FinnhubAdapter {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl FinnhubAdapter {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key,
            http,
        })
    }

    pub fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }

    pub async fn fetch_hours_until_earnings(
        &self,
        symbol: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<i64>> {
        let key = self
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow!("FINNHUB_API_KEY missing"))?;
        let from = now.date_naive();
        let to = (now + Duration::days(90)).date_naive();
        let api_base = self
            .base_url
            .strip_suffix("/api/v1")
            .unwrap_or(&self.base_url);
        let url = format!("{api_base}/api/v1/calendar/earnings");
        let resp = self
            .http
            .get(&url)
            .header("X-Finnhub-Token", key)
            .query(&[("from", from.to_string()), ("to", to.to_string())])
            .send()
            .await
            .context("finnhub earnings request failed")?;
        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!("finnhub earnings HTTP {status}"));
        }
        let body: EarningsResponse = resp
            .json()
            .await
            .context("invalid finnhub earnings response")?;

        let symbol = symbol.trim().to_uppercase();
        let mut nearest: Option<i64> = None;
        for entry in body.earnings_calendar {
            if !entry.symbol.eq_ignore_ascii_case(&symbol) {
                continue;
            }
            let Some(when) = earnings_time(&entry.date, entry.hour.as_deref()) else {
                continue;
            };
            let hours = crate::engine::event_risk::hours_until(when, now);
            if hours < 0 {
                continue;
            }
            nearest = Some(nearest.map_or(hours, |current| current.min(hours)));
        }
        Ok(nearest)
    }
}

#[derive(Debug, Deserialize)]
struct EarningsResponse {
    #[serde(rename = "earningsCalendar")]
    earnings_calendar: Vec<EarningsEntry>,
}

#[derive(Debug, Deserialize)]
struct EarningsEntry {
    symbol: String,
    date: String,
    hour: Option<String>,
}

/// Finnhub `bmo` = 09:30 ET, `dmh` = 13:00 ET, `amc` = 16:00 ET.
/// Unknown values fall back to market close (conservative: later than open).
fn earnings_time(date: &str, hour: Option<&str>) -> Option<DateTime<Utc>> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let (h, m) = match hour.unwrap_or_default() {
        "bmo" => (9, 30),
        "dmh" => (13, 0),
        "amc" => (16, 0),
        _ => (16, 0),
    };
    let naive = date.and_hms_opt(h, m, 0)?;
    America::New_York
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fixed_now() -> DateTime<Utc> {
        "2026-11-25T00:00:00Z".parse().unwrap()
    }

    #[tokio::test]
    async fn returns_hours_to_nearest_upcoming_earnings_for_symbol() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/calendar/earnings"))
            .and(header("X-Finnhub-Token", "k"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "earningsCalendar": [
                    {"symbol":"AAPL","date":"2026-12-20","hour":"bmo"},
                    {"symbol":"NVDA","date":"2026-11-26","hour":"bmo"}
                ]
            })))
            .mount(&server)
            .await;

        let adapter =
            FinnhubAdapter::new(format!("{}/api/v1", server.uri()), Some("k".into())).unwrap();
        let hours = adapter
            .fetch_hours_until_earnings("NVDA", fixed_now())
            .await
            .unwrap();

        // 2026-11-26 09:30 America/New_York = 14:30 UTC -> 38h30m from fixed_now.
        assert_eq!(hours, Some(38));
    }

    #[tokio::test]
    async fn distinguishes_empty_calendar_from_missing_calendar() {
        for (body, expected) in [
            (serde_json::json!({"earningsCalendar": []}), Some(None)),
            (serde_json::json!({"error": "unavailable"}), None),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/api/v1/calendar/earnings"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;

            let adapter = FinnhubAdapter::new(server.uri(), Some("k".into())).unwrap();
            let result = adapter
                .fetch_hours_until_earnings("NVDA", fixed_now())
                .await;
            assert_eq!(result.ok(), expected);
        }
    }

    #[tokio::test]
    async fn returns_none_when_symbol_has_no_earnings() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/calendar/earnings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "earningsCalendar": [{"symbol":"AAPL","date":"2026-12-20","hour":"bmo"}]
            })))
            .mount(&server)
            .await;

        let adapter = FinnhubAdapter::new(server.uri(), Some("k".into())).unwrap();
        let hours = adapter
            .fetch_hours_until_earnings("NVDA", fixed_now())
            .await
            .unwrap();

        assert_eq!(hours, None);
    }

    #[tokio::test]
    async fn ignores_past_earnings_dates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/calendar/earnings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "earningsCalendar": [{"symbol":"NVDA","date":"2026-11-20","hour":"amc"}]
            })))
            .mount(&server)
            .await;

        let adapter = FinnhubAdapter::new(server.uri(), Some("k".into())).unwrap();
        let hours = adapter
            .fetch_hours_until_earnings("NVDA", fixed_now())
            .await
            .unwrap();

        assert_eq!(hours, None);
    }
}
