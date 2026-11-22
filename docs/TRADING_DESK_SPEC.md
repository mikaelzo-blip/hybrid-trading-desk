# Hybrid AI Trading Desk — Architecture & Implementation Spec

> **Target Executor:** **OpenCode 2 CLI** (dioptimalkan untuk eksekusi menggunakan **OpenCode Zen Free & OpenCode Go**)
>
> **Mode Eksekusi Default:** Analisis Real-Time + Paper Trading (Simulasi) — **DILARANG mengaktifkan auto-execution ke akun live tanpa persetujuan eksplisit pengguna.**
>
> **Cakupan Pasar:** Crypto, Saham AS (US Equities), Forex, dan Saham Indonesia (IDX / BEI).

## 1. Tujuan Sistem & Prinsip Arsitektur

Membangun sistem **Multi-Agent Trading Desk** berbasis WebUI dengan candlestick *real-time*, *Scouting Agent* otomatis untuk mencari kandidat beli harian, serta mesin keputusan `BUY / HOLD / SELL` menggunakan **`TauricResearch/TradingAgents`** yang berjalan hemat biaya dengan **OpenCode Zen (Free) & OpenCode Go** (plus fallback opsional ke Cloud Free Tier lain / Ollama lokal).

### Aturan Desain Wajib (Agent Guardrails)

1. **Arsitektur Hybrid (Rust + Python + Next.js):**
   * **Rust (`rust-core`)** menangani *Fast Path*: koneksi WebSocket bursa, agregasi candle OHLCV, perhitungan indikator teknikal & *Market Regime*, *Quantitative Screener* Tahap 1, *Deterministic Risk & Portfolio Guardrail*, penyimpanan PostgreSQL/Redis, dan API Gateway tunggal ke frontend.
   * **Python (`ai-worker`)** menangani *Slow Path*: *LLM Gateway Router* (OpenCode Zen/Go + fallback), orkestrasi **`TauricResearch/TradingAgents`** (LangGraph), *Asset-Aware Router*, *Event Risk Filter*, *AI Triage Scout*, *JSON Repair Layer*, render chart PNG (`mplfinance`) untuk *Chart Vision*, serta *streaming* progres debat via SSE.
   * **Next.js (`frontend`)** menangani WebUI: chart candlestick *real-time* (`KLineChart`), overlay panah sinyal & garis SL/TP, panel *live debate stream*, indikator *Market Regime & Circuit Breaker*, dan sidebar *AI Radar*.
2. **Presisi Keuangan di Rust:** Semua harga, volume, ekuitas, SL, TP, *drawdown*, dan *position sizing* di `rust-core` **WAJIB** menggunakan `rust_decimal::Decimal` (dilarang menggunakan `f64` untuk kalkulasi moneter/order).
3. **Pemisahan Jalur (Dual-Path):** Streaming candlestick tidak boleh terblokir oleh eksekusi LLM. Proses `TradingAgents` hanya dipicu oleh tombol **Analyze Now**, *candle close* pada timeframe terpilih, atau hasil *Scouting Agent*.
4. **LLM Provider-Agnostic & Free-Tier Resilient:** Dilarang meng-*hardcode* vendor atau nama model di kode Python. Semua panggilan LLM wajib lewat `app/llm_router.py` (berbasis protokol OpenAI-compatible) yang mendukung **OpenCode Zen**, **OpenCode Go**, serta fallback otomatis saat terjadi `HTTP 429` / timeout, dilengkapi antrean sekuensial (*anti-burst*) dan *JSON Repair*.
5. **Konteks Multi-Timeframe (MTF) & Intermarket Regime:** Setiap analisis agen wajib menerima ringkasan 2 timeframe (**Higher Timeframe** + **Execution Timeframe**) ditambah objek `market_regime` (konteks indeks induk/makro) dan `event_risk` (kalender rilis/anomali pasar).
6. **Deterministic Risk & Portfolio Guardrail:** Output JSON dari LLM **tidak pernah dipercaya mentah-mentah**. Semua sinyal `BUY`/`SELL` wajib melewati 7 lapis validator deterministik di Rust sebelum disimpan sebagai *Approved Plan* atau dikirim ke Paper Trading.

