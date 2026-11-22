#![allow(dead_code)]

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::models::features::{EventRisk, MarketRegime};
use crate::models::market::AssetClass;
use crate::models::signal::{Action, PortfolioRiskState, ProposedSignal, ValidatedOrderPlan};

pub const MIN_RR_RATIO: Decimal = dec!(1.5);
pub const MAX_CORRELATED_POSITIONS_PER_GROUP: usize = 2;
pub const MAX_SINGLE_POSITION_NOTIONAL_PCT: Decimal = dec!(20); // percent of equity

#[derive(Debug, Clone)]
pub struct GuardrailInput {
    pub signal: ProposedSignal,
    pub live_price: Decimal,
    pub atr_14: Decimal,
    pub portfolio: PortfolioRiskState,
    pub event_risk: EventRisk,
    pub market_regime: MarketRegime,
    pub equity: Decimal,
    /// Base position size in units (already risk-adjusted upstream if desired).
    pub base_position_size: Decimal,
    pub daily_drawdown_limit_pct: Decimal,
    pub weekly_drawdown_limit_pct: Decimal,
}

pub fn evaluate(input: &GuardrailInput) -> ValidatedOrderPlan {
    let action = input.signal.action;
    if matches!(action, Action::Hold) {
        return ValidatedOrderPlan {
            approved: true,
            action,
            reject_reasons: Vec::new(),
            risk_reward_ratio: None,
            position_size: None,
            entry_price: Some(input.signal.entry_price),
            stop_loss: Some(input.signal.stop_loss),
            take_profit: Some(input.signal.take_profit),
            entry_drift_pct: Some(drift_pct(input.live_price, input.signal.entry_price)),
        };
    }

    let mut reasons = Vec::new();

    // Lapis 1: Drawdown Circuit Breaker
    if input.portfolio.circuit_breaker_active
        || input.portfolio.daily_pnl_pct <= -abs(input.daily_drawdown_limit_pct)
        || input.portfolio.weekly_pnl_pct <= -abs(input.weekly_drawdown_limit_pct)
    {
        reasons.push("layer1_circuit_breaker: daily/weekly drawdown limit breached".to_owned());
    }

    // Lapis 2: Event Risk Hard-Block
    if input.event_risk.blocked_entry {
        reasons.push("layer2_event_risk: blocked_entry=true".to_owned());
    }

    // Lapis 3: Entry Staleness / Slippage Guard
    let drift = abs(input.live_price - input.signal.entry_price);
    let max_drift = input.atr_14 * dec!(0.5);
    let drift_pct_val = drift_pct(input.live_price, input.signal.entry_price);
    if input.atr_14 <= Decimal::ZERO || drift > max_drift {
        reasons.push(format!(
            "layer3_staleness: |live-entry|={} > 0.5*ATR={}",
            drift, max_drift
        ));
    }

    // Lapis 4-5: SL/TP direction, RR, ATR distance bounds
    let rr = validate_direction_and_rr(&input.signal, input.atr_14, &mut reasons);

    // Lapis 6: Portfolio Correlation & Concentration Limit
    validate_concentration(input, &mut reasons);

    // Lapis 7: Regime-Adjusted Position Sizing
    let mut position_size = input.base_position_size;
    if input.market_regime.volatility_regime == "extreme" {
        position_size *= dec!(0.5);
    }
    if matches!(input.signal.asset_class, AssetClass::IdxStock) {
        position_size = round_down_lot(position_size, Decimal::from(100u64));
    }
    if position_size <= Decimal::ZERO {
        reasons.push("layer7_position_size: computed position size <= 0".to_owned());
    }

    let approved = reasons.is_empty();
    ValidatedOrderPlan {
        approved,
        action,
        reject_reasons: reasons,
        risk_reward_ratio: rr,
        position_size: if approved { Some(position_size) } else { None },
        entry_price: Some(input.signal.entry_price),
        stop_loss: Some(input.signal.stop_loss),
        take_profit: Some(input.signal.take_profit),
        entry_drift_pct: Some(drift_pct_val),
    }
}

