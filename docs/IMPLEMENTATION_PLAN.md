# Implementation Plan — Hybrid AI Trading Desk

> Panduan eksekusi bertahap untuk **Oh My Pi (`omp`)** menggunakan **OpenCode Zen (Free) & OpenCode Go**.
> **Aturan Micro-Batch:** Kerjakan **2–3 task ID per iterasi**, jalankan *Verification Gate*, lalu ubah `[ ]` menjadi `[x]` hanya setelah lulus. Jangan menumpuk terlalu banyak file baru dalam satu giliran agar konteks model tetap tajam dan bebas *rate limit*.

---

## Ringkasan Status Fase

| Fase | Fokus Utama | Status |
| :--- | :--- | :--- |
| **Fase 1** | Fondasi Monorepo, PostgreSQL/Redis & Live Candlestick Crypto (Fast Path) | `[ ]` Belum mulai |
| **Fase 2** | MTF Indicator, Market Regime, 7-Layer Guardrail & `ai-worker` (OpenCode Zen/Go) | `[ ]` Belum mulai |
| **Fase 3** | Multi-Market Adapters (US, Forex, IDX), Event Risk Filter & Paper Trading | `[ ]` Belum mulai |
| **Fase 4** | Scouting Agent Funnel (4 Tahap, Sekuensial Free-Tier Safe) & AI Radar | `[ ]` Belum mulai |
| **Fase 5** | Chart Vision Analyst (`mplfinance` + Graceful Degradation) | `[ ]` Belum mulai |
| **Fase 6 (v2)** | Post-Trade Audit, Confidence Calibration & Reflection Memory | `[ ]` Opsional (Post-MVP) |

---

## Fase 1 — Fondasi Monorepo, Database & Live Chart Crypto (Fast Path)

**Tujuan:** Menghidupkan aliran candle *real-time* dari Binance -> `rust-core` -> `KLineChart` di WebUI Next.js tanpa menyentuh AI dulu.

### Batch 1A — Infrastruktur & Core Models
- [x] **P1-01** `(root)` Buat `docker-compose.yml` (`redis`, `postgres`, `rust-core`, `ai-worker`, `frontend`) dan `.env.example` lengkap dengan variabel **OpenCode Zen/Go** sesuai `docs/TRADING_DESK_SPEC.md` Bagian 11.
- [x] **P1-02** `(rust-core)` Inisialisasi Cargo project, konfigurasi `Cargo.toml`, `src/config.rs`, dan buat migrasi `migrations/0001_init.sql` (seluruh 6 tabel termasuk `llm_provider_used`, `portfolio_risk_state`, `post_trade_reviews`).
- [x] **P1-03** `(rust-core)` Implementasikan model standar di `src/models/market.rs` (`AssetClass`, `Timeframe`, `CorrelationGroup`, `Candle` dengan `Decimal`) dan trait `MarketAdapter` di `src/adapters/mod.rs`.

### Batch 1B — Binance Live Feed & API Gateway
- [x] **P1-04** `(rust-core)` Implementasikan `src/adapters/binance.rs`:
  - REST historis ke `BINANCE_REST_BASE_URL` (`/api/v3/klines`).
  - WebSocket stream kline dengan *exponential backoff auto-reconnect* dan dukungan opsional `OUTBOUND_PROXY_URL`.
  - Caching candle aktif & harga *live* terakhir ke Redis + penyimpanan historis ke PostgreSQL.
- [x] **P1-05** `(rust-core)` Implementasikan endpoint `GET /api/candles` (`src/routes/candles.rs`) dan `WS /ws/candles/:asset_class/:ticker/:tf` (`src/routes/ws.rs`) plus konfigurasi CORS di `src/main.rs`.

### Batch 1C — Next.js Live Chart WebUI
- [x] **P1-06** `(frontend)` Inisialisasi Next.js App Router + Tailwind CSS + `klinecharts` + `zustand` + `lucide-react`.
- [x] **P1-07** `(frontend)` Bangun `src/lib/api.ts`, `src/lib/ws.ts` (auto-reconnect), `MarketSelector.tsx`, dan `LiveCandleChart.tsx` yang memuat 200 candle historis lalu update real-time via WebSocket.

### Definition of Done (Fase 1)
- `cd rust-core && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` lulus.
- `cd frontend && npm run lint && npx tsc --noEmit && npm run build` lulus.
- `GET http://localhost:8080/api/candles?asset_class=crypto&ticker=BTCUSDT&timeframe=15m&limit=50` mengembalikan 50 candle valid.
- `http://localhost:3000` menampilkan chart `KLineChart` `BTCUSDT` bergerak *real-time* dan perpindahan timeframe (`1m`, `15m`, `1h`, `4h`, `1d`) berjalan mulus.

