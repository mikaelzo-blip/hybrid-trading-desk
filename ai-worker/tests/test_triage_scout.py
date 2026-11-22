import pytest

from app.llm_router import LLMResponse
from app.schemas import ScoutTriageRequest
from app.triage_scout import triage_candidates


class FakeRouter:
    async def complete(self, messages, *, temperature=0.2, max_tokens=800):
        return LLMResponse(
            text='{"selected_candidates":[{"ticker":"BTCUSDT","triage_rank":1,"priority_score":0.9,"triage_notes":"Momentum kuat."}],"rejected_summary":[{"ticker":"ETHUSDT","reason":"Terlalu berkorelasi."}]}',
            provider="test",
            model="test-model",
        )

    async def quick_complete(self, prompt, *, system=""):
        return LLMResponse(text=prompt, provider="test", model="test-model")


@pytest.mark.asyncio
async def test_triage_candidates_parses_valid_json():
    req = ScoutTriageRequest(
        asset_class="crypto",
        strategy_tag="momentum_breakout",
        timestamp_wib="2026-01-01T09:00:00+07:00",
        max_select=3,
        market_regime={"asset_class": "crypto", "regime_label": "risk_on", "volatility_regime": "normal", "bias": "bullish", "summary": "ok"},
        candidates=[{"ticker": "BTCUSDT", "asset_class": "crypto"}, {"ticker": "ETHUSDT", "asset_class": "crypto"}],
    )
    out = await triage_candidates(req, router=FakeRouter())
    assert len(out.selected_candidates) == 1
    assert out.selected_candidates[0].ticker == "BTCUSDT"
    assert out.rejected_summary[0].ticker == "ETHUSDT"
