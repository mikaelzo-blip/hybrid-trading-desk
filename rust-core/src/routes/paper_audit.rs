use std::collections::{BTreeMap, BTreeSet};

use axum::{extract::State, http::StatusCode, Json};
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use serde_json::{json, Value};

use crate::routes::AppState;

const WINDOW_DAYS: i64 = 30;

#[derive(sqlx::FromRow)]
struct SignalRow {
    entry_drift_pct: Option<Decimal>,
    guardrail_approved: bool,
    reject_reason: Option<String>,
}

#[derive(sqlx::FromRow)]
struct ClosedOrderRow {
    side: String,
    entry_price: Decimal,
    filled_price: Option<Decimal>,
    stop_loss: Option<Decimal>,
    position_size: Decimal,
    realized_pnl: Option<Decimal>,
    confidence: Option<f64>,
}

pub async fn get_paper_audit(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let end = Utc::now();
    let start = end - Duration::days(WINDOW_DAYS);
    let signals = sqlx::query_as::<_, SignalRow>(
        "SELECT entry_drift_pct, guardrail_approved, reject_reason
         FROM agent_signals
         WHERE action IN ('BUY', 'SELL') AND candle_timestamp BETWEEN $1 AND $2",
    )
    .bind(start)
    .bind(end)
    .fetch_all(&state.db);
    let orders = sqlx::query_as::<_, ClosedOrderRow>(
        "SELECT p.side, p.entry_price, p.filled_price, p.stop_loss, p.position_size,
                p.realized_pnl, COALESCE(s.confidence, pick.confidence) AS confidence
         FROM paper_orders p
         LEFT JOIN agent_signals s ON s.id = p.signal_id
         LEFT JOIN scout_picks pick ON pick.id = p.scout_pick_id
         WHERE p.status = 'closed' AND p.closed_at BETWEEN $1 AND $2",
    )
    .bind(start)
    .bind(end)
    .fetch_all(&state.db);
    let (signals, orders) = tokio::try_join!(signals, orders).map_err(|err| {
        tracing::error!(error = %err, "paper audit query failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "paper_audit_unavailable"})),
        )
    })?;
    let mut audit = build_audit(&signals, &orders);
    audit["window"] = json!({ "days": WINDOW_DAYS, "start": start, "end": end });
    Ok(Json(audit))
}

fn build_audit(signals: &[SignalRow], orders: &[ClosedOrderRow]) -> Value {
    let mut rejections = BTreeMap::<&str, usize>::new();
    let mut rejected_count = 0;
    let mut drift_count = 0;
    let mut drift_abs_sum = Decimal::ZERO;
    for signal in signals {
        if let Some(drift) = signal.entry_drift_pct {
            drift_count += 1;
            drift_abs_sum += drift.abs();
        }
        if !signal.guardrail_approved {
            rejected_count += 1;
            let reasons: BTreeSet<_> = signal
                .reject_reason
                .as_deref()
                .unwrap_or_default()
                .split("; ")
                .map(|reason| {
                    reason
                        .split_once(':')
                        .map(|(code, _)| code)
                        .filter(|code| {
                            code.starts_with("layer")
                                && code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                        })
                        .unwrap_or("other")
                })
                .collect();
            for reason in reasons {
                *rejections.entry(reason).or_default() += 1;
            }
        }
    }

    let mut realized_count = 0;
    let mut calibration = [(0, 0); 5];
    let mut r_buckets = [0usize; 5];
    let mut r_count = 0;
    let mut r_sum = Some(Decimal::ZERO);

    for order in orders {
        let Some(pnl) = order.realized_pnl else {
            continue;
        };
        realized_count += 1;
        if let Some(confidence) = order.confidence.filter(|c| (0.0..=1.0).contains(c)) {
            let bin = [0.2, 0.4, 0.6, 0.8].partition_point(|b| confidence >= *b);
            calibration[bin].0 += 1;
            calibration[bin].1 += usize::from(pnl > Decimal::ZERO);
        }
        if let Some(r) = realized_r(order, pnl) {
            let bin = if r < Decimal::ZERO {
                0
            } else if r.is_zero() {
                1
            } else if r < Decimal::ONE {
                2
            } else if r < Decimal::TWO {
                3
            } else {
                4
            };
            r_count += 1;
            r_buckets[bin] += 1;
            r_sum = r_sum.and_then(|sum| sum.checked_add(r));
        }
    }
    let calibration: Vec<_> = calibration
        .into_iter()
        .enumerate()
        .filter(|(_, (count, _))| *count != 0)
        .map(|(bin, (count, wins))| {
            json!({
                "min_confidence_pct": bin * 20,
                "max_confidence_pct": (bin + 1) * 20,
                "sample_count": count,
                "win_rate_pct": mean(Decimal::from(wins) * Decimal::from(100), count),
            })
        })
        .collect();
    let r_multiple = (r_count != 0).then(|| {
        json!({
            "sample_count": r_count,
            "mean": r_sum.and_then(|sum| mean(sum, r_count)),
            "distribution": {
                "negative": r_buckets[0], "flat": r_buckets[1], "under_one": r_buckets[2],
                "one_to_two": r_buckets[3], "two_plus": r_buckets[4],
            },
        })
    });
    json!({
        "entry_drift": {
            "sample_count": drift_count,
            "mean_absolute_pct": mean(drift_abs_sum, drift_count),
        },
        "guardrail": {
            "directional_signals": signals.len(),
            "rejected_signals": rejected_count,
            "rejection_reasons": rejections,
        },
        "closed_orders": {
            "sample_count": orders.len(),
            "realized_sample_count": realized_count,
            "r_multiple": r_multiple,
            "confidence_calibration": (!calibration.is_empty()).then_some(calibration),
        },
    })
}

