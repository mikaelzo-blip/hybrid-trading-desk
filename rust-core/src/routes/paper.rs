use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::engine::guardrail::{self, GuardrailInput};
use crate::engine::indicators::compute_exec_features;
use crate::engine::regime::RegimeEngine;
use crate::models::market::{AssetClass, CorrelationGroup, Timeframe};
use crate::models::signal::{Action, OpenPosition, PortfolioRiskState, ProposedSignal};
use crate::routes::AppState;

#[derive(Debug, Deserialize)]
pub struct PaperOrderRequest {
    pub signal_id: Option<Uuid>,
    pub asset_class: String,
    pub ticker: String,
    pub correlation_group: Option<String>,
    pub side: String, // BUY | SELL
    pub entry_price: Decimal,
    pub stop_loss: Option<Decimal>,
    pub take_profit: Option<Decimal>,
    pub position_size: Decimal,
    #[serde(default)]
    #[allow(dead_code)]
    pub guardrail_approved: bool, // client hint ignored; server evaluates
    #[serde(default = "default_timeframe")]
    pub timeframe: String,
}

fn default_timeframe() -> String {
    "15m".to_owned()
}

pub async fn place_paper_order(
    State(state): State<AppState>,
    Json(req): Json<PaperOrderRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let asset_class: AssetClass = req.asset_class.parse().map_err(bad_request)?;
    let timeframe: Timeframe = req.timeframe.parse().map_err(bad_request)?;
    let ticker = req.ticker.trim().to_uppercase();
    let side = req.side.trim().to_uppercase();
    let action = match side.as_str() {
        "BUY" => Action::Buy,
        "SELL" => Action::Sell,
        _ => return Err(bad_request("side must be BUY or SELL".into())),
    };
    if req.position_size <= Decimal::ZERO || req.entry_price <= Decimal::ZERO {
        return Err(bad_request(
            "entry_price and position_size must be > 0".into(),
        ));
    }
    let stop_loss = req
        .stop_loss
        .ok_or_else(|| bad_request("stop_loss is required".into()))?;
    let take_profit = req
        .take_profit
        .ok_or_else(|| bad_request("take_profit is required".into()))?;

    let group = req
        .correlation_group
        .clone()
        .unwrap_or_else(|| "general".into());
    let correlation_group = CorrelationGroup::from(group.clone());

    let proposed = ProposedSignal {
        asset_class,
        ticker: ticker.clone(),
        correlation_group: correlation_group.clone(),
        timeframe,
        action,
        confidence: 1.0,
        entry_price: req.entry_price,
        stop_loss,
        take_profit,
        reasoning_summary: "paper order request".to_owned(),
        llm_provider_used: None,
    };

    let candles = crate::routes::candles::fetch_candles_db(
        &state,
        asset_class,
        &ticker,
        timeframe,
        None,
        None,
        200,
    )
    .await
    .map_err(internal_error)?;
    let exec = compute_exec_features(asset_class, &ticker, timeframe, &candles);
    let atr = exec.as_ref().and_then(|f| f.atr_14).unwrap_or(Decimal::ONE);
    let live_price = fetch_live_price(&state, &ticker)
        .await
        .unwrap_or(req.entry_price);
    let portfolio = load_portfolio_state(&state, asset_class)
        .await
        .map_err(internal_error)?;
    let regime = load_regime(&state, asset_class).await;
    let event_risk = crate::engine::event_risk::evaluate(
        asset_class,
        &ticker,
        &crate::engine::event_risk::EventRiskInput::default(),
    );

    let validated = guardrail::evaluate(&GuardrailInput {
        signal: proposed,
        live_price,
        atr_14: atr,
        portfolio,
        event_risk: event_risk.clone(),
        market_regime: regime,
        equity: Decimal::from(10_000u64),
        base_position_size: req.position_size,
        daily_drawdown_limit_pct: Decimal::from(2u64),
        weekly_drawdown_limit_pct: Decimal::from(5u64),
    });

    if !validated.approved {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "guardrail_rejected",
                "reject_reasons": validated.reject_reasons,
                "risk_reward_ratio": validated.risk_reward_ratio.map(|v| v.to_string()),
                "position_size": validated.position_size.map(|v| v.to_string()),
            })),
        ));
    }

    let id = Uuid::new_v4();
    let approved_size = validated.position_size.unwrap_or(req.position_size);
    sqlx::query(
        r#"INSERT INTO paper_orders (
            id, signal_id, asset_class, ticker, correlation_group, side, status,
            entry_price, stop_loss, take_profit, position_size, filled_price, guardrail_approved
        ) VALUES ($1,$2,$3,$4,$5,$6,'open',$7,$8,$9,$10,$11,$12)"#,
    )
    .bind(id)
    .bind(req.signal_id)
    .bind(asset_class.as_str())
    .bind(&ticker)
    .bind(&group)
    .bind(&side)
    .bind(req.entry_price)
    .bind(stop_loss)
    .bind(take_profit)
    .bind(approved_size)
    .bind(req.entry_price)
    .bind(true)
    .execute(&state.db)
    .await
    .map_err(internal_error)?;

    sqlx::query(
        r#"INSERT INTO portfolio_risk_state (scope, asset_class, equity, daily_pnl_pct, weekly_pnl_pct, peak_equity, drawdown_pct, circuit_breaker_active, updated_at)
           VALUES ('global', NULL, 10000, 0, 0, 10000, 0, false, $1)
           ON CONFLICT (scope, asset_class) DO UPDATE SET updated_at = EXCLUDED.updated_at"#,
    )
    .bind(Utc::now())
    .execute(&state.db)
    .await
    .map_err(internal_error)?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "status":"open",
            "guardrail_approved": true,
            "reject_reasons": [],
            "position_size": approved_size.to_string(),
            "risk_reward_ratio": validated.risk_reward_ratio.map(|v| v.to_string())
        })),
    ))
}

