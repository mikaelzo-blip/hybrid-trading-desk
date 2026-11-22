use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::engine::regime::RegimeEngine;
use crate::models::market::AssetClass;
use crate::routes::AppState;

pub async fn get_regime(
    State(state): State<AppState>,
    Path(asset_class_raw): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let asset_class: AssetClass = asset_class_raw
        .parse()
        .map_err(|e: String| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;

    let engine = RegimeEngine::with_default().map_err(internal_error)?;
    let regime = engine
        .get_regime(
            asset_class,
            &state.binance,
            &state.alpaca,
            &state.idx,
            &state.redis,
        )
        .await
        .map_err(internal_error)?;

    Ok(Json(
        serde_json::to_value(regime).map_err(|e| internal_error(anyhow::anyhow!(e)))?,
    ))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, Json<Value>) {
    tracing::error!(error = %err, "regime error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "internal server error" })),
    )
}
