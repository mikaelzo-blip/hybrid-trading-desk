use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Supported market classes for the trading desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    Crypto,
    UsStock,
    Forex,
    IdxStock,
}

impl AssetClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Crypto => "crypto",
            Self::UsStock => "us_stock",
            Self::Forex => "forex",
            Self::IdxStock => "idx_stock",
        }
    }
}

impl fmt::Display for AssetClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AssetClass {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "crypto" => Ok(Self::Crypto),
            "us_stock" | "usstock" | "us-stock" => Ok(Self::UsStock),
            "forex" => Ok(Self::Forex),
            "idx_stock" | "idxstock" | "idx-stock" | "idx" => Ok(Self::IdxStock),
            other => Err(format!("unsupported asset_class: {other}")),
        }
    }
}

/// Supported candle timeframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timeframe {
    #[serde(rename = "1m")]
    M1,
    #[serde(rename = "5m")]
    M5,
    #[serde(rename = "15m")]
    M15,
    #[serde(rename = "1h")]
    H1,
    #[serde(rename = "4h")]
    H4,
    #[serde(rename = "1d")]
    D1,
}

impl Timeframe {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::M1 => "1m",
            Self::M5 => "5m",
            Self::M15 => "15m",
            Self::H1 => "1h",
            Self::H4 => "4h",
            Self::D1 => "1d",
        }
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Timeframe {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "1m" => Ok(Self::M1),
            "5m" => Ok(Self::M5),
            "15m" => Ok(Self::M15),
            "1h" => Ok(Self::H1),
            "4h" => Ok(Self::H4),
            "1d" => Ok(Self::D1),
            other => Err(format!("unsupported timeframe: {other}")),
        }
    }
}

/// Logical correlation bucket used by portfolio/risk guardrails.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CorrelationGroup(pub String);

impl CorrelationGroup {
    pub fn general() -> Self {
        Self("general".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CorrelationGroup {
    fn default() -> Self {
        Self::general()
    }
}

impl fmt::Display for CorrelationGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for CorrelationGroup {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for CorrelationGroup {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Unified OHLCV candle used across all asset classes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candle {
    pub asset_class: AssetClass,
    pub ticker: String,
    pub timeframe: Timeframe,
    pub open_time: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}