fn validate_direction_and_rr(
    signal: &ProposedSignal,
    atr: Decimal,
    reasons: &mut Vec<String>,
) -> Option<Decimal> {
    let entry = signal.entry_price;
    let sl = signal.stop_loss;
    let tp = signal.take_profit;

    if entry <= Decimal::ZERO || sl <= Decimal::ZERO || tp <= Decimal::ZERO {
        reasons.push("layer4_invalid_prices: entry/sl/tp must be > 0".to_owned());
        return None;
    }

    let (risk, reward, dir_ok) = match signal.action {
        Action::Buy => (entry - sl, tp - entry, sl < entry && entry < tp),
        Action::Sell => (sl - entry, entry - tp, tp < entry && entry < sl),
        Action::Hold => (Decimal::ZERO, Decimal::ZERO, true),
    };

    if !dir_ok {
        reasons.push("layer4_direction: SL/TP direction invalid for action".to_owned());
        return None;
    }

    if risk <= Decimal::ZERO {
        reasons.push("layer4_risk: risk must be > 0".to_owned());
        return None;
    }

    // SL distance bounds: 0.5x ATR to 4.0x ATR
    if atr <= Decimal::ZERO {
        reasons.push("layer5_atr: ATR must be > 0".to_owned());
    } else {
        let min_sl = atr * dec!(0.5);
        let max_sl = atr * dec!(4.0);
        if risk < min_sl || risk > max_sl {
            reasons.push(format!(
                "layer5_sl_distance: risk {} outside {}..{} (0.5x-4.0x ATR)",
                risk, min_sl, max_sl
            ));
        }
    }

    let rr = reward / risk;
    if rr < MIN_RR_RATIO {
        reasons.push(format!(
            "layer5_rr: risk_reward_ratio {} < {}",
            rr, MIN_RR_RATIO
        ));
    }
    Some(rr)
}

fn validate_concentration(input: &GuardrailInput, reasons: &mut Vec<String>) {
    let group = &input.signal.correlation_group;
    let same_group = input
        .portfolio
        .open_positions
        .iter()
        .filter(|p| &p.correlation_group == group)
        .count();
    if same_group >= MAX_CORRELATED_POSITIONS_PER_GROUP {
        reasons.push(format!(
            "layer6_correlation: {} open positions already in group {} (max {})",
            same_group,
            group.as_str(),
            MAX_CORRELATED_POSITIONS_PER_GROUP
        ));
    }

    if input.equity > Decimal::ZERO {
        let notional = input.signal.entry_price * input.base_position_size;
        let pct = (notional / input.equity) * dec!(100);
        if pct > MAX_SINGLE_POSITION_NOTIONAL_PCT {
            reasons.push(format!(
                "layer6_concentration: position notional {}% > {}%",
                pct, MAX_SINGLE_POSITION_NOTIONAL_PCT
            ));
        }
    }
}

fn drift_pct(live: Decimal, entry: Decimal) -> Decimal {
    if entry.is_zero() {
        return Decimal::ZERO;
    }
    ((live - entry) / entry) * dec!(100)
}

fn abs(d: Decimal) -> Decimal {
    if d < Decimal::ZERO {
        -d
    } else {
        d
    }
}

