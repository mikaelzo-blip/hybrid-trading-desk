#![allow(dead_code)]

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::models::market::{AssetClass, Timeframe};

/// Higher timeframe context paired to an execution timeframe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HigherTfContext {
    pub timeframe: Timeframe,
    pub trend_bias: String, // "bullish" | "bearish" | "neutral"
    pub ema_20: Option<Decimal>,
    pub ema_50: Option<Decimal>,
    pub ema_200: Option<Decimal>,
    pub adx_14: Option<f64>,
    pub rsi_14: Option<f64>,
    pub macd: Option<f64>,
    pub macd_signal: Option<f64>,
    pub nearest_support: Option<Decimal>,
    pub nearest_resistance: Option<Decimal>,
}

/// Execution timeframe computed features used by agents and guardrails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecTfFeatures {
    pub asset_class: AssetClass,
    pub ticker: String,
    pub timeframe: Timeframe,
    pub close: Decimal,
    pub ema_20: Option<Decimal>,
    pub ema_50: Option<Decimal>,
    pub ema_200: Option<Decimal>,
    pub adx_14: Option<f64>,
    pub rsi_14: Option<f64>,
    pub macd: Option<f64>,
    pub macd_signal: Option<f64>,
    pub macd_hist: Option<f64>,
    pub atr_14: Option<Decimal>,
    pub bb_upper: Option<Decimal>,
    pub bb_middle: Option<Decimal>,
    pub bb_lower: Option<Decimal>,
    pub volume_ratio: Option<f64>,
    pub vwap: Option<Decimal>,
    pub nearest_support: Option<Decimal>,
    pub nearest_resistance: Option<Decimal>,
    pub structure_notes: Vec<String>,
}

/// Intermarket / macro regime snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketRegime {
    pub asset_class: AssetClass,
    pub regime_label: String, // e.g. "risk_on", "risk_off", "mixed", "unknown"
    pub volatility_regime: String, // "low" | "normal" | "high" | "extreme"
    pub bias: String,         // "bullish" | "bearish" | "neutral"
    pub summary: String,
    pub details: serde_json::Value,
}

/// Event / calendar risk snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRisk {
    pub asset_class: AssetClass,
    pub blocked_entry: bool,
    pub severity: String, // "none" | "caution" | "high" | "blocked"
    pub reasons: Vec<String>,
    pub details: serde_json::Value,
}

impl Default for EventRisk {
    fn default() -> Self {
        Self {
            asset_class: AssetClass::Crypto,
            blocked_entry: false,
            severity: "none".to_owned(),
            reasons: Vec::new(),
            details: serde_json::json!({}),
        }
    }
}

/// Deterministic mapping: Execution TF -> Higher TF.
pub fn higher_timeframe_for(exec: Timeframe) -> Timeframe {
    match exec {
        Timeframe::M1 => Timeframe::M15,
        Timeframe::M5 => Timeframe::H1,
        Timeframe::M15 => Timeframe::H1,
        Timeframe::H1 => Timeframe::H4,
        Timeframe::H4 => Timeframe::D1,
        Timeframe::D1 => Timeframe::D1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_execution_to_higher_timeframe() {
        assert_eq!(higher_timeframe_for(Timeframe::M15), Timeframe::H1);
        assert_eq!(higher_timeframe_for(Timeframe::H1), Timeframe::H4);
        assert_eq!(higher_timeframe_for(Timeframe::H4), Timeframe::D1);
    }
}
