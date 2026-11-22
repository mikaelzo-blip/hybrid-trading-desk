use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use chrono::Utc;
use futures_util::StreamExt;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::engine::guardrail::{self, GuardrailInput};
use crate::engine::indicators::{compute_exec_features, compute_higher_tf_context};
use crate::engine::regime::RegimeEngine;
use crate::models::features::{higher_timeframe_for, EventRisk};
use crate::models::market::{AssetClass, Candle, CorrelationGroup, Timeframe};
use crate::models::signal::{Action, PortfolioRiskState, ProposedSignal};
use crate::routes::AppState;

#[derive(Debug, Deserialize)]
pub struct AnalyzeRequest {
    pub asset_class: String,
    pub ticker: String,
    pub timeframe: String,
    #[serde(default)]
    pub enable_vision: bool,
}

pub async fn analyze(
    State(state): State<AppState>,
    Json(req): Json<AnalyzeRequest>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);
    tokio::spawn(async move {
        if let Err(err) = run_analyze(state, req, tx.clone()).await {
            let _ = tx
                .send(Ok(Event::default()
                    .event("error")
                    .data(json!({ "error": err.to_string() }).to_string())))
                .await;
        }
    });
    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

async fn run_analyze(
    state: AppState,
    req: AnalyzeRequest,
    tx: mpsc::Sender<Result<Event, Infallible>>,
) -> anyhow::Result<()> {
    let asset_class: AssetClass = req
        .asset_class
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;
    let timeframe: Timeframe = req
        .timeframe
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;
    let ticker = req.ticker.trim().to_uppercase();

    // 1) Load candles for exec TF and higher TF
    let exec_candles = load_candles(&state, asset_class, &ticker, timeframe, 200).await?;
    if exec_candles.is_empty() {
        anyhow::bail!("no candles available for {ticker} {timeframe}");
    }
    let htf = higher_timeframe_for(timeframe);
    let htf_candles = load_candles(&state, asset_class, &ticker, htf, 200).await?;

    let exec_features = compute_exec_features(asset_class, &ticker, timeframe, &exec_candles)
        .ok_or_else(|| anyhow::anyhow!("failed to compute exec features"))?;
    let higher_tf = compute_higher_tf_context(htf, &htf_candles);

    // 2) Market regime + event risk
    let regime_engine = RegimeEngine::with_default()?;
    let regime = regime_engine
        .get_regime(
            asset_class,
            &state.binance,
            &state.alpaca,
            &state.idx,
            &state.redis,
        )
        .await
        .unwrap_or_else(|_| crate::models::features::MarketRegime {
            asset_class,
            regime_label: "unknown".into(),
            volatility_regime: "unknown".into(),
            bias: "neutral".into(),
            summary: "regime unavailable".into(),
            details: json!({}),
        });
    let event_risk = compute_event_risk(asset_class, &ticker);

    // 3) Proxy SSE from ai-worker
    let ai_payload = json!({
        "asset_class": asset_class.as_str(),
        "ticker": ticker,
        "timeframe": timeframe.as_str(),
        "higher_tf_context": higher_tf,
        "exec_tf_features": exec_features,
        "market_regime": regime,
        "event_risk": event_risk,
        "enable_vision": req.enable_vision,
    });

    let ai_url = format!(
        "{}/analyze/stream",
        state.config.ai_worker_base_url.trim_end_matches('/')
    );
    let client = reqwest::Client::new();
    let resp = client.post(&ai_url).json(&ai_payload).send().await?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("ai-worker error: {body}");
    }

    let mut final_signal: Option<Value> = None;
    let mut llm_provider_used: Option<String> = None;
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find("\n\n") {
            let block = buf[..pos].to_string();
            buf = buf[pos + 2..].to_string();
            if let Some((event, data)) = parse_sse_block(&block) {
                if event == "final" {
                    final_signal = Some(data.clone());
                    if let Some(p) = data.get("provider").and_then(|v| v.as_str()) {
                        llm_provider_used = Some(p.to_string());
                    }
                }
                let _ = tx
                    .send(Ok(Event::default().event(event).data(data.to_string())))
                    .await;
            }
        }
    }

    // 4) Guardrail + persist + emit guardrail result
    let Some(final_val) = final_signal else {
        anyhow::bail!("ai-worker did not emit final signal");
    };
    let signal_val = final_val
        .get("signal")
        .cloned()
        .unwrap_or(final_val.clone());
    let proposed = parse_proposed_signal(
        asset_class,
        &ticker,
        timeframe,
        &signal_val,
        llm_provider_used.clone(),
    );

    let live_price = fetch_live_price(&state, &ticker)
        .await
        .unwrap_or(exec_features.close);
    let atr = exec_features.atr_14.unwrap_or(Decimal::ONE);
    let portfolio = load_portfolio_state(&state, asset_class)
        .await
        .unwrap_or_default();
    let guardrail_input = GuardrailInput {
        signal: proposed.clone(),
        live_price,
        atr_14: atr,
        portfolio,
        event_risk: event_risk.clone(),
        market_regime: regime.clone(),
        equity: Decimal::from(10_000u64), // default paper equity baseline
        base_position_size: Decimal::from(1u64),
        daily_drawdown_limit_pct: Decimal::from(2u64),
        weekly_drawdown_limit_pct: Decimal::from(5u64),
    };
    let validated = guardrail::evaluate(&guardrail_input);

    let signal_id = Uuid::new_v4();
    persist_signal(
        &state,
        signal_id,
        asset_class,
        &ticker,
        timeframe,
        &proposed,
        live_price,
        &validated,
        &regime,
        &event_risk,
        &final_val,
    )
    .await?;

    let _ = tx
        .send(Ok(Event::default().event("guardrail").data(
            json!({
                "signal_id": signal_id,
                "approved": validated.approved,
                "reject_reasons": validated.reject_reasons,
                "risk_reward_ratio": validated.risk_reward_ratio.map(|d| d.to_string()),
                "position_size": validated.position_size.map(|d| d.to_string()),
                "live_price": live_price.to_string(),
            })
            .to_string(),
        )))
        .await;

    Ok(())
}

