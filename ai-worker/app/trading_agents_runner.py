from __future__ import annotations

import asyncio
import json
from collections.abc import AsyncIterator
from typing import Any

from app.config import get_settings
from app.json_repair import repair_or_hold
from app.llm_router import LLMRouter
from app.router import asset_profile_prompt, enabled_analysts, event_risk_policy, normalize_event_risk
from app.schemas import Action, AgentSignalOutput, AnalyzeRequest, hold_fallback
from app.vision import analyze_chart_vision

SYSTEM_GUARDRAILS = """You are part of a quantitative multi-agent trading desk.
STRICT DATA INTEGRITY, TOKEN BUDGET & RISK RULES:
1. BREVITY: Keep your analysis under 120 words using concise, evidence-backed bullet points. No greetings or filler prose.
2. Trust the pre-computed Multi-Timeframe (MTF) indicators, Support/Resistance levels, Market Regime, and Event Risk in the JSON payload. Do NOT recalculate EMA, RSI, MACD, or ATR.
3. NEVER invent price levels. Ground all levels strictly in exec_tf_features.close, nearest_support, nearest_resistance, and atr_14.
4. INTERMARKET REGIME ALIGNMENT: Check market_regime before recommending a directional trade. Avoid fighting a strong opposing benchmark trend.
5. EVENT RISK OBEDIENCE: If event_risk.blocked_entry is true, you MUST recommend HOLD regardless of technical strength. If event_risk.severity is "caution", demand higher confluence and explicitly mention the risk.
6. Respect the Higher Timeframe trend bias. Only consider counter-trend setups on extreme exhaustion with clear candle structure and volume confirmation.
7. Risk geometry:
   - BUY: stop_loss < entry_price < take_profit
   - SELL: take_profit < entry_price < stop_loss
   - Risk:Reward >= 1.5 and stop_loss distance between 0.5x ATR and 4.0x ATR. Otherwise recommend HOLD."""


def _sse(event: str, data: dict[str, Any]) -> dict[str, Any]:
    return {"event": event, "data": data}


