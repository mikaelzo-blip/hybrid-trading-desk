from __future__ import annotations

import json

from app.json_repair import repair_and_validate
from app.llm_router import LLMRouter
from app.schemas import ScoutTriageOutput, ScoutTriageRequest

_SYSTEM = """You are a disciplined Quantitative Triage Scout.
Evaluate up to 15 pre-screened trading candidates with their headlines and current Intermarket Regime, then select ONLY the Top {{max_select}} highest-quality candidates for deep multi-agent analysis.

MANDATORY REJECTION FILTERS:
1. Imminent binary events (earnings < 48h, CPI/NFP/rate decision < 30m, suspension, IDX FCA/UMA).
2. Severe negative catalysts (dilutive rights issue, fraud/lawsuit, delisting, protocol hack).
3. Overextended chases (RSI > 78 with no nearby support) or extreme crowded crypto funding.
4. Concentration: avoid picking all candidates from the exact same `correlation_group` unless overwhelmingly justified.

Output ONLY raw valid JSON without markdown fences or extra prose."""


def _build_prompt(req: ScoutTriageRequest) -> str:
    regime = req.market_regime if isinstance(req.market_regime, dict) else req.market_regime.model_dump(mode="json")
    candidates = [c.model_dump(mode="json") for c in req.candidates]
    return (
        f"Asset Class: {req.asset_class.value}\n"
        f"Strategy Focus: {req.strategy_tag}\n"
        f"Current Timestamp (WIB): {req.timestamp_wib}\n"
        f"Max Candidates to Select: {req.max_select}\n"
        f"Intermarket Regime: {json.dumps(regime, ensure_ascii=False)}\n\n"
        f"CANDIDATES BATCH:\n{json.dumps(candidates, ensure_ascii=False)}\n\n"
        "EXACT JSON SCHEMA:\n"
        "{\n"
        '  "selected_candidates": [\n'
        "    {\n"
        '      "ticker": "string",\n'
        '      "triage_rank": 1,\n'
        '      "priority_score": 0.85,\n'
        '      "triage_notes": "Alasan singkat 1-2 kalimat dalam Bahasa Indonesia."\n'
        "    }\n"
        "  ],\n"
        '  "rejected_summary": [\n'
        "    {\n"
        '      "ticker": "string",\n'
        '      "reason": "Alasan singkat penolakan dalam Bahasa Indonesia."\n'
        "    }\n"
        "  ]\n"
        "}\n"
    )


async def triage_candidates(req: ScoutTriageRequest, router: LLMRouter | None = None) -> ScoutTriageOutput:
    router = router or LLMRouter()
    system = _SYSTEM.replace("{{max_select}}", str(req.max_select))
    prompt = _build_prompt(req)

    async def _repair(prompt_text: str, _malformed: str) -> str:
        resp = await router.quick_complete(prompt_text, system="Return only corrected raw JSON.")
        return resp.text

    try:
        resp = await router.complete(
            [{"role": "system", "content": system}, {"role": "user", "content": prompt}],
            temperature=0.0,
            max_tokens=1200,
        )
        return await repair_and_validate(resp.text, ScoutTriageOutput, _repair)
    except Exception:
        # Safe fallback: select none rather than hallucinate / 500 on missing provider
        return ScoutTriageOutput(selected_candidates=[], rejected_summary=[])