## 2. Alur Sistem (High-Level Architecture)

```text
[Market Data & Intermarket Providers]
  ├── Crypto   : Binance (WS + data-api.binance.vision REST + Futures Funding/OI)
  ├── US Stock : Alpaca (WS IEX + Paper Trading) + Finnhub (News/Fundamentals/Earnings)
  ├── Forex    : Twelve Data / MetaTrader 5 Bridge + Economic Calendar Feed
  └── IDX/BEI  : yfinance (.JK + ^JKSE + USDIDR) [MVP] -> Extensible ke Vendor/Broker Feed
        │
        ▼
┌──────────────────────────────────────────────────────────────────────┐
│  RUST CORE ENGINE (Axum + Tokio + SQLx + Redis) — Port 8080          │
│  1. Market Ingestor & Unified OHLCV Normalizer                       │
│  2. MTF Indicator, Candle Structure & Market Regime Engine           │
│  3. Scouting Stage 1: Parallel Quantitative Screener                 │
│  4. Deterministic Trade + Portfolio Guardrail & Circuit Breaker      │
│  5. Single API Gateway (REST + WS Candlestick + SSE Proxy)           │
└───────────────────┬────────────────────────────────┬─────────────────┘
                    │ WS & REST                      │ HTTP / SSE (Internal)
                    ▼                                ▼
┌────────────────────────────────────┐  ┌──────────────────────────────────────────┐
│  NEXT.JS WEBUI — Port 3000         │  │  PYTHON AI WORKER — Port 8000            │
│  • KLineChart Live Candlestick     │  │  • LLM Router (OpenCode Zen/Go+Fallback) │
│  • BUY/HOLD/SELL & SL/TP Overlay   │  │  • FastAPI + TradingAgents (Semaphore=1) │
│  • Live Agent Debate Panel (SSE)   │  │  • Asset-Aware Router + JSON Repair      │
│  • Market Regime & Risk Status Bar │  │  • Chart Vision (Graceful Degradation)   │
│  • AI Radar (Scout Top Picks)      │  │  • Scouting Stage 2 (AI Triage Batch)    │
└────────────────────────────────────┘  └──────────────────────────────────────────┘
```

## 3. Struktur Direktori Monorepo