---

## Fase 2 — Indicator Engine MTF, Market Regime, 7-Layer Guardrail & `ai-worker` (OpenCode Zen/Go)

**Tujuan:** Mengaktifkan otak analisis multi-agent via tombol **Analyze Now** memakai **OpenCode Zen & OpenCode Go** dengan fallback otomatis, validasi 7 lapis di Rust, SSE streaming, dan overlay sinyal di chart.

### Batch 2A — Rust Technical, Structure & Regime Engine
- [x] **P2-01** `(rust-core)` Implementasikan `src/models/features.rs` (`HigherTfContext`, `ExecTfFeatures`, `MarketRegime`, `EventRisk`) dan `src/engine/indicators.rs` (`EMA 20/50/200`, `ADX 14`, `RSI 14`, `MACD`, `ATR 14`, `BB`, `Volume Ratio`, `VWAP`) dengan pemetaan otomatis *Execution TF -> Higher TF*.
- [x] **P2-02** `(rust-core)` Implementasikan `src/engine/structure.rs` (Swing High/Low window 5 bar, `nearest_support`, `nearest_resistance`, pola candlestick deterministik).
- [x] **P2-03** `(rust-core)` Implementasikan `src/engine/regime.rs` (versi awal Crypto: tren `BTCUSDT` 4H/1D, *Funding Rate* & *Open Interest* 24h via Binance Futures public API, cache Redis) dan endpoint `GET /api/regime/:asset_class`.

### Batch 2B — Rust 7-Layer Deterministic Guardrail
- [x] **P2-04** `(rust-core)` Implementasikan `src/models/signal.rs` dan 7-lapis `src/engine/guardrail.rs` menggunakan `rust_decimal`:
  - Lapis 1: *Drawdown Circuit Breaker* (`daily_pnl_pct`, `weekly_pnl_pct`).
  - Lapis 2: *Event Risk Hard-Block*.
  - Lapis 3: *Entry Staleness / Slippage Guard* (`|live_price - entry_price| <= 0.5 * ATR`).
  - Lapis 4–5: Validasi arah SL/TP, `MIN_RR_RATIO` (`1.5`), dan batas jarak SL berbasis ATR (`0.5x–4.0x`).
  - Lapis 6: *Portfolio Correlation & Concentration Limit*.
  - Lapis 7: *Regime-Adjusted Position Sizing* (potong 50% jika `volatility_regime == "extreme"` + pembulatan 100 lembar untuk `idx_stock`).
  - Unit test menyeluruh untuk seluruh 7 lapis.

### Batch 2C — Python LLM Router (OpenCode Zen/Go + Fallback) & JSON Repair
- [x] **P2-05** `(ai-worker)` Inisialisasi proyek Python (`pyproject.toml`, `Dockerfile`, FastAPI, `langchain-openai`, `tenacity`, `TauricResearch/TradingAgents`), `app/config.py`, dan `app/schemas.py` (sinkron penuh dengan `docs/TRADING_DESK_SPEC.md`).
- [x] **P2-06** `(ai-worker)` Implementasikan `app/llm_router.py`:
  - Factory OpenAI-compatible untuk `opencode_zen`, `opencode_go`, `gemini_free`, `groq_free`, `openrouter_free`, dan `ollama` — semua model ID & base URL dibaca dari `.env`.
  - `asyncio.Semaphore(LLM_MAX_CONCURRENCY)` + jeda pacing `LLM_NODE_DELAY_MS`.
  - Fallback chain otomatis saat `HTTP 429` / `5xx` / timeout + unit test simulasi `429` fallback (`tests/test_llm_router.py`).
- [x] **P2-07** `(ai-worker)` Implementasikan `app/json_repair.py` (hapus `<think>...</think>`, hapus markdown fence, ekstrak JSON terluar, validasi Pydantic v2, maks 1x repair call, fallback aman ke `HOLD`) + unit test (`tests/test_json_repair.py`).

