# AGENTS.md — Hybrid AI Trading Desk

> Instruksi kerja permanen untuk **OpenCode 2 CLI** di seluruh sesi.
> Dioptimalkan untuk *development* menggunakan **OpenCode Zen (Free) & OpenCode Go**. Baca file ini terlebih dahulu sebelum mengubah kode apa pun.

## 1. Ringkasan Proyek

Sistem **Multi-Agent Trading Desk** berbasis WebUI dengan candlestick *real-time*, *Scouting Agent* otomatis, *Intermarket Regime & Event Risk Filter*, serta analisis `BUY / HOLD / SELL` menggunakan **`TauricResearch/TradingAgents`** untuk 4 pasar (`crypto`, `us_stock`, `forex`, `idx_stock`).

## 2. Batas Workspace (Monorepo Boundaries)

- **`rust-core/`** — *Fast Path & Core Gateway* (Rust, Axum, Tokio, SQLx, Redis, `rust_decimal`, Port `8080`): WebSocket/REST bursa, OHLCV, indikator MTF, `regime.rs`, `event_risk.rs`, *Quantitative Screener* Tahap 1, 7-lapis `guardrail.rs` + *Circuit Breaker*, DB/Redis, API Gateway.
- **`ai-worker/`** — *Slow Path AI Service* (Python, FastAPI, `TauricResearch/TradingAgents`, `langchain-openai`, `mplfinance`, Port `8000`): `llm_router.py` (OpenCode Zen/Go + fallback), `json_repair.py`, *Asset-Aware Router*, SSE debate stream, *Chart Vision*, *Scout Stage 2 Triage*.
- **`frontend/`** — *Trading Desk WebUI* (Next.js App Router, TypeScript, Tailwind CSS, `klinecharts`, Zustand, Port `3000`): live candlestick, overlay sinyal & SL/TP, `RegimeStatusBar`, panel SSE, kontrol Paper Trading, sidebar *AI Radar*.

## 3. Disiplin Development (OpenCode 2 CLI + Free Zen & Go)

Aturan ini wajib dipatuhi agar sesi coding di OpenCode 2 CLI tetap tajam, hemat konteks, dan bebas *rate limit*:

1. **Plan-First lalu Build per Micro-Batch:** Sebelum menulis kode untuk sebuah batch, susun daftar file yang akan dibuat/diubah terlebih dahulu. Kerjakan maksimal **2–3 item checklist** (`P1-01..P1-03`) dalam satu giliran/sesi.
2. **Context Hygiene:** Jangan membaca seluruh isi `docs/` sekaligus. Baca hanya bagian spesifik dari `docs/TRADING_DESK_SPEC.md` atau `docs/AGENT_PROMPTS.md` yang relevan dengan task ID aktif, dan lakukan *compact*/sesi baru setelah satu batch selesai diverifikasi.
3. **Verifikasi Cepat Setelah Setiap Edit:** Jalankan test/compile terfokus pada modul yang baru diubah (`cargo test <modul>`, `pytest tests/<file>`, `npx tsc --noEmit`) sebelum lanjut ke file berikutnya.
4. **Dilarang Menebak Schema:** Jika butuh nama kolom SQL, field JSON, atau env var, cek langsung bagian terkait di `docs/TRADING_DESK_SPEC.md`.

## 4. Aturan Keras Sistem (Non-Negotiable Hard Rules)

