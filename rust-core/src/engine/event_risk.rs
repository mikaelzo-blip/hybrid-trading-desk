#![allow(dead_code)]

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::adapters::bea_calendar::BeaCalendar;
use crate::adapters::finnhub::FinnhubAdapter;
use crate::adapters::idx_status::{self, IdxFlags};
use crate::models::features::EventRisk;
use crate::models::market::AssetClass;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EventRiskInput {
    pub now: Option<DateTime<Utc>>,
    pub earnings_within_hours: Option<i64>,
    /// True when the earnings feed is unavailable: fail closed for US stocks.
    pub earnings_unknown: bool,
    pub forex_high_impact_within_minutes: Option<i64>,
    pub forex_calendar_status: Option<ForexCalendarStatus>,
    pub idx_fca_uma_suspend: bool,
    pub idx_snapshot_flags: Option<IdxFlags>,
    pub idx_foreign_flow_hook: Option<String>,
    pub crypto_funding_rate: Option<f64>, // e.g. 0.001 = 0.1%
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForexCalendarStatus {
    BeaUsdPartial,
    Unavailable,
    OutsideCoverage,
}

pub fn evaluate(asset_class: AssetClass, ticker: &str, input: &EventRiskInput) -> EventRisk {
    let _ = ticker;
    let now = input.now.unwrap_or_else(Utc::now);
    let mut reasons = Vec::new();
    let mut blocked = false;
    let mut severity = "none".to_owned();

    match asset_class {
        AssetClass::UsStock => {
            if input.earnings_unknown {
                blocked = true;
                severity = "blocked".into();
                reasons.push("Earnings feed unavailable - entry blocked (fail closed)".into());
            } else if let Some(h) = input.earnings_within_hours {
                if h < 48 {
                    blocked = true;
                    severity = "blocked".into();
                    reasons.push(format!("Earnings dalam < 48 jam ({h}h)"));
                }
            }
        }
        AssetClass::Forex => {
            match input.forex_calendar_status {
                Some(ForexCalendarStatus::Unavailable) => {
                    blocked = true;
                    reasons.push("Kalender BEA tidak tersedia; entry forex diblokir".into());
                }
                Some(ForexCalendarStatus::OutsideCoverage) => {
                    blocked = true;
                    reasons
                        .push("Pasangan forex di luar cakupan kalender USD; entry diblokir".into());
                }
                Some(ForexCalendarStatus::BeaUsdPartial) => {
                    severity = "caution".into();
                    reasons.push("Cakupan parsial: hanya rilis GDP/PCE dari BEA (USD)".into());
                }
                None => {}
            }
            if let Some(m) = input
                .forex_high_impact_within_minutes
                .filter(|m| (0..30).contains(m))
            {
                blocked = true;
                reasons.push(format!("High-impact forex news dalam < 30 menit ({m}m)"));
            }
            if blocked {
                severity = "blocked".into();
            }
        }
        AssetClass::IdxStock => {
            if input.idx_fca_uma_suspend
                || input
                    .idx_snapshot_flags
                    .is_some_and(|flags| flags.fca || flags.uma || flags.suspended)
            {
                blocked = true;
                severity = "blocked".into();
                reasons.push("IDX FCA/UMA/Suspend aktif".into());
            } else if input.idx_snapshot_flags.is_none() {
                blocked = true;
                severity = "blocked".into();
                reasons
                    .push("Status FCA/UMA/Suspend IDX belum terverifikasi; entry diblokir".into());
            }
            if let Some(flow) = &input.idx_foreign_flow_hook {
                if !flow.trim().is_empty() {
                    reasons.push(format!("Foreign flow hook: {flow}"));
                    if severity == "none" {
                        severity = "caution".into();
                    }
                }
            }
        }
        AssetClass::Crypto => {
            if let Some(fr) = input.crypto_funding_rate {
                if fr.abs() >= 0.001 {
                    // 0.1% extreme threshold
                    severity = "caution".into();
                    reasons.push(format!("Funding rate ekstrem: {:.4}%", fr * 100.0));
                    if fr.abs() >= 0.003 {
                        // very extreme -> blocked
                        blocked = true;
                        severity = "blocked".into();
                    }
                }
            }
        }
    }

    EventRisk {
        asset_class,
        blocked_entry: blocked,
        severity,
        reasons,
        details: serde_json::json!({
            "evaluated_at": now.to_rfc3339(),
            "input": input,
        }),
    }
}

/// Builds `EventRiskInput` from deterministic feeds at the Rust boundary.
/// LLM workers must never fetch or recompute this themselves.
pub async fn evaluate_with_feeds(
    finnhub: &FinnhubAdapter,
    bea: &BeaCalendar,
    idx_snapshot: &Path,
    asset_class: AssetClass,
    ticker: &str,
) -> EventRisk {
    let now = Utc::now();
    let mut input = EventRiskInput {
        now: Some(now),
        ..Default::default()
    };
    if matches!(asset_class, AssetClass::UsStock) {
        if !finnhub.is_configured() {
            input.earnings_unknown = true;
        } else {
            match finnhub.fetch_hours_until_earnings(ticker, now).await {
                Ok(hours) => input.earnings_within_hours = hours,
                Err(_) => input.earnings_unknown = true,
            }
        }
    } else if asset_class == AssetClass::Forex {
        input.forex_calendar_status = Some(if !is_usd_pair(ticker) {
            ForexCalendarStatus::OutsideCoverage
        } else {
            match bea.next_release_minutes(now).await {
                Ok(minutes) => {
                    input.forex_high_impact_within_minutes = Some(minutes);
                    ForexCalendarStatus::BeaUsdPartial
                }
                Err(_) => ForexCalendarStatus::Unavailable,
            }
        });
    } else if asset_class == AssetClass::IdxStock {
        input.idx_snapshot_flags = idx_status::read_status(idx_snapshot, ticker, now)
            .await
            .ok()
            .flatten();
        input.idx_fca_uma_suspend = input
            .idx_snapshot_flags
            .is_some_and(|flags| flags.fca || flags.uma || flags.suspended);
    }
    evaluate(asset_class, ticker, &input)
}

fn is_usd_pair(ticker: &str) -> bool {
    let ticker = ticker.trim().to_ascii_uppercase();
    let Some((base, quote)) = ticker
        .split_once('/')
        .or_else(|| (ticker.len() == 6 && ticker.is_ascii()).then(|| ticker.split_at(3)))
    else {
        return false;
    };
    let valid =
        |currency: &str| currency.len() == 3 && currency.bytes().all(|c| c.is_ascii_uppercase());
    base != quote && valid(base) && valid(quote) && (base == "USD" || quote == "USD")
}

/// Convenience helper for simple checks.
pub fn hours_until(ts: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    (ts - now).num_hours()
}

pub fn minutes_until(ts: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    (ts - now).num_minutes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn us_stock_earnings_blocks_under_48h() {
        let risk = evaluate(
            AssetClass::UsStock,
            "NVDA",
            &EventRiskInput {
                earnings_within_hours: Some(24),
                ..Default::default()
            },
        );
        assert!(risk.blocked_entry);
        assert_eq!(risk.severity, "blocked");
    }

    #[test]
    fn us_stock_blocks_when_earnings_feed_is_unavailable() {
        let risk = evaluate(
            AssetClass::UsStock,
            "NVDA",
            &EventRiskInput {
                earnings_unknown: true,
                ..Default::default()
            },
        );
        assert!(risk.blocked_entry);
        assert!(risk
            .reasons
            .iter()
            .any(|r| r.contains("Earnings feed unavailable")));
    }

    #[tokio::test]
    async fn us_stock_fails_closed_when_finnhub_key_is_missing() {
        let adapter =
            crate::adapters::finnhub::FinnhubAdapter::new("http://127.0.0.1:1", None).unwrap();
        let bea = BeaCalendar::new("http://127.0.0.1:1").unwrap();
        let risk = evaluate_with_feeds(
            &adapter,
            &bea,
            Path::new("unused"),
            AssetClass::UsStock,
            "NVDA",
        )
        .await;
        assert!(risk.blocked_entry);
        assert!(risk
            .reasons
            .iter()
            .any(|r| r.contains("Earnings feed unavailable")));
    }

    #[test]
    fn us_stock_allows_when_no_upcoming_earnings_are_scheduled() {
        let risk = evaluate(AssetClass::UsStock, "NVDA", &EventRiskInput::default());
        assert!(!risk.blocked_entry);
        assert_eq!(risk.severity, "none");
    }

    #[test]
    fn forex_high_impact_blocks_under_30m() {
        let risk = evaluate(
            AssetClass::Forex,
            "EURUSD",
            &EventRiskInput {
                forex_high_impact_within_minutes: Some(10),
                ..Default::default()
            },
        );
        assert!(risk.blocked_entry);
    }

    #[test]
    fn partial_calendar_warns_without_blocking_at_exactly_30_minutes() {
        let risk = evaluate(
            AssetClass::Forex,
            "EURUSD",
            &EventRiskInput {
                forex_high_impact_within_minutes: Some(30),
                forex_calendar_status: Some(ForexCalendarStatus::BeaUsdPartial),
                ..Default::default()
            },
        );
        assert!(!risk.blocked_entry);
        assert_eq!(risk.severity, "caution");
    }

    #[tokio::test]
    async fn forex_calendar_blocks_near_release_outage_and_uncovered_pair() {
        use chrono::Duration;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let finnhub = FinnhubAdapter::new(server.uri(), None).unwrap();
        let release = (Utc::now() + Duration::minutes(15)).format("%Y%m%dT%H%M%SZ");
        let body = format!(
            "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:GDP (Advance Estimate)\n\
             DTSTART:{release}\nEND:VEVENT\nEND:VCALENDAR"
        );
        Mock::given(method("GET"))
            .and(path("/calendar"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;

        let bea = BeaCalendar::new(format!("{}/calendar", server.uri())).unwrap();
        for ticker in ["EURUSD", "USD/JPY"] {
            let risk = evaluate_with_feeds(
                &finnhub,
                &bea,
                Path::new("unused"),
                AssetClass::Forex,
                ticker,
            )
            .await;
            assert!(risk.blocked_entry);
            assert!(risk.reasons.iter().any(|r| r.contains("< 30 menit")));
        }
        let uncovered = evaluate_with_feeds(
            &finnhub,
            &bea,
            Path::new("unused"),
            AssetClass::Forex,
            "EURJPY",
        )
        .await;
        assert!(uncovered.blocked_entry);
        assert_eq!(
            uncovered.details["input"]["forex_calendar_status"],
            "outside_coverage"
        );

        let offline = BeaCalendar::new(format!("{}/unavailable", server.uri())).unwrap();
        let risk = evaluate_with_feeds(
            &finnhub,
            &offline,
            Path::new("unused"),
            AssetClass::Forex,
            "EURUSD",
        )
        .await;
        assert!(risk.blocked_entry);
        assert_eq!(
            risk.details["input"]["forex_calendar_status"],
            "unavailable"
        );
    }

    #[test]
    fn idx_fca_uma_blocks() {
        let risk = evaluate(
            AssetClass::IdxStock,
            "BBCA.JK",
            &EventRiskInput {
                idx_fca_uma_suspend: true,
                idx_foreign_flow_hook: Some("net_sell".into()),
                ..Default::default()
            },
        );
        assert!(risk.blocked_entry);
        assert!(risk.reasons.iter().any(|r| r.contains("FCA/UMA")));
    }

    #[tokio::test]
    async fn idx_snapshot_allows_only_explicitly_clear_stock() {
        let path = std::env::temp_dir().join(format!("idx-status-{}.json", uuid::Uuid::new_v4()));
        let finnhub = FinnhubAdapter::new("http://127.0.0.1:1", None).unwrap();
        let bea = BeaCalendar::new("http://127.0.0.1:1").unwrap();
        let missing =
            evaluate_with_feeds(&finnhub, &bea, &path, AssetClass::IdxStock, "SAFE.JK").await;
        let body = serde_json::json!({
            "verified_at": Utc::now(),
            "symbols": [
                {"ticker":"SAFE","fca":false,"uma":false,"suspended":false},
                {"ticker":"TEST","fca":false,"uma":false,"suspended":true}
            ]
        });
        tokio::fs::write(&path, body.to_string()).await.unwrap();
        let clear =
            evaluate_with_feeds(&finnhub, &bea, &path, AssetClass::IdxStock, "SAFE.JK").await;
        let suspended =
            evaluate_with_feeds(&finnhub, &bea, &path, AssetClass::IdxStock, "TEST.JK").await;
        let absent =
            evaluate_with_feeds(&finnhub, &bea, &path, AssetClass::IdxStock, "ABSENT.JK").await;
        tokio::fs::remove_file(&path).await.unwrap();
        assert!(missing.blocked_entry);
        assert!(!clear.blocked_entry);
        assert!(suspended.blocked_entry);
        assert_eq!(
            suspended.details["input"]["idx_snapshot_flags"]["suspended"],
            true
        );
        assert!(absent.blocked_entry);
    }

    #[test]
    fn crypto_extreme_funding_caution_or_block() {
        let caution = evaluate(
            AssetClass::Crypto,
            "BTCUSDT",
            &EventRiskInput {
                crypto_funding_rate: Some(0.0015),
                ..Default::default()
            },
        );
        assert!(!caution.blocked_entry);
        assert_eq!(caution.severity, "caution");
        let blocked = evaluate(
            AssetClass::Crypto,
            "BTCUSDT",
            &EventRiskInput {
                crypto_funding_rate: Some(0.0035),
                ..Default::default()
            },
        );
        assert!(blocked.blocked_entry);
    }
}
