use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::engine::guardrail::{self, GuardrailInput};
use crate::engine::indicators::{compute_exec_features, compute_higher_tf_context};
use crate::engine::regime::RegimeEngine;
use crate::models::features::{higher_timeframe_for, MarketRegime};
use crate::models::market::{AssetClass, CorrelationGroup, Timeframe};
use crate::models::signal::{Action, PortfolioRiskState, ProposedSignal};
use crate::routes::AppState;
use crate::scout::screener;

#[derive(Debug, Deserialize)]
pub struct ScoutRunRequest {
    pub asset_class: String,
}

#[derive(Debug, Deserialize)]
pub struct ScoutPicksQuery {
    pub asset_class: Option<String>,
    pub limit: Option<i64>,
}

pub async fn run_scout(
    State(state): State<AppState>,
    Json(req): Json<ScoutRunRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let asset_class: AssetClass = req.asset_class.parse().map_err(bad_request)?;
    let run_id = Uuid::new_v4();
    let max_deep = std::env::var("SCOUT_MAX_DEEP_DIVE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3usize);
    let free_tier = std::env::var("FREE_TIER_MODE")
        .ok()
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);
    let deep_limit = if free_tier { max_deep.min(5) } else { max_deep };

    let candidates = screener::screen_top_candidates(&state.db, asset_class, 15)
        .await
        .map_err(internal_error)?;

    let triage_body = json!({
        "asset_class": asset_class.as_str(),
        "strategy_tag": "momentum_breakout",
        "timestamp_wib": Utc::now().to_rfc3339(),
        "max_select": deep_limit,
        "market_regime": {"asset_class": asset_class.as_str(), "regime_label":"mixed","volatility_regime":"normal","bias":"neutral","summary":"scout baseline","details":{}},
        "candidates": candidates.iter().map(|c| json!({
            "ticker": c.ticker,
            "asset_class": c.asset_class.as_str(),
            "strategy_tag": "momentum_breakout",
            "metrics": {"ret_pct": c.ret_pct, "avg_volume": c.avg_volume, "atr_pct": c.atr_pct, "last_close": c.last_close.to_string()},
            "headlines": []
        })).collect::<Vec<_>>()
    });
    let client = reqwest::Client::new();
    let triage_url = format!(
        "{}/scout/triage",
        state.config.ai_worker_base_url.trim_end_matches('/')
    );
    let triage_resp = client
        .post(triage_url)
        .json(&triage_body)
        .send()
        .await
        .map_err(internal_error)?;
    let triage_json: Value = triage_resp.json().await.map_err(internal_error)?;
    let selected = triage_json
        .get("selected_candidates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut saved = Vec::new();
    for (idx, sel) in selected.into_iter().take(deep_limit).enumerate() {
        let Some(ticker) = sel.get("ticker").and_then(|v| v.as_str()) else {
            continue;
        };
        let triage_rank = sel
            .get("triage_rank")
            .and_then(|v| v.as_i64())
            .unwrap_or((idx as i64) + 1);
        let priority_score = sel
            .get("priority_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let triage_notes = sel
            .get("triage_notes")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let candles = crate::routes::candles::fetch_candles_db(
            &state,
            asset_class,
            ticker,
            Timeframe::H1,
            None,
            None,
            240,
        )
        .await
        .map_err(internal_error)?;
        if candles.is_empty() {
            continue;
        }

        let htf = higher_timeframe_for(Timeframe::H1);
        let htf_candles = crate::routes::candles::fetch_candles_db(
            &state,
            asset_class,
            ticker,
            htf,
            None,
            None,
            240,
        )
        .await
        .unwrap_or_default();
        let Some(exec_features) =
            compute_exec_features(asset_class, ticker, Timeframe::H1, &candles)
        else {
            continue;
        };
        let higher_tf = compute_higher_tf_context(htf, &htf_candles);

        let regime_engine = RegimeEngine::with_default().ok();
        let regime = if let Some(engine) = regime_engine {
            engine
                .get_regime(
                    asset_class,
                    &state.binance,
                    &state.alpaca,
                    &state.idx,
                    &state.redis,
                )
                .await
                .unwrap_or_else(|_| baseline_regime(asset_class))
        } else {
            baseline_regime(asset_class)
        };
        let event_risk = crate::engine::event_risk::evaluate(
            asset_class,
            ticker,
            &crate::engine::event_risk::EventRiskInput::default(),
        );

        let analyze_body = json!({
            "asset_class": asset_class.as_str(),
            "ticker": ticker,
            "timeframe": "1h",
            "higher_tf_context": higher_tf,
            "exec_tf_features": exec_features,
            "market_regime": regime,
            "event_risk": event_risk,
            "candles": candles.iter().rev().take(120).rev().cloned().collect::<Vec<_>>(),
        });

        let analyze_url = format!(
            "{}/analyze/stream",
            state.config.ai_worker_base_url.trim_end_matches('/')
        );
        let resp = match client.post(analyze_url).json(&analyze_body).send().await {
            Ok(r) => r,
            Err(_) => continue,
        };
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut final_signal: Option<Value> = None;
        let mut llm_provider_used: Option<String> = None;
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let Ok(chunk) = chunk else {
                break;
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(sep) = buf.find("\n\n") {
                let block = buf[..sep].to_string();
                buf = buf[sep + 2..].to_string();
                let mut event_name = "message".to_string();
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        event_name = v.trim().to_string();
                    }
                    if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.trim());
                    }
                }
                if let Ok(d) = serde_json::from_str::<Value>(&data) {
                    if let Some(p) = d.get("provider").and_then(|v| v.as_str()) {
                        llm_provider_used = Some(p.to_string());
                    }
                    if event_name == "final" {
                        final_signal = Some(d.get("signal").cloned().unwrap_or(d));
                    }
                }
            }
        }

        let Some(signal_val) = final_signal else {
            continue;
        };
        let proposed = parse_proposed_signal(
            asset_class,
            ticker,
            Timeframe::H1,
            &signal_val,
            llm_provider_used,
        );
        let live_price = fetch_live_price(&state, ticker)
            .await
            .unwrap_or(exec_features.close);
        let atr = exec_features.atr_14.unwrap_or(Decimal::ONE);
        let portfolio = load_portfolio_state(&state).await.unwrap_or_default();
        let input = GuardrailInput {
            signal: proposed.clone(),
            live_price,
            atr_14: atr,
            portfolio,
            event_risk: event_risk.clone(),
            market_regime: regime.clone(),
            equity: Decimal::from(10_000u64),
            base_position_size: Decimal::ONE,
            daily_drawdown_limit_pct: Decimal::from(2u64),
            weekly_drawdown_limit_pct: Decimal::from(5u64),
        };
        let validated = guardrail::evaluate(&input);

        let id = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO scout_picks (
                id, run_id, asset_class, ticker, correlation_group, timeframe, candle_timestamp,
                strategy_tag, action, confidence, entry_price, stop_loss, take_profit,
                guardrail_approved, reject_reason, risk_reward_ratio, position_size,
                triage_rank, priority_score, llm_provider_used, market_regime_snapshot, event_risk_snapshot, reasoning_summary
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23)"#,
        )
        .bind(id)
        .bind(run_id)
        .bind(asset_class.as_str())
        .bind(ticker)
        .bind(CorrelationGroup::general().as_str())
        .bind("1h")
        .bind(candles.last().map(|c| c.open_time).unwrap_or_else(Utc::now))
        .bind("momentum_breakout")
        .bind(proposed.action.as_str())
        .bind(proposed.confidence)
        .bind(proposed.entry_price)
        .bind(proposed.stop_loss)
        .bind(proposed.take_profit)
        .bind(validated.approved)
        .bind(if validated.reject_reasons.is_empty() { None } else { Some(validated.reject_reasons.join("; ")) })
        .bind(validated.risk_reward_ratio)
        .bind(validated.position_size)
        .bind(triage_rank as i32)
        .bind(priority_score)
        .bind(proposed.llm_provider_used.clone())
        .bind(serde_json::to_value(&regime).ok())
        .bind(serde_json::to_value(&event_risk).ok())
        .bind(if triage_notes.is_empty() { proposed.reasoning_summary.clone() } else { format!("{} | {}", triage_notes, proposed.reasoning_summary) })
        .execute(&state.db)
        .await
        .map_err(internal_error)?;

        saved.push(json!({
            "id": id, "ticker": ticker, "triage_rank": triage_rank, "priority_score": priority_score,
            "action": proposed.action.as_str(), "guardrail_approved": validated.approved,
        }));
    }

    Ok(Json(json!({
        "run_id": run_id,
        "asset_class": asset_class.as_str(),
        "stage1_candidates": candidates.len(),
        "stage2_selected": saved.len(),
        "saved": saved,
    })))
}

