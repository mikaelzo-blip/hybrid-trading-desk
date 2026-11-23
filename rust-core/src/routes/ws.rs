use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tracing::warn;

use crate::adapters::binance::BinanceAdapter;
use crate::models::market::{AssetClass, Candle, Timeframe};
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

    // Send initial snapshot from DB (last 200)
    if !send_snapshot(&mut socket, &state, asset_class, &ticker, timeframe).await {
        return;
    }

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(256);

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let reader = tokio::spawn(async move {
        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    let upstream = tokio::spawn(async move {
        match asset_class {
            AssetClass::Crypto => run_crypto_upstream(state, ticker, timeframe, tx).await,
            _ => run_poll_upstream(state, asset_class, ticker, timeframe, tx).await,
        }
    });

    tokio::select! {
        _ = reader => {},
        _ = writer => {},
    }
    upstream.abort();
}

async fn send_snapshot(
    socket: &mut WebSocket,
    state: &AppState,
    asset_class: AssetClass,
    ticker: &str,
    timeframe: Timeframe,
) -> bool {
    match crate::routes::candles::fetch_candles_db(
        state,
        asset_class,
        ticker,
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
            socket
                .send(Message::Text(msg.to_string().into()))
                .await
                .is_ok()
        }
        Err(err) => {
            warn!(error = %err, "failed to load snapshot");
            socket
                .send(Message::Text(json!({"type":"snapshot","asset_class":asset_class.as_str(),"ticker":ticker,"timeframe":timeframe.as_str(),"candles":[]}).to_string().into()))
                .await
                .is_ok()
        }
    }
}

async fn run_crypto_upstream(
    state: AppState,
    ticker: String,
    timeframe: Timeframe,
    tx: tokio::sync::mpsc::Sender<String>,
) {
    let mut backoff = crate::adapters::binance::Backoff::new(
        std::time::Duration::from_millis(500),
        std::time::Duration::from_secs(30),
    );
    let symbol = ticker.to_lowercase();
    let stream = format!("{}@kline_{}", symbol, timeframe.as_str());
    let ws_url = format!("{}/{}", state.binance.ws_base_url(), stream);
    loop {
        match stream_once(&ws_url, &ticker, timeframe, &state, &tx).await {
            Ok(()) => backoff.reset(),
            Err(err) => {
                warn!(error = %err, %ws_url, "upstream ws reconnecting");
            }
        }
        tokio::time::sleep(backoff.next_delay()).await;
    }
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
            let _ = BinanceAdapter::cache_live_candle(&state.redis, &candle, closed).await;
            if closed {
                let _ =
                    BinanceAdapter::persist_candles(&state.db, std::slice::from_ref(&candle)).await;
            }
            let payload = json!({ "type": "candle", "closed": closed, "candle": candle });
            if tx.send(payload.to_string()).await.is_err() {
                break;
            }
        }
    }
    Ok(())
}

async fn run_poll_upstream(
    state: AppState,
    asset_class: AssetClass,
    ticker: String,
    timeframe: Timeframe,
    tx: tokio::sync::mpsc::Sender<String>,
) {
    let mut last_open: Option<String> = None;
    let mut last_close: Option<String> = None;
    loop {
        match fetch_latest_candle(&state, asset_class, &ticker, timeframe).await {
            Ok(Some(candle)) => {
                let open_key = candle.open_time.to_rfc3339();
                let close_key = candle.close.to_string();
                let changed = last_open.as_ref() != Some(&open_key)
                    || last_close.as_ref() != Some(&close_key);
                let closed = last_open.as_ref().is_some() && last_open.as_ref() != Some(&open_key);
                if changed {
                    let payload = json!({ "type": "candle", "closed": closed, "candle": candle });
                    if tx.send(payload.to_string()).await.is_err() {
                        break;
                    }
                    last_open = Some(open_key);
                    last_close = Some(close_key);
                }
            }
            Ok(None) => {}
            Err(err) => warn!(error = %err, %ticker, "poll ws fetch failed"),
        }
        tokio::time::sleep(poll_interval(timeframe)).await;
    }
}

async fn fetch_latest_candle(
    state: &AppState,
    asset_class: AssetClass,
    ticker: &str,
    timeframe: Timeframe,
) -> anyhow::Result<Option<Candle>> {
    let candles = match asset_class {
        AssetClass::Crypto => {
            state
                .binance
                .fetch_klines(ticker, timeframe, None, None, 2)
                .await?
        }
        AssetClass::UsStock => state.alpaca.fetch_candles(ticker, timeframe, 2).await?,
        AssetClass::Forex => state.forex.fetch_candles(ticker, timeframe, 2).await?,
        AssetClass::IdxStock => state.idx.fetch_candles(ticker, timeframe, 2).await?,
    };
    if candles.is_empty() {
        return Ok(None);
    }
    let _ = BinanceAdapter::persist_candles(&state.db, &candles).await;
    Ok(candles.last().cloned())
}

fn poll_interval(timeframe: Timeframe) -> std::time::Duration {
    match timeframe {
        Timeframe::M1 => std::time::Duration::from_secs(5),
        Timeframe::M5 => std::time::Duration::from_secs(10),
        Timeframe::M15 => std::time::Duration::from_secs(15),
        Timeframe::H1 => std::time::Duration::from_secs(30),
        Timeframe::H4 => std::time::Duration::from_secs(60),
        Timeframe::D1 => std::time::Duration::from_secs(120),
    }
}
