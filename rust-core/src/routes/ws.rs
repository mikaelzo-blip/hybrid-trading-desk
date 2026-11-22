use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tracing::warn;

use crate::adapters::binance::BinanceAdapter;
use crate::models::market::{AssetClass, Timeframe};
use crate::routes::AppState;

pub async fn ws_candles(
    ws: WebSocketUpgrade,
    Path((asset_class, ticker, tf)): Path<(String, String, String)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, asset_class, ticker, tf))
}

async fn handle_socket(
    mut socket: WebSocket,
    state: AppState,
    asset_class_raw: String,
    ticker_raw: String,
    tf_raw: String,
) {
    let asset_class: AssetClass = match asset_class_raw.parse() {
        Ok(v) => v,
        Err(e) => {
            let _ = socket
                .send(Message::Text(json!({ "error": e }).to_string().into()))
                .await;
            return;
        }
    };
    let timeframe: Timeframe = match tf_raw.parse() {
        Ok(v) => v,
        Err(e) => {
            let _ = socket
                .send(Message::Text(json!({ "error": e }).to_string().into()))
                .await;
            return;
        }
    };
    let ticker = ticker_raw.trim().to_uppercase();

    if !matches!(asset_class, AssetClass::Crypto) {
        let _ = socket
            .send(Message::Text(
                json!({ "error": "only crypto asset_class is supported in Batch 1B" })
                    .to_string()
                    .into(),
            ))
            .await;
        return;
    }

    // Send initial snapshot from DB (last 200)
    match crate::routes::candles::fetch_candles_db(
        &state,
        asset_class,
        &ticker,
        timeframe,
        None,
        None,
        200,
    )
    .await
    {
        Ok(candles) => {
            let msg = json!({
                "type": "snapshot",
                "asset_class": asset_class.as_str(),
                "ticker": ticker,
                "timeframe": timeframe.as_str(),
                "candles": candles,
            });
            if socket
                .send(Message::Text(msg.to_string().into()))
                .await
                .is_err()
            {
                return;
            }
        }
        Err(err) => {
            warn!(error = %err, "failed to load snapshot");
        }
    }

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(256);

    // Forward upstream messages to websocket client
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Drain client messages (we don't require client input; handle close/ping)
    let reader = tokio::spawn(async move {
        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    // Upstream binance stream loop
    let state_clone = state.clone();
    let ticker_clone = ticker.clone();
    let tx_clone = tx.clone();
    let upstream = tokio::spawn(async move {
        let mut backoff = crate::adapters::binance::Backoff::new(
            std::time::Duration::from_millis(500),
            std::time::Duration::from_secs(30),
        );
        let symbol = ticker_clone.to_lowercase();
        let stream = format!("{}@kline_{}", symbol, timeframe.as_str());
        let ws_url = format!("{}/{}", state_clone.binance.ws_base_url(), stream);
        loop {
            match stream_once(&ws_url, &ticker_clone, timeframe, &state_clone, &tx_clone).await {
                Ok(()) => backoff.reset(),
                Err(err) => {
                    warn!(error = %err, %ws_url, "upstream ws reconnecting");
                }
            }
            tokio::time::sleep(backoff.next_delay()).await;
        }
    });

    tokio::select! {
        _ = reader => {},
        _ = writer => {},
    }
    upstream.abort();
}

async fn stream_once(
    ws_url: &str,
    ticker: &str,
    timeframe: Timeframe,
    state: &AppState,
    tx: &tokio::sync::mpsc::Sender<String>,
) -> anyhow::Result<()> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let request = ws_url.into_client_request()?;
    let (mut upstream, _) = tokio_tungstenite::connect_async(request).await?;
    while let Some(msg) = upstream.next().await {
        let msg = msg?;
        if !msg.is_text() {
            continue;
        }
        let text = msg.into_text()?;
        if let Some((candle, closed)) =
            crate::adapters::binance::parse_ws_kline(&text, ticker, timeframe)?
        {
            // Cache live candle in Redis
            let _ = BinanceAdapter::cache_live_candle(&state.redis, &candle, closed).await;
            // Persist closed candle to DB
            if closed {
                let _ =
                    BinanceAdapter::persist_candles(&state.db, std::slice::from_ref(&candle)).await;
            }
            let payload = json!({
                "type": "candle",
                "closed": closed,
                "candle": candle,
            });
            if tx.send(payload.to_string()).await.is_err() {
                break;
            }
        }
    }
    Ok(())
}