```text
hybrid-trading-desk/
├── AGENTS.md
├── docker-compose.yml
├── .env.example
├── docs/
│   ├── TRADING_DESK_SPEC.md
│   ├── IMPLEMENTATION_PLAN.md
│   └── AGENT_PROMPTS.md
├── rust-core/
│   ├── Cargo.toml
│   ├── Dockerfile
│   ├── migrations/
│   │   └── 0001_init.sql
│   └── src/
│       ├── main.rs                 # Bootstrap Axum, Redis, Postgres, Scheduler
│       ├── config.rs               # Env parser
│       ├── models/
│       │   ├── market.rs           # Unified Candle, AssetClass, Timeframe, CorrelationGroup
│       │   ├── features.rs         # MTF Technical Features, MarketRegime, EventRisk
│       │   └── signal.rs           # AgentSignal, ValidatedOrderPlan, CircuitBreakerState
│       ├── adapters/
│       │   ├── mod.rs              # Trait MarketAdapter
│       │   ├── binance.rs          # Crypto WS + REST + Funding Rate / Open Interest
│       │   ├── alpaca.rs           # US Equities WS/REST + Paper Order API
│       │   ├── forex.rs            # Twelve Data / MT5 adapter
│       │   └── idx.rs              # IDX (.JK) adapter + ^JKSE/USDIDR benchmark
│       ├── engine/
│       │   ├── indicators.rs       # EMA, ADX, RSI, MACD, ATR, BB, VWAP, Vol Ratio
│       │   ├── structure.rs        # Swing High/Low, S/R terdekat, Pola Candle
│       │   ├── regime.rs           # Intermarket Regime (SPY/VIX, IHSG/USDIDR, BTC/BTC.D, DXY/US10Y)
│       │   ├── event_risk.rs       # Filter kalender earnings, high-impact forex, UMA/FCA, funding ekstrem
│       │   └── guardrail.rs        # 7-Lapis Trade + Portfolio Guardrail & Circuit Breaker
│       ├── scout/
│       │   ├── screener.rs         # Tahap 1: Quantitative Screener paralel
│       │   └── scheduler.rs        # Cron schedule per sesi pasar (WIB)
│       └── routes/
│           ├── ws.rs               # /ws/candles/:market/:ticker/:tf
│           ├── candles.rs          # GET /api/candles
│           ├── regime.rs           # GET /api/regime/:asset_class
│           ├── analyze.rs          # POST /api/analyze (proxy SSE + guardrail)
│           ├── scout.rs            # GET /api/scout/picks, POST /api/scout/run
│           └── paper.rs            # POST /api/paper/orders, GET /api/paper/portfolio, GET /api/paper/audit
├── ai-worker/
│   ├── pyproject.toml
│   ├── Dockerfile
│   └── app/
│       ├── main.py                 # FastAPI endpoints (/health, /analyze/stream, /scout/triage)
│       ├── config.py               # Env loader untuk OpenCode Zen/Go, fallback & pacing
│       ├── schemas.py              # Pydantic v2 kontrak JSON (sinkron dgn Rust)
│       ├── llm_router.py           # OpenAI-compatible Client Factory + Fallback Chain + Backoff + Semaphore
│       ├── json_repair.py          # Strip <think>, strip fences, extract JSON, Pydantic validate + 1x retry
│       ├── router.py               # Asset-Aware Router + Event Risk policy
│       ├── vision.py               # Render PNG via mplfinance (80-120 candle) + capability check
│       ├── trading_agents_runner.py # Wrapper TauricResearch/TradingAgents + SSE emitter
│       └── triage_scout.py         # Tahap 2 Scouting: penyaring berita & Top 3-5 ranking
└── frontend/
    ├── package.json
    ├── Dockerfile
    └── src/
        ├── app/
        │   ├── layout.tsx
        │   └── page.tsx            # Trading Desk main workspace
        ├── lib/
        │   ├── api.ts              # Fetcher REST & SSE client ke rust-core
        │   └── ws.ts               # Auto-reconnecting WebSocket client
        └── components/
            ├── MarketSelector.tsx  # Pilih pasar (Crypto/US/Forex/IDX), ticker, TF
            ├── RegimeStatusBar.tsx # Status Intermarket Regime, Event Risk & Circuit Breaker
            ├── LiveCandleChart.tsx # KLineChart + marker panah + garis Entry/SL/TP
            ├── AgentDebateStream.tsx # Live stream SSE tahap router -> analis -> vision -> debat -> final
            ├── TradeSignalPanel.tsx # Ringkasan keputusan, Guardrail status, Paper Trade btn
            └── AiRadarSidebar.tsx  # Daftar hasil Scouting Agent harian
```

## 4. Tech Stack & Dependensi

### 4.1 `rust-core` (`Cargo.toml`)
* **Runtime & Web:** `tokio` (`full`), `axum` (`ws`, `macros`), `tower-http` (`cors`, `trace`), `reqwest` (`json`, `stream`, `rustls-tls`), `tokio-tungstenite` (`rustls-tls-native-roots`)
* **Serialization & Numerics:** `serde`, `serde_json`, `rust_decimal`, `rust_decimal_macros`, `chrono`, `chrono-tz`, `uuid`
* **Database & Cache:** `sqlx` (`runtime-tokio`, `postgres`, `rust_decimal`, `chrono`, `uuid`), `redis` (`tokio-comp`)
* **Concurrency & Scheduling:** `rayon`, `tokio-cron-scheduler`, `futures-util`
* **Logging & Error:** `tracing`, `tracing-subscriber`, `thiserror`, `anyhow`

