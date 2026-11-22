use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::Utc;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::models::market::AssetClass;
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
    pub guardrail_approved: bool,
}

#[derive(Debug, Serialize)]
pub struct PaperOrderResponse {
    pub id: Uuid,
    pub status: String,
}

pub async fn place_paper_order(
    State(state): State<AppState>,
    Json(req): Json<PaperOrderRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let asset_class: AssetClass = req.asset_class.parse().map_err(bad_request)?;
    let ticker = req.ticker.trim().to_uppercase();
    let side = req.side.trim().to_uppercase();
    if side != "BUY" && side != "SELL" {
        return Err(bad_request("side must be BUY or SELL".into()));
    }
    if !req.guardrail_approved {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error":"guardrail_approved must be true"})),
        ));
    }
    if req.position_size <= Decimal::ZERO || req.entry_price <= Decimal::ZERO {
        return Err(bad_request(
            "entry_price and position_size must be > 0".into(),
        ));
    }

    let id = Uuid::new_v4();
    let group = req.correlation_group.unwrap_or_else(|| "general".into());
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
    .bind(req.stop_loss)
    .bind(req.take_profit)
    .bind(req.position_size)
    .bind(req.entry_price)
    .bind(req.guardrail_approved)
    .execute(&state.db)
    .await
    .map_err(internal_error)?;

    // Update portfolio risk state (baseline: ensure global row exists, mark breaker false unless already active)
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
        Json(json!({"id": id, "status":"open"})),
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
