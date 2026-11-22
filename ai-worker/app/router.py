from __future__ import annotations

from app.schemas import AnalyzeRequest, AssetClass, EventRisk


def enabled_analysts(req: AnalyzeRequest) -> list[str]:
    """Asset-aware analyst selection. Fundamentals disabled for forex/crypto."""
    analysts = ["market", "news", "social"]
    if req.asset_class in {AssetClass.us_stock, AssetClass.idx_stock}:
        analysts.insert(1, "fundamentals")
    return analysts


def normalize_event_risk(raw: EventRisk | dict) -> EventRisk:
    if isinstance(raw, EventRisk):
        return raw
    return EventRisk.model_validate(raw)


def event_risk_policy(req: AnalyzeRequest) -> tuple[bool, str | None]:
    """Returns (blocked, reason). Deterministic pre-check; LLM must still obey."""
    risk = normalize_event_risk(req.event_risk)
    if risk.blocked_entry:
        reason = "; ".join(risk.reasons) if risk.reasons else "event_risk.blocked_entry=true"
        return True, reason
    return False, None


def asset_profile_prompt(asset_class: AssetClass) -> str:
    if asset_class == AssetClass.us_stock:
        return (
            "ASSET PROFILE: US Equities.\n"
            "Focus on earnings momentum, SEC/financial fundamentals, sector strength, volume confirmation, and Finnhub news. "
            "Evaluate against market_regime (SPY/QQQ trend and VIX). If event_risk flags earnings within 48 hours, you MUST recommend HOLD."
        )
    if asset_class == AssetClass.idx_stock:
        return (
            "ASSET PROFILE: Indonesian Equities (IDX / BEI).\n"
            "Focus on liquidity, trend structure, volume breakout, corporate actions (right issue, dividend, split), and local catalysts. "
            "Weight market_regime (IHSG /^JKSE trend and USD/IDR) heavily. Treat Foreign Flow (Net Buy/Sell) as institutional confirmation when available. "
            "Immediately output HOLD if event_risk flags FCA, UMA, or suspension."
        )
    if asset_class == AssetClass.forex:
        return (
            "ASSET PROFILE: Spot Forex.\n"
            "Company fundamentals DO NOT EXIST—ignore equity valuation. Focus on higher-timeframe structure, liquidity sweeps, central bank rates, CPI/NFP, and market_regime (DXY, US10Y, active session: Asia vs London/NY). "
            "Volume is tick volume only (volume_reliability: low_tick_only). Immediately output HOLD if event_risk.blocked_entry is true."
        )
    return (
        "ASSET PROFILE: Spot/Perpetual Crypto (24/7 high-volatility).\n"
        "Company fundamentals DO NOT EXIST. Focus on MTF structure, volume expansion, market_regime (BTC trend, BTC Dominance, Funding Rate, Open Interest), and breaking crypto news/sentiment. "
        "Warn against chasing longs when Funding Rate is extreme positive (long-squeeze risk). Account for wider wicks using ATR."
    )