### 4.2 `ai-worker` (`pyproject.toml`)
* **Core:** `fastapi`, `uvicorn[standard]`, `pydantic>=2`, `httpx`, `python-dotenv`, `tenacity`
* **Agent Framework:** `TauricResearch/TradingAgents` (install via git / submodule) + `langgraph`
* **LLM Providers (OpenAI-Compatible First):** `openai`, `langchain-openai` (untuk **OpenCode Zen**, **OpenCode Go**, Groq, OpenRouter, dan Ollama via `base_url`), opsional `langchain-google-genai` (jika memakai endpoint native Google AI Studio).
* **Chart Vision & Data Support:** `mplfinance`, `pandas`, `yfinance`, `finnhub-python`
* **Testing:** `pytest`, `pytest-asyncio`, `respx`

### 4.3 `frontend` (`package.json`)
* **Framework:** Next.js (App Router, TypeScript), React 19, Tailwind CSS
* **Charting:** `klinecharts` (v9/v10)
* **Icons & State:** `lucide-react`, `zustand`

## 5. Skema Database (`rust-core/migrations/0001_init.sql`)

```sql
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- 1. Cache historis candle OHLCV terstandarisasi
CREATE TABLE IF NOT EXISTS candles (
    asset_class VARCHAR(20) NOT NULL, -- 'crypto' | 'us_stock' | 'forex' | 'idx_stock'
    ticker      VARCHAR(32) NOT NULL,
    timeframe   VARCHAR(10) NOT NULL, -- '1m','5m','15m','1h','4h','1d'
    open_time   TIMESTAMPTZ NOT NULL,
    open        NUMERIC(20, 8) NOT NULL,
    high        NUMERIC(20, 8) NOT NULL,
    low         NUMERIC(20, 8) NOT NULL,
    close       NUMERIC(20, 8) NOT NULL,
    volume      NUMERIC(24, 8) NOT NULL,
    PRIMARY KEY (asset_class, ticker, timeframe, open_time)
);

-- 2. Riwayat eksekusi analisis TradingAgents + hasil Risk & Portfolio Guardrail
CREATE TABLE IF NOT EXISTS agent_signals (
    id                     UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    asset_class            VARCHAR(20) NOT NULL,
    ticker                 VARCHAR(32) NOT NULL,
    correlation_group      VARCHAR(40) NOT NULL DEFAULT 'general',
    timeframe              VARCHAR(10) NOT NULL,
    candle_timestamp       TIMESTAMPTZ NOT NULL,
    action                 VARCHAR(10) NOT NULL, -- 'BUY' | 'HOLD' | 'SELL'
    confidence             DOUBLE PRECISION NOT NULL,
    entry_price            NUMERIC(20, 8),
    live_price_at_decision NUMERIC(20, 8),
    entry_drift_pct        NUMERIC(10, 4),
    stop_loss              NUMERIC(20, 8),
    take_profit            NUMERIC(20, 8),
    guardrail_approved     BOOLEAN NOT NULL DEFAULT FALSE,
    reject_reason          TEXT,
    risk_reward_ratio      NUMERIC(10, 2),
    position_size          NUMERIC(20, 8),
    llm_provider_used      VARCHAR(50),          -- 'opencode_zen' | 'opencode_go' | 'gemini_free' | 'ollama'
    market_regime_snapshot JSONB,
    event_risk_snapshot    JSONB,
    reasoning_summary      TEXT NOT NULL,
    debate_log             JSONB,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 3. Daftar kandidat harian hasil Scouting Agent (AI Radar)
CREATE TABLE IF NOT EXISTS scout_picks (
    id                UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id            UUID NOT NULL,
    asset_class       VARCHAR(20) NOT NULL,
    ticker            VARCHAR(32) NOT NULL,
    strategy_tag      VARCHAR(50) NOT NULL, -- 'momentum_breakout' | 'pullback_support' | '