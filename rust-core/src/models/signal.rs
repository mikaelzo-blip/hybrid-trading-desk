#![allow(dead_code)]

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::features::{EventRisk, MarketRegime};
use crate::models::market::{AssetClass, CorrelationGroup, Timeframe};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Action {
    Buy,
    Hold,
    Sell,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Hold => "HOLD",
            Self::Sell => "SELL",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSignal {
    pub id: Uuid,
    pub asset_class: AssetClass,
    pub ticker: String,
    pub correlation_group: CorrelationGroup,
    pub timeframe: Timeframe,
    pub candle_timestamp: DateTime<Utc>,
    pub action: Action,
    pub confidence: f64,
    pub entry_price: Option<Decimal>,
    pub live_price_at_decision: Option<Decimal>,
    pub entry_drift_pct: Option<Decimal>,
    pub stop_loss: Option<Decimal>,
    pub take_profit: Option<Decimal>,
    pub guardrail_approved: bool,
    pub reject_reason: Option<String>,
    pub risk_reward_ratio: Option<Decimal>,
    pub position_size: Option<Decimal>,
    pub llm_provider_used: Option<String>,
    pub market_regime_snapshot: Option<MarketRegime>,
    pub event_risk_snapshot: Option<EventRisk>,
    pub reasoning_summary: String,
    pub debate_log: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Portfolio/risk state used by guardrail circuit breaker and concentration checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioRiskState {
    pub daily_pnl_pct: Decimal,
    pub weekly_pnl_pct: Decimal,
    pub circuit_breaker_active: bool,
    pub open_positions: Vec<OpenPosition>,
}

impl Default for PortfolioRiskState {
    fn default() -> Self {
        Self {
            daily_pnl_pct: Decimal::ZERO,
            weekly_pnl_pct: Decimal::ZERO,
            circuit_breaker_active: false,
            open_positions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenPosition {
    pub asset_class: AssetClass,
    pub ticker: String,
    pub correlation_group: CorrelationGroup,
    pub side: Action,
    pub notional: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircuitBreakerState {
    pub active: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatedOrderPlan {
    pub approved: bool,
    pub action: Action,
    pub reject_reasons: Vec<String>,
    pub risk_reward_ratio: Option<Decimal>,
    pub position_size: Option<Decimal>,
    pub entry_price: Option<Decimal>,
    pub stop_loss: Option<Decimal>,
    pub take_profit: Option<Decimal>,
    pub entry_drift_pct: Option<Decimal>,
}

impl ValidatedOrderPlan {
    pub fn rejected(action: Action, reasons: Vec<String>) -> Self {
        Self {
            approved: false,
            action,
            reject_reasons: reasons,
            risk_reward_ratio: None,
            position_size: None,
            entry_price: None,
            stop_loss: None,
            take_profit: None,
            entry_drift_pct: None,
        }
    }
}

/// LLM-proposed signal before deterministic guardrail validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedSignal {
    pub asset_class: AssetClass,
    pub ticker: String,
    pub correlation_group: CorrelationGroup,
    pub timeframe: Timeframe,
    pub action: Action,
    pub confidence: f64,
    pub entry_price: Decimal,
    pub stop_loss: Decimal,
    pub take_profit: Decimal,
    pub reasoning_summary: String,
    pub llm_provider_used: Option<String>,
}
