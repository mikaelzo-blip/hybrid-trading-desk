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
    id                     UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id                 UUID NOT NULL,
    asset_class            VARCHAR(20) NOT NULL,
    ticker                 VARCHAR(32) NOT NULL,
    correlation_group      VARCHAR(40) NOT NULL DEFAULT 'general',
    timeframe              VARCHAR(10) NOT NULL,
    candle_timestamp       TIMESTAMPTZ NOT NULL,
    strategy_tag           VARCHAR(50) NOT NULL, -- 'momentum_breakout' | 'pullback_support' | 'mean_reversion'
    action                 VARCHAR(10) NOT NULL, -- 'BUY' | 'HOLD' | 'SELL'
    confidence             DOUBLE PRECISION NOT NULL,
    entry_price            NUMERIC(20, 8),
    stop_loss              NUMERIC(20, 8),
    take_profit            NUMERIC(20, 8),
    guardrail_approved     BOOLEAN NOT NULL DEFAULT FALSE,
    reject_reason          TEXT,
    risk_reward_ratio      NUMERIC(10, 2),
    position_size          NUMERIC(20, 8),
    triage_rank            INTEGER,
    priority_score         DOUBLE PRECISION,
    llm_provider_used      VARCHAR(50),
    market_regime_snapshot JSONB,
    event_risk_snapshot    JSONB,
    reasoning_summary      TEXT NOT NULL,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 4. Order paper trading (semi-auto, guardrail-approved)
CREATE TABLE IF NOT EXISTS paper_orders (
    id                     UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    signal_id              UUID REFERENCES agent_signals(id),
    asset_class            VARCHAR(20) NOT NULL,
    ticker                 VARCHAR(32) NOT NULL,
    correlation_group      VARCHAR(40) NOT NULL DEFAULT 'general',
    side                   VARCHAR(10) NOT NULL, -- 'BUY' | 'SELL'
    status                 VARCHAR(20) NOT NULL DEFAULT 'open', -- 'open' | 'closed' | 'cancelled'
    entry_price            NUMERIC(20, 8) NOT NULL,
    stop_loss              NUMERIC(20, 8),
    take_profit            NUMERIC(20, 8),
    position_size          NUMERIC(20, 8) NOT NULL,
    filled_price           NUMERIC(20, 8),
    exit_price             NUMERIC(20, 8),
    realized_pnl           NUMERIC(20, 8),
    realized_pnl_pct       NUMERIC(10, 4),
    guardrail_approved     BOOLEAN NOT NULL DEFAULT FALSE,
    opened_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at              TIMESTAMPTZ,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 5. Snapshot risiko portofolio & circuit breaker state
CREATE TABLE IF NOT EXISTS portfolio_risk_state (
    id                    UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    scope                 VARCHAR(20) NOT NULL DEFAULT 'global', -- 'global' | per asset_class
    asset_class           VARCHAR(20),
    equity                NUMERIC(20, 8) NOT NULL DEFAULT 0,
    daily_pnl_pct         NUMERIC(10, 4) NOT NULL DEFAULT 0,
    weekly_pnl_pct        NUMERIC(10, 4) NOT NULL DEFAULT 0,
    peak_equity           NUMERIC(20, 8) NOT NULL DEFAULT 0,
    drawdown_pct          NUMERIC(10, 4) NOT NULL DEFAULT 0,
    circuit_breaker_active BOOLEAN NOT NULL DEFAULT FALSE,
    circuit_breaker_reason TEXT,
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (scope, asset_class)
);

-- 6. Post-trade reflection / calibration memory (Fase 6, schema prepared early)
CREATE TABLE IF NOT EXISTS post_trade_reviews (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    paper_order_id      UUID NOT NULL REFERENCES paper_orders(id) ON DELETE CASCADE,
    asset_class         VARCHAR(20) NOT NULL,
    ticker              VARCHAR(32) NOT NULL,
    signal_id           UUID REFERENCES agent_signals(id),
    outcome             VARCHAR(20) NOT NULL, -- 'win' | 'loss' | 'breakeven' | 'cancelled'
    r_multiple          NUMERIC(10, 4),
    confidence_at_entry DOUBLE PRECISION,
    slippage_pct        NUMERIC(10, 4),
    lessons             TEXT NOT NULL,
    reflection_payload  JSONB,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agent_signals_lookup
    ON agent_signals (asset_class, ticker, timeframe, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_scout_picks_run
    ON scout_picks (run_id, asset_class, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_paper_orders_status
    ON paper_orders (status, asset_class, ticker, opened_at DESC);
CREATE INDEX IF NOT EXISTS idx_post_trade_reviews_order
    ON post_trade_reviews (paper_order_id, created_at DESC);
