from __future__ import annotations

from enum import Enum
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field, field_validator


class AssetClass(str, Enum):
    crypto = "crypto"
    us_stock = "us_stock"
    forex = "forex"
    idx_stock = "idx_stock"


class Timeframe(str, Enum):
    m1 = "1m"
    m5 = "5m"
    m15 = "15m"
    h1 = "1h"
    h4 = "4h"
    d1 = "1d"


class Action(str, Enum):
    BUY = "BUY"
    HOLD = "HOLD"
    SELL = "SELL"


class StrictModel(BaseModel):
    model_config = ConfigDict(extra="allow")


class Candle(StrictModel):
    asset_class: AssetClass
    ticker: str
    timeframe: Timeframe
    open_time: str
    open: str
    high: str
    low: str
    close: str
    volume: str


class HigherTfContext(StrictModel):
    timeframe: Timeframe
    trend_bias: str
    ema_20: str | None = None
    ema_50: str | None = None
    ema_200: str | None = None
    adx_14: float | None = None
    rsi_14: float | None = None
    macd: float | None = None
    macd_signal: float | None = None
    nearest_support: str | None = None
    nearest_resistance: str | None = None


class ExecTfFeatures(StrictModel):
    asset_class: AssetClass
    ticker: str
    timeframe: Timeframe
    close: str
    ema_20: str | None = None
    ema_50: str | None = None
    ema_200: str | None = None
    adx_14: float | None = None
    rsi_14: float | None = None
    macd: float | None = None
    macd_signal: float | None = None
    macd_hist: float | None = None
    atr_14: str | None = None
    bb_upper: str | None = None
    bb_middle: str | None = None
    bb_lower: str | None = None
    volume_ratio: float | None = None
    vwap: str | None = None
    nearest_support: str | None = None
    nearest_resistance: str | None = None
    structure_notes: list[str] = Field(default_factory=list)


class MarketRegime(StrictModel):
    asset_class: AssetClass
    regime_label: str
    volatility_regime: str
    bias: str
    summary: str
    details: dict[str, Any] = Field(default_factory=dict)


class EventRisk(StrictModel):
    asset_class: AssetClass
    blocked_entry: bool = False
    severity: str = "none"
    reasons: list[str] = Field(default_factory=list)
    details: dict[str, Any] = Field(default_factory=dict)


class AnalyzeRequest(StrictModel):
    asset_class: AssetClass
    ticker: str
    timeframe: Timeframe
    higher_tf_context: HigherTfContext | dict[str, Any] | None = None
    exec_tf_features: ExecTfFeatures | dict[str, Any]
    market_regime: MarketRegime | dict[str, Any]
    event_risk: EventRisk | dict[str, Any]
    candles: list[Candle] | list[dict[str, Any]] = Field(default_factory=list)
    enable_vision: bool = False


class AgentSignalOutput(StrictModel):
    action: Action
    confidence: float = Field(ge=0.0, le=1.0)
    entry_price: str
    stop_loss: str
    take_profit: str
    reasoning_summary: str

    @field_validator("entry_price", "stop_loss", "take_profit", mode="before")
    @classmethod
    def coerce_price_str(cls, v: Any) -> str:
        if v is None:
            return "0"
        if isinstance(v, (int, float)):
            return format(float(v), "f").rstrip("0").rstrip(".") or "0"
        s = str(v).strip().replace(",", "")
        return s if s else "0"

    @field_validator("reasoning_summary", mode="before")
    @classmethod
    def coerce_summary(cls, v: Any) -> str:
        return str(v or "").strip()


class VisionAnalysisOutput(StrictModel):
    visual_trend: Literal[
        "strong_uptrend",
        "weak_uptrend",
        "ranging",
        "weak_downtrend",
        "strong_downtrend",
    ]
    chart_patterns: list[str] = Field(default_factory=list)
    key_zones: dict[str, str] = Field(default_factory=dict)
    price_action_bias: Literal["bullish", "neutral", "bearish"]
    trap_warnings: list[str] = Field(default_factory=list)
    visual_summary: str


class ScoutCandidate(StrictModel):
    ticker: str
    asset_class: AssetClass
    strategy_tag: str | None = None
    metrics: dict[str, Any] = Field(default_factory=dict)
    headlines: list[str] = Field(default_factory=list)


class ScoutTriageRequest(StrictModel):
    asset_class: AssetClass
    strategy_tag: str = "momentum_breakout"
    timestamp_wib: str
    max_select: int = 3
    market_regime: MarketRegime | dict[str, Any]
    candidates: list[ScoutCandidate] = Field(default_factory=list)


class SelectedCandidate(StrictModel):
    ticker: str
    triage_rank: int
    priority_score: float = Field(ge=0.0, le=1.0)
    triage_notes: str


class RejectedCandidate(StrictModel):
    ticker: str
    reason: str


class ScoutTriageOutput(StrictModel):
    selected_candidates: list[SelectedCandidate] = Field(default_factory=list)
    rejected_summary: list[RejectedCandidate] = Field(default_factory=list)


def hold_fallback(reason: str) -> AgentSignalOutput:
    return AgentSignalOutput(
        action=Action.HOLD,
        confidence=0.0,
        entry_price="0",
        stop_loss="0",
        take_profit="0",
        reasoning_summary=reason,
    )