### Batch 2D — TradingAgents Runner, SSE Proxy & Frontend Overlay
- [x] **P2-08** `(ai-worker)` Implementasikan `app/router.py`, `app/trading_agents_runner.py` (memakai prompt `docs/AGENT_PROMPTS.md` & `llm_router.py`), dan ekspos `POST /analyze/stream` (SSE: `router`, `analyst`, `debate`, `provider_fallback`, `final`) + unit test mock LLM di `tests/`.
- [x] **P2-09** `(rust-core)` Implementasikan `src/routes/analyze.rs` (`POST /api/analyze`): hitung fitur MTF + `market_regime` + `event_risk` -> proxy SSE dari `ai-worker` -> cek harga *live* Redis -> jalankan 7-lapis `guardrail.rs` -> simpan ke `agent_signals` (termasuk `llm_provider_used`) -> kirim hasil final ke frontend.
- [x] **P2-10** `(frontend)` Bangun `RegimeStatusBar.tsx`, `AgentDebateStream.tsx` (tampilkan juga badge provider aktif & notifikasi fallback bila terjadi), dan `TradeSignalPanel.tsx`, serta overlay marker `BUY`/`SELL` + garis Entry/SL/TP di `LiveCandleChart.tsx`.

### Definition of Done (Fase 2)
- `cd rust-core && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` lulus.
- `cd ai-worker && pytest` (termasuk `test_llm_router.py` dan `test_json_repair.py`) lulus.
- `cd frontend && npm run lint && npx tsc --noEmit && npm run build` lulus.
- Menekan **Analyze Now** untuk `BTCUSDT` (`15m`) berjalan sukses memakai OpenCode Zen/Go, menampilkan stream SSE, menyimpan record ke `agent_signals`, dan menggambar marker + SL/TP di `KLineChart`.

---

## Fase 3 — Multi-Market Adapters (US, Forex, IDX), Event Risk Filter & Paper Trading

**Tujuan:** Memperluas cakupan ke 4 pasar, melengkapi `regime.rs` & `event_risk.rs`, serta mengaktifkan *Paper Trading* semi-otomatis lengkap dengan *Drawdown Circuit Breaker*.

### Batch 3A — Multi-Market Adapters & Regime
- [x] **P3-01** `(rust-core)` Implementasikan `src/adapters/alpaca.rs` (candle IEX + Paper Order API), `src/adapters/forex.rs` (Twelve Data, `volume_reliability = "low_tick_only"`), dan `src/adapters/idx.rs` (ticker `.JK` + `^JKSE` + `USDIDR`).
- [x] **P3-02** `(rust-core)` Lengkapi `src/engine/regime.rs` untuk seluruh 4 pasar (`SPY`/`QQQ`/`VIX`, `^JKSE`/`USDIDR`, `DXY`/`US10Y`/sesi aktif, `BTC`/`BTC.D`/Funding/OI).

### Batch 3B — Event Risk & Paper Trading
- [x] **P3-03** `(rust-core)` Implementasikan `src/engine/event_risk.rs` (Earnings < 48 jam US, High-Impact Forex < 30 menit, FCA/UMA/Suspend IDX + hook Foreign Flow, Funding Rate ekstrem Crypto) + unit test.
- [x] **P3-04** `(rust-core)` Implementasikan `src/routes/paper.rs` (`POST /api/paper/orders` wajib `guardrail_approved = true` & update `portfolio_risk_state`, serta `GET /api/paper/portfolio`).
- [x] **P3-05** `(frontend)` Perbarui `MarketSelector.tsx` (4 pasar), tampilkan *Event Risk* & status *Circuit Breaker* di `RegimeStatusBar.tsx`, dan tambahkan tombol **Execute Paper Trade** + ringkasan portofolio di `TradeSignalPanel.tsx`.

### Definition of Done (Fase 3)
- Seluruh *Verification Gate* (`rust-core`, `ai-worker`, `frontend`) lulus.
- `GET /api/candles` dan `GET /api/regime/:asset_class` bekerja untuk `BTCUSDT`, `NVDA`, `EURUSD`, dan `BBCA.JK`.
- Sinyal terkena `event_risk.blocked_entry = true` atau `circuit_breaker_active = true` ditolak oleh `guardrail.rs` dan `POST /api/paper/orders` mengembalikan HTTP `422`.

---

## Fase 4 — Scouting Agent Funnel (4 Tahap, Free-Tier Safe) & AI Radar Sidebar

**Tujuan:** Membangun agen pemburu kandidat beli harian otomatis dengan antrean sekuensial yang aman untuk kuota gratis OpenCode Zen/Go.

### Batch 4A — Screener & AI Triage
- [x] **P4-01** `(rust-core)` Implementasikan `src/scout/screener.rs` (Tahap 1 — 0 token LLM): pemindaian paralel *universe* 4 pasar + pra-filter `event_risk` -> **Top 15 kandidat**.
- [x] **P4-02** `(ai-worker)` Implementasikan `app/triage_scout.py` dan endpoint `POST /scout/triage` (1 panggilan batch `quick_think_llm` lewat `llm_router.py` + `json_repair.py`) -> **Top 3–5 kandidat**.