pub async fn get_paper_portfolio(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let orders = sqlx::query_as::<_, PaperOrderRow>(
        r#"SELECT id, asset_class, ticker, side, status, entry_price, stop_loss, take_profit, position_size, realized_pnl, opened_at, closed_at
           FROM paper_orders ORDER BY opened_at DESC LIMIT 100"#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(internal_error)?;

    let risk = sqlx::query_as::<_, (Decimal, Decimal, bool)>(
        "SELECT daily_pnl_pct, weekly_pnl_pct, circuit_breaker_active FROM portfolio_risk_state WHERE scope='global' ORDER BY updated_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await
    .map_err(internal_error)?;

    let (daily_pnl_pct, weekly_pnl_pct, circuit_breaker_active) =
        risk.unwrap_or((Decimal::ZERO, Decimal::ZERO, false));

    Ok(Json(json!({
        "orders": orders.into_iter().map(|o| json!({
            "id": o.id, "asset_class": o.asset_class, "ticker": o.ticker, "side": o.side,
            "status": o.status, "entry_price": o.entry_price.to_string(),
            "stop_loss": o.stop_loss.map(|v| v.to_string()),
            "take_profit": o.take_profit.map(|v| v.to_string()),
            "position_size": o.position_size.to_string(),
            "realized_pnl": o.realized_pnl.map(|v| v.to_string()),
            "opened_at": o.opened_at, "closed_at": o.closed_at
        })).collect::<Vec<_>>(),
        "risk_state": {
            "daily_pnl_pct": daily_pnl_pct.to_string(),
            "weekly_pnl_pct": weekly_pnl_pct.to_string(),
            "circuit_breaker_active": circuit_breaker_active
        }
    })))
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

async fn load_portfolio_state(
    state: &AppState,
    asset_class: AssetClass,
) -> anyhow::Result<PortfolioRiskState> {
    let row = sqlx::query_as::<_, (Decimal, Decimal, bool)>(
        "SELECT daily_pnl_pct, weekly_pnl_pct, circuit_breaker_active FROM portfolio_risk_state WHERE scope='global' ORDER BY updated_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    let open_positions = sqlx::query_as::<_, (String, Decimal)>(
        "SELECT correlation_group, position_size FROM paper_orders WHERE status='open' AND asset_class=$1",
    )
    .bind(asset_class.as_str())
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|(group, size)| OpenPosition {
        correlation_group: CorrelationGroup::from(group),
        asset_class,
        ticker: String::new(),
        side: Action::Buy,
        notional: size,
    })
    .collect();

    let Some((daily_pnl_pct, weekly_pnl_pct, circuit_breaker_active)) = row else {
        return Ok(PortfolioRiskState {
            open_positions,
            ..PortfolioRiskState::default()
        });
    };
    Ok(PortfolioRiskState {
        daily_pnl_pct,
        weekly_pnl_pct,
        circuit_breaker_active,
        open_positions,
    })
}

async fn load_regime(
    state: &AppState,
    asset_class: AssetClass,
) -> crate::models::features::MarketRegime {
    let Ok(engine) = RegimeEngine::with_default() else {
        return baseline_regime(asset_class);
    };
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
}

fn baseline_regime(asset_class: AssetClass) -> crate::models::features::MarketRegime {
    crate::models::features::MarketRegime {
        asset_class,
        regime_label: "unknown".into(),
        volatility_regime: "normal".into(),
        bias: "neutral".into(),
        summary: "baseline regime".into(),
        details: json!({}),
    }
}

#[derive(Debug, sqlx::FromRow)]
struct PaperOrderRow {
    id: Uuid,
    asset_class: String,
    ticker: String,
    side: String,
    status: String,
    entry_price: Decimal,
    stop_loss: Option<Decimal>,
    take_profit: Option<Decimal>,
    position_size: Decimal,
    realized_pnl: Option<Decimal>,
    opened_at: chrono::DateTime<Utc>,
    closed_at: Option<chrono::DateTime<Utc>>,
}

fn bad_request(msg: String) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"error": msg})))
}

fn internal_error<E: std::fmt::Display>(err: E) -> (StatusCode, Json<Value>) {
    tracing::error!(error = %err, "paper route error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":"internal server error"})),
    )
}