pub async fn get_scout_picks(
    State(state): State<AppState>,
    Query(q): Query<ScoutPicksQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows = if let Some(ac) = q.asset_class {
        let ac: AssetClass = ac.parse().map_err(bad_request)?;
        sqlx::query_as::<_, ScoutPickRow>(
            r#"SELECT id, run_id, asset_class, ticker, correlation_group, timeframe, strategy_tag, action, confidence,
                      entry_price, stop_loss, take_profit, guardrail_approved, reject_reason, risk_reward_ratio, position_size,
                      triage_rank, priority_score, llm_provider_used, market_regime_snapshot, event_risk_snapshot, reasoning_summary, created_at
               FROM scout_picks WHERE asset_class=$1 ORDER BY created_at DESC LIMIT $2"#,
        ).bind(ac.as_str()).bind(limit).fetch_all(&state.db).await.map_err(internal_error)?
    } else {
        sqlx::query_as::<_, ScoutPickRow>(
            r#"SELECT id, run_id, asset_class, ticker, correlation_group, timeframe, strategy_tag, action, confidence,
                      entry_price, stop_loss, take_profit, guardrail_approved, reject_reason, risk_reward_ratio, position_size,
                      triage_rank, priority_score, llm_provider_used, market_regime_snapshot, event_risk_snapshot, reasoning_summary, created_at
               FROM scout_picks ORDER BY created_at DESC LIMIT $1"#,
        ).bind(limit).fetch_all(&state.db).await.map_err(internal_error)?
    };

    let picks = rows.into_iter().map(|r| json!({
        "id": r.id, "run_id": r.run_id, "asset_class": r.asset_class, "ticker": r.ticker,
        "correlation_group": r.correlation_group, "timeframe": r.timeframe, "strategy_tag": r.strategy_tag,
        "action": r.action, "confidence": r.confidence,
        "entry_price": r.entry_price.map(|v| v.to_string()),
        "stop_loss": r.stop_loss.map(|v| v.to_string()),
        "take_profit": r.take_profit.map(|v| v.to_string()),
        "guardrail_approved": r.guardrail_approved, "reject_reason": r.reject_reason,
        "risk_reward_ratio": r.risk_reward_ratio.map(|v| v.to_string()),
        "position_size": r.position_size.map(|v| v.to_string()),
        "triage_rank": r.triage_rank, "priority_score": r.priority_score,
        "llm_provider_used": r.llm_provider_used,
        "market_regime_snapshot": r.market_regime_snapshot,
        "event_risk_snapshot": r.event_risk_snapshot,
        "reasoning_summary": r.reasoning_summary,
        "created_at": r.created_at,
    })).collect::<Vec<_>>();

    Ok(Json(json!({"picks": picks})))
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
        entry_price: parse_decimal_val(signal.get("entry_price")),
        stop_loss: parse_decimal_val(signal.get("stop_loss")),
        take_profit: parse_decimal_val(signal.get("take_profit")),
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

fn baseline_regime(asset_class: AssetClass) -> MarketRegime {
    MarketRegime {
        asset_class,
        regime_label: "unknown".into(),
        volatility_regime: "normal".into(),
        bias: "neutral".into(),
        summary: "baseline regime".into(),
        details: json!({}),
    }
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

async fn load_portfolio_state(state: &AppState) -> anyhow::Result<PortfolioRiskState> {
    let row = sqlx::query_as::<_, (Decimal, Decimal, bool)>(
        "SELECT daily_pnl_pct, weekly_pnl_pct, circuit_breaker_active FROM portfolio_risk_state WHERE scope='global' ORDER BY updated_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    Ok(row
        .map(
            |(daily_pnl_pct, weekly_pnl_pct, circuit_breaker_active)| PortfolioRiskState {
                daily_pnl_pct,
                weekly_pnl_pct,
                circuit_breaker_active,
                open_positions: Vec::new(),
            },
        )
        .unwrap_or_default())
}

#[derive(Debug, sqlx::FromRow)]
struct ScoutPickRow {
    id: Uuid,
    run_id: Uuid,
    asset_class: String,
    ticker: String,
    correlation_group: String,
    timeframe: String,
    strategy_tag: String,
    action: String,
    confidence: f64,
    entry_price: Option<Decimal>,
    stop_loss: Option<Decimal>,
    take_profit: Option<Decimal>,
    guardrail_approved: bool,
    reject_reason: Option<String>,
    risk_reward_ratio: Option<Decimal>,
    position_size: Option<Decimal>,
    triage_rank: Option<i32>,
    priority_score: Option<f64>,
    llm_provider_used: Option<String>,
    market_regime_snapshot: Option<Value>,
    event_risk_snapshot: Option<Value>,
    reasoning_summary: String,
    created_at: chrono::DateTime<Utc>,
}

fn bad_request(msg: String) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"error": msg})))
}

fn internal_error<E: std::fmt::Display>(err: E) -> (StatusCode, Json<Value>) {
    tracing::error!(error = %err, "scout route error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":"internal server error"})),
    )
}