### Batch 4B — Sequential Funnel, Scheduler & UI
- [ ] **P4-03** `(rust-core)` Implementasikan `src/routes/scout.rs` (`POST /api/scout/run`, `GET /api/scout/picks`): Tahap 1 -> Tahap 2 -> Tahap 3 Deep Dive **wajib diproses sekuensial (1 per 1)** dengan batas `SCOUT_MAX_DEEP_DIVE` (default `3` di `FREE_TIER_MODE=true`) -> 7-lapis `guardrail.rs` -> simpan ke `scout_picks`.
- [ ] **P4-04** `(rust-core)` Implementasikan `src/scout/scheduler.rs` memakai `tokio-cron-scheduler` jadwal WIB (`Asia/Jakarta`).
- [ ] **P4-05** `(frontend)` Bangun `AiRadarSidebar.tsx` (daftar `scout_picks` per pasar, klik kandidat buka live chart + overlay Entry/SL/TP, tombol **Run Scout Now**).

### Definition of Done (Fase 4)
- Seluruh *Verification Gate* lulus.
- `POST /api/scout/run` (`asset_class=crypto`) menyelesaikan funnel secara sekuensial tanpa memicu burst `429` dan mengisi `scout_picks`.
- Klik kandidat di `AiRadarSidebar.tsx` memuat candlestick *real-time* + overlay sinyal.

---

## Fase 5 — Chart Vision Analyst (Multimodal + Graceful Degradation)

**Tujuan:** Menambahkan analisis visual candlestick PNG yang aman dijalankan bahkan ketika model gratis aktif hanya mendukung teks.

- [ ] **P5-01** `(ai-worker)` Implementasikan `app/vision.py` (`mplfinance`, 80–120 candle + EMA 20/50 + volume, PNG < 500 KB) lengkap dengan *capability check* terhadap `VISION_MODEL` yang terkonfigurasi di `.env`.
- [ ] **P5-02** `(ai-worker)` Integrasikan node *Chart Vision Analyst* ke `app/trading_agents_runner.py` (emit `stage: "vision"` dengan `status: "completed"` atau `"skipped_unsupported"` bila tidak ada vision model aktif / kena limit).
- [ ] **P5-03** `(frontend)` Tambahkan toggle **Enable Chart Vision** di kontrol analisis WebUI dan tampilkan hasil/statusnya di `AgentDebateStream.tsx`.

### Definition of Done (Fase 5)
- Unit test `vision.py` memastikan PNG valid < 500 KB dan *graceful degradation* lulus saat `VISION_MODEL` kosong atau error `429`.

---

## Fase 6 (v2 / Post-MVP) — Post-Trade Audit, Confidence Calibration & Reflection Memory

- [ ] **P6-01** `(rust-core)` Implementasikan `GET /api/paper/audit` (kalibrasi *Confidence Score*, rata-rata *slippage* `entry_drift_pct`, distribusi R-multiple).
- [ ] **P6-02** `(ai-worker)` Job refleksi otomatis saat `paper_orders` ditutup untuk mengisi `post_trade_reviews` + suntikkan maks 3 pelajaran terbaru ke *Risk Manager*.
- [ ] **P6-03** `(frontend)` Tab **Performance Audit** (kurva ekuitas simulasi, tabel kalibrasi *confidence*, statistik *slippage*).

---

## Prompt Micro-Batch Siap Pakai untuk Oh My Pi (`omp` + OpenCode Zen/Go)

> **Tips Development Free Tier:** Jalankan per **Batch** (2–3 task) agar konteks tetap kecil, hasil kode rapi, dan tidak terkena timeout/rate limit.

### Contoh 1 — Mulai Batch 1A
```text
Baca AGENTS.md, lalu baca hanya bagian 3, 4.1, 5, dan 11 dari docs/TRADING_DESK_SPEC.md.
Kerjakan Batch 1A saja (P1-01, P1-02, P1-03) sesuai docs/IMPLEMENTATION_PLAN.md.
Jalankan Verification Gate rust-core, lalu centang [x] hanya untuk P1-01..P1-03 yang sudah lulus.
```

### Contoh 2 — Lanjut Batch Berikutnya (mis. Batch 1B, 1C, 2A, 2C, dst.)
```text
Baca AGENTS.md dan cek status terakhir di docs/IMPLEMENTATION_PLAN.md.
Kerjakan Batch [XX] saja (task [ID1..ID3]). Baca hanya bagian relevan di docs/TRADING_DESK_SPEC.md dan docs/AGENT_PROMPTS.md.
Jalankan Verification Gate terkait, perbaiki semua warning/error, lalu update checklist [x] di docs/IMPLEMENTATION_PLAN.md.
```