fn round_down_lot(size: Decimal, lot: Decimal) -> Decimal {
    if lot <= Decimal::ZERO {
        return size;
    }
    (size / lot).floor() * lot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::features::{EventRisk, MarketRegime};
    use crate::models::market::{AssetClass, CorrelationGroup, Timeframe};
    use crate::models::signal::{OpenPosition, PortfolioRiskState, ProposedSignal};

    fn base_signal() -> ProposedSignal {
        ProposedSignal {
            asset_class: AssetClass::Crypto,
            ticker: "BTCUSDT".to_owned(),
            correlation_group: CorrelationGroup::from("btc_beta"),
            timeframe: Timeframe::M15,
            action: Action::Buy,
            confidence: 0.8,
            entry_price: dec!(100),
            stop_loss: dec!(98),
            take_profit: dec!(104),
            reasoning_summary: "test".to_owned(),
            llm_provider_used: Some("opencode_zen".to_owned()),
        }
    }

    fn base_input() -> GuardrailInput {
        GuardrailInput {
            signal: base_signal(),
            live_price: dec!(100),
            atr_14: dec!(2),
            portfolio: PortfolioRiskState::default(),
            event_risk: EventRisk::default(),
            market_regime: MarketRegime {
                asset_class: AssetClass::Crypto,
                regime_label: "risk_on".to_owned(),
                volatility_regime: "normal".to_owned(),
                bias: "bullish".to_owned(),
                summary: "ok".to_owned(),
                details: serde_json::json!({}),
            },
            equity: dec!(10000),
            base_position_size: dec!(10),
            daily_drawdown_limit_pct: dec!(2),
            weekly_drawdown_limit_pct: dec!(5),
        }
    }

    #[test]
    fn layer1_circuit_breaker_rejects() {
        let mut input = base_input();
        input.portfolio.daily_pnl_pct = dec!(-2);
        let out = evaluate(&input);
        assert!(!out.approved);
        assert!(out
            .reject_reasons
            .iter()
            .any(|r| r.contains("layer1_circuit_breaker")));
    }

    #[test]
    fn layer2_event_risk_blocks() {
        let mut input = base_input();
        input.event_risk.blocked_entry = true;
        let out = evaluate(&input);
        assert!(!out.approved);
        assert!(out
            .reject_reasons
            .iter()
            .any(|r| r.contains("layer2_event_risk")));
    }

    #[test]
    fn layer3_staleness_rejects_drift() {
        let mut input = base_input();
        input.live_price = dec!(102); // drift 2, atr 2 => max 1
        let out = evaluate(&input);
        assert!(!out.approved);
        assert!(out
            .reject_reasons
            .iter()
            .any(|r| r.contains("layer3_staleness")));
    }

    #[test]
    fn layer4_rejects_bad_direction() {
        let mut input = base_input();
        input.signal.stop_loss = dec!(101); // above entry for BUY
        let out = evaluate(&input);
        assert!(!out.approved);
        assert!(out
            .reject_reasons
            .iter()
            .any(|r| r.contains("layer4_direction")));
    }

    #[test]
    fn layer5_rejects_low_rr_and_bad_sl_distance() {
        let mut input = base_input();
        input.signal.take_profit = dec!(101); // RR 0.5
        let out = evaluate(&input);
        assert!(!out.approved);
        assert!(out.reject_reasons.iter().any(|r| r.contains("layer5_rr")));

        let mut input2 = base_input();
        input2.atr_14 = dec!(10); // min SL distance 5, but risk is 2
        let out2 = evaluate(&input2);
        assert!(!out2.approved);
        assert!(out2
            .reject_reasons
            .iter()
            .any(|r| r.contains("layer5_sl_distance")));
    }

    #[test]
    fn layer6_rejects_correlation_and_concentration() {
        let mut input = base_input();
        input.portfolio.open_positions = vec![
            OpenPosition {
                asset_class: AssetClass::Crypto,
                ticker: "ETHUSDT".into(),
                correlation_group: CorrelationGroup::from("btc_beta"),
                side: Action::Buy,
                notional: dec!(1000),
            },
            OpenPosition {
                asset_class: AssetClass::Crypto,
                ticker: "SOLUSDT".into(),
                correlation_group: CorrelationGroup::from("btc_beta"),
                side: Action::Buy,
                notional: dec!(1000),
            },
        ];
        let out = evaluate(&input);
        assert!(!out.approved);
        assert!(out
            .reject_reasons
            .iter()
            .any(|r| r.contains("layer6_correlation")));

        let mut input2 = base_input();
        input2.base_position_size = dec!(30); // notional 3000 = 30% of 10000
        let out2 = evaluate(&input2);
        assert!(!out2.approved);
        assert!(out2
            .reject_reasons
            .iter()
            .any(|r| r.contains("layer6_concentration")));
    }

    #[test]
    fn layer7_adjusts_size_for_extreme_and_idx_lot() {
        let mut input = base_input();
        input.market_regime.volatility_regime = "extreme".to_owned();
        input.base_position_size = dec!(10);
        let out = evaluate(&input);
        assert!(out.approved);
        assert_eq!(out.position_size, Some(dec!(5)));

        let mut input2 = base_input();
        input2.signal.asset_class = AssetClass::IdxStock;
        input2.signal.ticker = "BBCA.JK".to_owned();
        input2.market_regime.asset_class = AssetClass::IdxStock;
        input2.equity = dec!(1_000_000);
        input2.base_position_size = dec!(550);
        let out2 = evaluate(&input2);
        assert!(out2.approved);
        assert_eq!(out2.position_size, Some(dec!(500))); // rounded down to lot 100
    }

    #[test]
    fn hold_is_approved_without_rr() {
        let mut input = base_input();
        input.signal.action = Action::Hold;
        let out = evaluate(&input);
        assert!(out.approved);
        assert!(out.risk_reward_ratio.is_none());
    }
}