async fn load_candles(
    state: &AppState,
    asset_class: AssetClass,
    ticker: &str,
    timeframe: Timeframe,
    limit: usize,
) -> anyhow::Result<Vec<Candle>> {
    crate::routes::candles::fetch_candles_db(
        state,
        asset_class,
        ticker,
        timeframe,
        None,
        None,
        limit,
    )
    .await
}

async fn fetch_live_price(state: &AppState, ticker: &str) -> anyhow::Result<Decimal> {
    let mut conn = state.redis.get_multiplexed_async_connection().await?;
    let key = crate::adapters::binance::live_price_key(ticker);
    let val: Option<String> = redis::AsyncCommands::get(&mut conn, &key).await?;
    let Some(v) = val else {
        anyhow::bail!("no live price")
    };
    Ok(v.parse()?)
}

fn compute_event_risk(asset_class: AssetClass, ticker: &str) -> EventRisk {
    // Baseline: use deterministic engine; external calendar feeds can enrich EventRiskInput later.
    let input = crate::engine::event_risk::EventRiskInput::default();
    crate::engine::event_risk::evaluate(asset_class, ticker, &input)
}

fn parse_sse_block(block: &str) -> Option<(String, Value)> {
    let mut event = "message".to_string();
    let mut data_lines: Vec<String> = Vec::new();
    for line in block.lines() {
        if let Some(v) = line.strip_prefix("event:") {
            event = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("data:") {
            data_lines.push(v.trim_start().to_string());
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let data_str = data_lines.join("\n");
    let data: Value =
        serde_json::from_str(&data_str).unwrap_or_else(|_| json!({ "raw": data_str }));
    Some((event, data))
}

fn parse_proposed_signal(
    asset_class: AssetClass,
    ticker: &str,
    timeframe: Timeframe,
    signal: &Value,
    llm_provider_used: Option<String>,
) -> ProposedSignal {
    let action = match signal
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("HOLD")
    {
        "BUY" => Action::Buy,
        "SELL" => Action::Sell,
        _ => Action::Hold,
    };
    let entry = parse_decimal_val(signal.get("entry_price"));
    let sl = parse_decimal_val(signal.get("stop_loss"));
    let tp = parse_decimal_val(signal.get("take_profit"));
    ProposedSignal {
        asset_class,
        ticker: ticker.to_string(),
        correlation_group: CorrelationGroup::general(),
        timeframe,
        action,
        confidence: signal
            .get("confidence")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        entry_price: entry,
        stop_loss: sl,
        take_profit: tp,
        reasoning_summary: signal
            .get("reasoning_summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        llm_provider_used,
    }
}

fn parse_decimal_val(v: Option<&Value>) -> Decimal {
    match v {
        Some(Value::String(s)) => s.parse().unwrap_or(Decimal::ZERO),
        Some(Value::Number(n)) => n.to_string().parse().unwrap_or(Decimal::ZERO),
        _ => Decimal::ZERO,
    }
}

async fn load_portfolio_state(
    state: &AppState,
    _asset_class: AssetClass,
) -> anyhow::Result<PortfolioRiskState> {
    // Baseline: read latest global portfolio_risk_state if present
    let row = sqlx::query_as::<_, (Decimal, Decimal, bool)>(
        "SELECT daily_pnl_pct, weekly_pnl_pct, circuit_breaker_active \
         FROM portfolio_risk_state WHERE scope='global' ORDER BY updated_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    Ok(row
        .map(|(d, w, cb)| PortfolioRiskState {
            daily_pnl_pct: d,
            weekly_pnl_pct: w,
            circuit_breaker_active: cb,
            open_positions: Vec::new(),
        })
        .unwrap_or_default())
}

#[allow(clippy::too_many_arguments)]
async fn persist_signal(
    state: &AppState,
    signal_id: Uuid,
    asset_class: AssetClass,
    ticker: &str,
    timeframe: Timeframe,
    proposed: &ProposedSignal,
    live_price: Decimal,
    validated: &crate::models::signal::ValidatedOrderPlan,
    regime: &crate::models::features::MarketRegime,
    event_risk: &EventRisk,
    raw_final: &Value,
) -> anyhow::Result<()> {
    let drift = if proposed.entry_price.is_zero() {
        Decimal::ZERO
    } else {
        ((live_price - proposed.entry_price) / proposed.entry_price) * Decimal::from(100u64)
    };
    sqlx::query(
        r#"INSERT INTO agent_signals (
            id, asset_class, ticker, correlation_group, timeframe, candle_timestamp,
            action, confidence, entry_price, live_price_at_decision, entry_drift_pct,
            stop_loss, take_profit, guardrail_approved, reject_reason, risk_reward_ratio,
            position_size, llm_provider_used, market_regime_snapshot, event_risk_snapshot,
            reasoning_summary, debate_log
        ) VALUES (
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22
        )"#,
    )
    .bind(signal_id)
    .bind(asset_class.as_str())
    .bind(ticker)
    .bind(proposed.correlation_group.as_str())
    .bind(timeframe.as_str())
    .bind(Utc::now())
    .bind(proposed.action.as_str())
    .bind(proposed.confidence)
    .bind(proposed.entry_price)
    .bind(live_price)
    .bind(drift)
    .bind(proposed.stop_loss)
    .bind(proposed.take_profit)
    .bind(validated.approved)
    .bind(if validated.reject_reasons.is_empty() {
        None
    } else {
        Some(validated.reject_reasons.join("; "))
    })
    .bind(validated.risk_reward_ratio)
    .bind(validated.position_size)
    .bind(proposed.llm_provider_used.clone())
    .bind(serde_json::to_value(regime).ok())
    .bind(serde_json::to_value(event_risk).ok())
    .bind(&proposed.reasoning_summary)
    .bind(raw_final.clone())
    .execute(&state.db)
    .await?;
    Ok(())
}