async def run_analysis(req: AnalyzeRequest, llm: LLMRouter | None = None) -> AsyncIterator[dict[str, Any]]:
    settings = get_settings()
    llm = llm or LLMRouter(settings)
    primary = settings.llm_primary_provider

    analysts = enabled_analysts(req)
    blocked, reason = event_risk_policy(req)
    risk = normalize_event_risk(req.event_risk)

    yield _sse("router", {
        "asset_class": req.asset_class.value,
        "ticker": req.ticker,
        "timeframe": req.timeframe.value,
        "enabled_analysts": analysts,
        "event_risk_blocked": blocked,
        "event_risk_reason": reason,
    })

    if blocked:
        out = hold_fallback(f"Event risk memblokir entry: {reason}")
        yield _sse("final", {"signal": out.model_dump(), "provider": "guardrail", "blocked": True})
        return

    try:
        context_json = json.dumps({
            "ticker": req.ticker,
            "asset_class": req.asset_class.value,
            "timeframe": req.timeframe.value,
            "higher_tf_context": req.higher_tf_context if isinstance(req.higher_tf_context, dict) else (req.higher_tf_context.model_dump() if req.higher_tf_context else None),
            "exec_tf_features": req.exec_tf_features if isinstance(req.exec_tf_features, dict) else req.exec_tf_features.model_dump(),
            "market_regime": req.market_regime if isinstance(req.market_regime, dict) else req.market_regime.model_dump(),
            "event_risk": req.event_risk if isinstance(req.event_risk, dict) else req.event_risk.model_dump(),
        }, ensure_ascii=False)

        if settings.free_tier_mode:
            if req.enable_vision:
                yield _sse("vision", {
                    "stage": "vision",
                    "status": "skipped_free_tier",
                    "reason": "Mode hemat kuota memakai konteks numerik saja.",
                })
            prompt = (
                f"{asset_profile_prompt(req.asset_class)}\n"
                f"CONTEXT JSON:\n{context_json}\n\n"
                "In one pass, weigh the strongest bullish and bearish evidence, then give "
                "one decision. Do not invent data or prices. If evidence is missing, return HOLD.\n"
                "Return only one JSON object. Use BUY, HOLD, or SELL for action; confidence "
                "must be between 0 and 1. Use plain numeric strings for entry_price, stop_loss, "
                "and take_profit; use zero stop_loss and take_profit for HOLD. "
                "Write a concise reasoning_summary in Bahasa Indonesia.\n"
                f"Required JSON schema: {json.dumps(AgentSignalOutput.model_json_schema(), ensure_ascii=False)}"
            )
            response = await llm.complete(
                [{"role": "system", "content": SYSTEM_GUARDRAILS}, {"role": "user", "content": prompt}],
                temperature=0.0,
                max_tokens=1200,
            )
            if response.provider != primary:
                yield _sse("provider_fallback", {"from": primary, "to": response.provider, "stage": "final"})
            # Spend no extra request on malformed output in the quota-saving path.
            signal = await repair_or_hold(response.text)
            yield _sse("final", {
                "signal": signal.model_dump(),
                "provider": response.provider,
                "model": response.model,
                "analyst_notes": [],
                "debate": {"bull": "", "bear": ""},
            })
            return

        analyst_notes: list[dict[str, str]] = []
        seen_provider_fallback = False

        vision_result: dict[str, Any] | None = None
        if req.enable_vision:
            candles_payload = []
            for c in req.candles[-120:]:
                candles_payload.append(c if isinstance(c, dict) else c.model_dump(mode="json"))
            vision_result = await analyze_chart_vision(
                candles=candles_payload,
                ticker=req.ticker,
                timeframe=req.timeframe.value,
                context_json=context_json,
                router=llm,
                settings=settings,
            )
            yield _sse("vision", {"stage": "vision", **vision_result})
            if vision_result.get("status") == "completed":
                analyst_notes.append({"analyst": "vision", "note": json.dumps(vision_result.get("analysis", {}), ensure_ascii=False), "provider": str(vision_result.get("provider", ""))})
        for analyst in analysts:
            prompt = (
                f"{SYSTEM_GUARDRAILS}\n\n{asset_profile_prompt(req.asset_class)}\n\n"
                f"ROLE: {analyst.upper()} analyst.\n"
                f"CONTEXT JSON:\n{context_json}\n\n"
                "Return concise bullet points in Bahasa Indonesia, max 120 words. No JSON, no markdown fence."
            )
            resp = await llm.complete(
                [
                    {"role": "system", "content": SYSTEM_GUARDRAILS},
                    {"role": "user", "content": prompt},
                ],
                temperature=0.2,
                max_tokens=500,
            )
            if resp.provider != primary and not seen_provider_fallback:
                seen_provider_fallback = True
                yield _sse("provider_fallback", {"from": primary, "to": resp.provider, "stage": "analyst"})
            note = resp.text.strip()
            analyst_notes.append({"analyst": analyst, "note": note, "provider": resp.provider})
            yield _sse("analyst", {"analyst": analyst, "note": note, "provider": resp.provider})

        # Debate (1 round by default)
        bull_prompt = (
            f"{SYSTEM_GUARDRAILS}\n\n{asset_profile_prompt(req.asset_class)}\n"
            f"CONTEXT JSON:\n{context_json}\n\nANALYST NOTES:\n{json.dumps(analyst_notes, ensure_ascii=False)}\n\n"
            "ROLE: Bull debater. Argue the strongest bullish case in max 120 words (Bahasa Indonesia). If not justified, admit it clearly."
        )
        bull = await llm.complete([{"role": "system", "content": SYSTEM_GUARDRAILS}, {"role": "user", "content": bull_prompt}], temperature=0.2, max_tokens=300)
        if bull.provider != primary and not seen_provider_fallback:
            seen_provider_fallback = True
            yield _sse("provider_fallback", {"from": primary, "to": bull.provider, "stage": "debate"})
        yield _sse("debate", {"side": "bull", "argument": bull.text.strip(), "provider": bull.provider})

        bear_prompt = (
            f"{SYSTEM_GUARDRAILS}\n\n{asset_profile_prompt(req.asset_class)}\n"
            f"CONTEXT JSON:\n{context_json}\n\nANALYST NOTES:\n{json.dumps(analyst_notes, ensure_ascii=False)}\n\nBULL ARGUMENT:\n{bull.text}\n\n"
            "ROLE: Bear debater. Argue the strongest bearish/risk case in max 120 words (Bahasa Indonesia). If not justified, admit it clearly."
        )
        bear = await llm.complete([{"role": "system", "content": SYSTEM_GUARDRAILS}, {"role": "user", "content": bear_prompt}], temperature=0.2, max_tokens=300)
        if bear.provider != primary and not seen_provider_fallback:
            seen_provider_fallback = True
            yield _sse("provider_fallback", {"from": primary, "to": bear.provider, "stage": "debate"})
        yield _sse("debate", {"side": "bear", "argument": bear.text.strip(), "provider": bear.provider})

        # Final Risk Manager decision (strict JSON)
        debate_summary = f"BULL:\n{bull.text}\n\nBEAR:\n{bear.text}\n\nANALYSTS:\n{json.dumps(analyst_notes, ensure_ascii=False)}"
        final_prompt = f"""Synthesize the analyst reports, intermarket regime, event risk warnings, and the Bull vs Bear debate into a single final trading decision.

ASSET CONTEXT:
- Ticker: {req.ticker}
- Asset Class: {req.asset_class.value}
- Execution Timeframe: {req.timeframe.value}
- Higher Timeframe Context: {json.dumps(req.higher_tf_context if isinstance(req.higher_tf_context, dict) else (req.higher_tf_context.model_dump() if req.higher_tf_context else None), ensure_ascii=False)}
- Execution TF Technical Features: {json.dumps(req.exec_tf_features if isinstance(req.exec_tf_features, dict) else req.exec_tf_features.model_dump(), ensure_ascii=False)}
- Intermarket Regime: {json.dumps(req.market_regime if isinstance(req.market_regime, dict) else req.market_regime.model_dump(), ensure_ascii=False)}
- Event Risk Status: {json.dumps(risk.model_dump(), ensure_ascii=False)}

DEBATE & RESEARCH SUMMARY:
{debate_summary}

OUTPUT RULES:
- Return ONLY a single valid JSON object. No markdown fences, no commentary before or after the JSON.
- Price fields MUST be plain numeric strings without currency symbols or thousands separators (e.g., "91450.50"). Use "0" for stop_loss and take_profit if action is "HOLD".
- `confidence` MUST be a float between 0.00 and 1.00.

EXACT JSON SCHEMA:
{{
  "action": "BUY" | "HOLD" | "SELL",
  "confidence": 0.00,
  "entry_price": "91450.50",
  "stop_loss": "90750.00",
  "take_profit": "92900.00",
  "reasoning_summary": "Ringkasan objektif 2-4 kalimat dalam Bahasa Indonesia (bias higher TF, market regime/event risk, konfirmasi utama, alasan SL/TP)."
}}"""

        final_resp = await llm.complete(
            [{"role": "system", "content": SYSTEM_GUARDRAILS}, {"role": "user", "content": final_prompt}],
            temperature=0.0,
            max_tokens=1200,
        )
        if final_resp.provider != primary and not seen_provider_fallback:
            seen_provider_fallback = True
            yield _sse("provider_fallback", {"from": primary, "to": final_resp.provider, "stage": "final"})

        async def repair_call(prompt: str, _malformed: str) -> str:
            nonlocal final_resp
            final_resp = await llm.complete(
                [
                    {"role": "system", "content": SYSTEM_GUARDRAILS},
                    {"role": "user", "content": final_prompt},
                    {"role": "user", "content": (
                        f"{prompt}\nUse only the market context above. "
                        "Do not invent prices or trade setups; return HOLD if the evidence is insufficient."
                    )},
                ],
                temperature=0.0,
                max_tokens=1200,
            )
            return final_resp.text

        signal = await repair_or_hold(final_resp.text, repair_call=repair_call)

        # Enforce event_risk hard block at output
        if risk.blocked_entry and signal.action != Action.HOLD:
            signal = hold_fallback(f"Event risk memblokir entry: {'; '.join(risk.reasons) if risk.reasons else 'blocked'}")

        yield _sse("final", {
            "signal": signal.model_dump(),
            "provider": final_resp.provider,
            "model": final_resp.model,
            "analyst_notes": analyst_notes,
            "debate": {"bull": bull.text.strip(), "bear": bear.text.strip()},
        })
    except Exception as e:
        yield _sse("error", {"error": str(e)})
        yield _sse("final", {
            "signal": hold_fallback(f"AI analysis unavailable: {e}").model_dump(),
            "provider": "fallback",
            "model": None,
            "error": str(e),
        })