fn mean(sum: Decimal, count: usize) -> Option<String> {
    sum.checked_div(Decimal::from(count))
        .map(|value| value.round_dp(8).normalize().to_string())
}

fn realized_r(order: &ClosedOrderRow, pnl: Decimal) -> Option<Decimal> {
    let entry = order.filled_price.unwrap_or(order.entry_price);
    let stop = order.stop_loss?;
    if entry <= Decimal::ZERO || stop <= Decimal::ZERO || order.position_size <= Decimal::ZERO {
        return None;
    }
    let distance = match order.side.as_str() {
        "BUY" => entry - stop,
        "SELL" => stop - entry,
        _ => return None,
    };
    if distance <= Decimal::ZERO {
        return None;
    }
    pnl.checked_div(distance.checked_mul(order.position_size)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn no_samples_is_not_zero_performance() {
        let audit = build_audit(&[], &[]);
        assert_eq!(audit["entry_drift"]["sample_count"], 0);
        assert_eq!(audit["entry_drift"]["mean_absolute_pct"], Value::Null);
        assert_eq!(audit["closed_orders"]["sample_count"], 0);
        assert_eq!(audit["closed_orders"]["r_multiple"], Value::Null);
        assert_eq!(
            audit["closed_orders"]["confidence_calibration"],
            Value::Null
        );
    }

    #[test]
    fn counts_reasons_once_per_signal_and_keeps_drift_absolute() {
        let signals = [
            SignalRow { entry_drift_pct: Some(dec!(-1)), guardrail_approved: true, reject_reason: None },
            SignalRow {
                entry_drift_pct: Some(dec!(3)),
                guardrail_approved: false,
                reject_reason: Some("layer2_event_risk: blocked; layer2_event_risk: repeated; layer3_staleness: drift".into()),
            },
        ];
        let audit = build_audit(&signals, &[]);
        assert_eq!(audit["entry_drift"]["sample_count"], 2);
        assert_eq!(audit["entry_drift"]["mean_absolute_pct"], "2");
        assert_eq!(audit["guardrail"]["rejected_signals"], 1);
        assert_eq!(
            audit["guardrail"]["rejection_reasons"],
            json!({
                "layer2_event_risk": 1, "layer3_staleness": 1
            })
        );
    }

    #[test]
    fn uses_realized_pnl_and_actual_fill_for_r_and_calibration() {
        let orders = [
            ClosedOrderRow {
                side: "BUY".into(),
                entry_price: dec!(100),
                filled_price: Some(dec!(101)),
                stop_loss: Some(dec!(99)),
                position_size: dec!(5),
                realized_pnl: Some(dec!(15)),
                confidence: Some(0.85),
            },
            ClosedOrderRow {
                side: "SELL".into(),
                entry_price: dec!(100),
                filled_price: None,
                stop_loss: Some(dec!(101)),
                position_size: dec!(5),
                realized_pnl: Some(dec!(-5)),
                confidence: Some(0.84),
            },
            ClosedOrderRow {
                side: "BUY".into(),
                entry_price: dec!(100),
                filled_price: None,
                stop_loss: Some(dec!(99)),
                position_size: dec!(5),
                realized_pnl: None,
                confidence: Some(0.9),
            },
        ];
        let audit = build_audit(&[], &orders);
        assert_eq!(audit["closed_orders"]["sample_count"], 3);
        assert_eq!(audit["closed_orders"]["realized_sample_count"], 2);
        assert_eq!(audit["closed_orders"]["r_multiple"]["mean"], "0.25");
        assert_eq!(
            audit["closed_orders"]["r_multiple"]["distribution"]["negative"],
            1
        );
        assert_eq!(
            audit["closed_orders"]["r_multiple"]["distribution"]["one_to_two"],
            1
        );
        assert_eq!(
            audit["closed_orders"]["confidence_calibration"][0]["sample_count"],
            2
        );
        assert_eq!(
            audit["closed_orders"]["confidence_calibration"][0]["win_rate_pct"],
            "50"
        );
    }
}