1. **Presisi Keuangan Wajib (`rust_decimal`):** Di `rust-core/`, **DILARANG** memakai `f64`/`f32` untuk harga, ekuitas, SL/TP, *drawdown*, R:R, atau *position sizing*. Wajib `rust_decimal::Decimal`.
2. **Dilarang Live Auto-Execution:** Mode default adalah **Analisis + Paper Trading (Semi-Auto)**. Jangan pernah menambahkan kode eksekusi order akun live tanpa instruksi eksplisit.
3. **7-Lapis Deterministic Guardrail Wajib:** Setiap sinyal `BUY`/`SELL` dari LLM wajib melewati `rust-core/src/engine/guardrail.rs` (Circuit Breaker, Event Risk Hard-Block, Entry Staleness/Slippage, arah SL/TP, R:R & ATR, Portfolio Correlation Limit, Regime-Adjusted Position Sizing).
4. **Pemisahan Fast Path vs Slow Path:** Dilarang memanggil `ai-worker` atau LLM di dalam loop WebSocket tick harga.
5. **LLM Agnostik & Free-Tier Safe:**
   - **Dilarang hardcode nama model atau URL provider** di kode Python — wajib baca dari `.env` melalui `ai-worker/app/llm_router.py` (protokol OpenAI-compatible untuk OpenCode Zen, OpenCode Go, dan fallback).
   - Wajib menegakkan `asyncio.Semaphore(LLM_MAX_CONCURRENCY)` (default `1`) dan jeda `LLM_NODE_DELAY_MS` saat `FREE_TIER_MODE=true`.
   - Wajib melewati `app/json_repair.py` (hapus blok `<think>...</think>`, hapus markdown fence, validasi Pydantic v2, maks 1x retry, fallback aman ke `HOLD`) sebelum mengembalikan keputusan ke `rust-core`.
   - Di Fase 4 (*Scouting*), Top 3–5 kandidat **wajib diproses sekuensial** (bukan paralel) saat `FREE_TIER_MODE=true`.
   - Di Fase 5 (*Vision*), jika model aktif tidak mendukung gambar atau terkena limit, **wajib *graceful degrade*** (skip Vision, lanjutkan analisis angka OHLCV).
6. **Anti-Duplikasi Perhitungan:** Indikator MTF, `market_regime`, dan `event_risk` dihitung di `rust-core/`. `ai-worker` hanya membaca payload JSON tersebut.
7. **Asset-Aware Routing & Event Risk Policy:** Matikan *Fundamentals Analyst* saham untuk `forex` & `crypto`. Blokir entry baru untuk: `us_stock` (*earnings* < 48 jam), `forex` (berita *high-impact* < 30 menit), dan `idx_stock` (status FCA/UMA/Suspend).
8. **Akses Binance di Indonesia & Lot IDX:** Pakai `BINANCE_REST_BASE_URL` (`https://data-api.binance.vision`) + dukung `OUTBOUND_PROXY_URL`. Untuk `idx_stock`, bulatkan *position sizing* ke bawah dalam kelipatan **100 lembar (1 lot)**.

## 5. Sumber Kebenaran (Documentation Pointers)

- **Arsitektur, Kontrak JSON, WebSocket, SSE, LLM Router & Skema DB:** `docs/TRADING_DESK_SPEC.md`
- **Checklist Fase, Micro-Batch & Definition of Done:** `docs/IMPLEMENTATION_PLAN.md`
- **System Prompt LLM & Aturan JSON Repair:** `docs/AGENT_PROMPTS.md`

## 6. Verification Gate (Wajib Lolos Sebelum Centang `[x]`)

- **Rust (`rust-core/`):**
  ```bash
  cd rust-core && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
  ```
- **Python (`ai-worker/`):**
  ```bash
  cd ai-worker && pytest
  ```
- **Frontend (`frontend/`):**
  ```bash
  cd frontend && npm run lint && npx tsc --noEmit && npm run build
  ```

## 7. Aturan Progres & Sinkronisasi

- Jangan mencentang `[x]` di `docs/IMPLEMENTATION_PLAN.md` sebelum semua perintah di *Verification Gate* benar-benar lulus.
- Jika kontrak JSON `rust-core` <-> `ai-worker` berubah, perbarui `rust-core/src/models/`, `ai-worker/app/schemas.py`, dan `docs/TRADING_DESK_SPEC.md` dalam perubahan yang sama.
- Jangan pernah commit API key/rahasia; simpan di `.env` dan perbarui `.env.example` bila menambah variabel baru.