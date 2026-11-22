#![allow(dead_code)]

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::models::features::EventRisk;
use crate::models::market::AssetClass;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EventRiskInput {
    pub now: Option<DateTime<Utc>>,
    pub earnings_within_hours: Option<i64>,
    pub forex_high_impact_within_minutes: Option<i64>,
    pub idx_fca_uma_suspend: bool,
    pub idx_foreign_flow_hook: Option<String>,
    pub crypto_funding_rate: Option<f64>, // e.g. 0.001 = 0.1%
}

pub fn evaluate(asset_class: AssetClass, ticker: &str, input: &EventRiskInput) -> EventRisk {
    let _ = ticker;
    let now = input.now.unwrap_or_else(Utc::now);
    let mut reasons = Vec::new();
    let mut blocked = false;
    let mut severity = "none".to_owned();

    match asset_class {
        AssetClass::UsStock => {
            if let Some(h) = input.earnings_within_hours {
                if h < 48 {
                    blocked = true;
                    severity = "blocked".into();
                    reasons.push(format!("Earnings dalam < 48 jam ({h}h)"));
                }
            }
        }
        AssetClass::Forex => {
            if let Some(m) = input.forex_high_impact_within_minutes {
                if m < 30 {
                    blocked = true;
                    severity = "blocked".into();
                    reasons.push(format!("High-impact forex news dalam < 30 menit ({m}m)"));
                }
            }
        }
        AssetClass::IdxStock => {
            if input.idx_fca_uma_suspend {
                blocked = true;
                severity = "blocked".into();
                reasons.push("IDX FCA/UMA/Suspend aktif".into());
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
