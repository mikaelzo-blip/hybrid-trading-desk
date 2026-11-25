# Hybrid AI Trading Desk

Multi-agent trading desk monorepo with real-time candlestick WebUI, deterministic guardrails, and an AI analysis worker. Default mode is **analysis + paper trading (semi-auto)** — no live auto-execution.

## Architecture

| Path | Stack | Role | Default Port |
|---|---:|---|---:|
| `rust-core/` | Rust, Axum, Tokio, SQLx, Redis, `rust_decimal` | Fast path: market adapters, OHLCV/MTF features, regime/event-risk engine, 7-layer guardrail, paper trading, scout screener/scheduler, API gateway | `8080` |
| `ai-worker/` | Python, FastAPI, OpenAI-compatible LLM router | Slow path: TradingAgents-style analysis, JSON repair/fallback, scout triage, chart vision (graceful degradation) | `8000` |
| `frontend/` | Next.js App Router, TypeScript, Tailwind, `klinecharts`, Zustand | Trading desk UI: live chart, regime/risk status, SSE debate stream, paper trade controls, AI Radar sidebar | `3000` |

## Key Safety Rules

- **Financial precision:** `rust-core` uses `rust_decimal::Decimal` for prices, equity, SL/TP, drawdown, R:R, and position sizing. No `f32/f64` money math.
- **7-layer deterministic guardrail:** Every BUY/SELL signal passes `rust-core/src/engine/guardrail.rs` before approval/persist (circuit breaker, event-risk hard block, staleness/slippage, SL/TP direction, R:R/ATR, correlation/concentration, regime-adjusted sizing).
- **No live auto-execution:** Paper trading only unless explicitly extended.
- **Fast vs slow path split:** LLM calls never run inside websocket/tick loops.
- **LLM agnostic:** Provider/model/base URL come from env via `ai-worker/app/llm_router.py` (OpenAI-compatible). JSON outputs go through `ai-worker/app/json_repair.py` with repair + safe HOLD fallback.
- **Asset-aware routing:** Fundamentals disabled for `forex`/`crypto`; event-risk rules cover US earnings, forex high-impact windows, IDX FCA/UMA/suspend, and extreme crypto funding.

## Quick Start (Docker)

1. Copy env template and fill secrets locally (never commit `.env`):

   ```bash
   cp .env.example .env
   ```

2. Start services:

   ```bash
   docker compose up --build
   ```

3. Open:
   - Frontend: `http://localhost:3000`
   - Rust core: `http://localhost:8080/health`
   - AI worker: `http://localhost:8000/health`

## Local Development

### Rust core

```bash
cd rust-core
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run
```

### AI worker

```bash
cd ai-worker
python -m venv .venv
# activate venv, then:
pip install -e .[dev]
pytest
uvicorn app.main:app --reload --port 8000
```

### Frontend

```bash
cd frontend
npm install
npm run lint
npx tsc --noEmit
npm run build
npm run dev
```

## Environment Variables

See `.env.example` for the full list. Important groups:

- **Core:** `DATABASE_URL`, `REDIS_URL`, `RUST_CORE_BIND`, `AI_WORKER_BASE_URL`
- **Binance:** `BINANCE_REST_BASE_URL`, `BINANCE_WS_BASE_URL`, optional `OUTBOUND_PROXY_URL` (Indonesia access)
- **LLM router:** `LLM_PRIMARY_PROVIDER`, `LLM_FALLBACK_PROVIDERS`, `FREE_TIER_MODE`, `LLM_MAX_CONCURRENCY`, `LLM_NODE_DELAY_MS`, provider keys/models
- **Vision:** `ENABLE_CHART_VISION`, `VISION_MODEL` (graceful skip if unsupported/unset/429)
- **Scout:** `SCOUT_MAX_DEEP_DIVE`, scheduler uses WIB (`Asia/Jakarta`)

### Use a free model through 9Router

With [9Router](https://9router.github.io/en/getting-started/quick-start/) running on the host, choose a free model ID from `http://localhost:20128/v1/models` and add these settings to `.env`:

```dotenv
LLM_PRIMARY_PROVIDER=9router
LLM_FALLBACK_PROVIDERS=
NINE_ROUTER_BASE_URL=http://host.docker.internal:20128/v1
NINE_ROUTER_API_KEY=
NINE_ROUTER_MODEL=
```

Fill in the key from the 9Router dashboard and the exact model ID. For a worker running outside Docker, use `http://localhost:20128/v1`. Apply Docker settings with `docker compose up -d --build --no-deps ai-worker`. An empty fallback keeps the worker from switching to another provider.

## Main API Surface

- `GET /api/candles` — OHLCV candles (all 4 asset classes). With no date range, crypto history ends at the latest closed Binance candle; stale or gapped history is refetched. Live, in-progress candles arrive over WebSocket.
- `GET /api/regime/{asset_class}` — market regime snapshot
- `POST /api/analyze` — SSE analysis stream; rust-core computes features/regime/event-risk, proxies ai-worker, applies guardrail, persists signal
- `POST /api/paper/orders` — re-evaluate guardrails and create a simulated order. For Scout, send only `{"scout_pick_id":"<uuid>"}`; the server loads the approved BUY/SELL and records its ID on the order.
- `GET /api/paper/portfolio` — paper orders + portfolio risk state
- `POST /api/scout/run` — Stage 1 screener -> Stage 2 triage -> Stage 3 sequential deep dive -> guardrail -> `scout_picks`
- `GET /api/scout/picks` — AI Radar picks
- `WS /ws/candles/{asset_class}/{ticker}/{tf}` — realtime candle stream (crypto upstream wired)

## Implemented Phases

- **Phase 1:** Monorepo scaffold, Docker Compose, env template, Rust models/migration, Binance adapter, candles REST/WS, frontend live chart.
- **Phase 2:** MTF indicators/structure, regime engine, 7-layer guardrail, ai-worker LLM router + JSON repair, SSE analyze flow, frontend signal/guardrail overlays.
- **Phase 3:** Alpaca/Forex/IDX adapters, multi-market regime, event-risk engine, paper trading routes, frontend risk controls and paper execution.
- **Phase 4:** Scout screener + AI triage, sequential scout funnel/scheduler (WIB), AI Radar sidebar.
- **Phase 5:** Chart vision PNG analysis with capability check and graceful degradation; frontend vision toggle/status.

Phase 6 (post-trade audit/reflection) is intentionally deferred.

## Verification Gate

```bash
cd rust-core && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd ai-worker && pytest
cd frontend && npm run lint && npx tsc --noEmit && npm run build
```
