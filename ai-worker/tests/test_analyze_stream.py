import json

import pytest
from httpx import ASGITransport, AsyncClient

from app.main import app
from app.llm_router import LLMResponse


class FakeLLM:
    def __init__(self):
        self.calls = 0

    async def complete(self, messages, temperature=0.2, max_tokens=800):
        self.calls += 1
        user = messages[-1]["content"]
        if "ROLE: MARKET analyst" in user or "ROLE: NEWS analyst" in user or "ROLE: SOCIAL analyst" in user or "ROLE: FUNDAMENTALS analyst" in user:
            return LLMResponse(text="- analisis singkat", provider="opencode_zen", model="zen")
        if "ROLE: Bull debater" in user:
            return LLMResponse(text="Bull case", provider="opencode_zen", model="zen")
        if "ROLE: Bear debater" in user:
            return LLMResponse(text="Bear case", provider="opencode_zen", model="zen")
        # final
        return LLMResponse(
            text=json.dumps({
                "action": "BUY",
                "confidence": 0.72,
                "entry_price": "100.5",
                "stop_loss": "99.0",
                "take_profit": "103.5",
                "reasoning_summary": "Setup bullish dengan konfirmasi tren."
            }),
            provider="opencode_zen",
            model="zen",
        )

    async def quick_complete(self, prompt, system="You are a precise assistant. Return only the requested output."):
        return LLMResponse(text=prompt, provider="opencode_zen", model="zen")


@pytest.mark.asyncio
async def test_analyze_stream_emits_expected_events(monkeypatch):
    monkeypatch.setenv("LLM_PRIMARY_PROVIDER", "opencode_zen")
    monkeypatch.setenv("OPENCODE_ZEN_API_KEY", "test")
    monkeypatch.setenv("OPENCODE_ZEN_BASE_URL", "https://zen.example/v1")
    monkeypatch.setenv("OPENCODE_ZEN_MODEL", "zen")

    from app.config import get_settings
    get_settings.cache_clear()

    import app.trading_agents_runner as runner_mod

    monkeypatch.setattr(runner_mod, "LLMRouter", lambda settings=None: FakeLLM())

    payload = {
        "asset_class": "crypto",
        "ticker": "BTCUSDT",
        "timeframe": "15m",
        "exec_tf_features": {
            "asset_class": "crypto",
            "ticker": "BTCUSDT",
            "timeframe": "15m",
            "close": "100.0",
            "atr_14": "1.0"
        },
        "market_regime": {"asset_class": "crypto", "regime_label": "risk_on", "volatility_regime": "normal", "bias": "bullish", "summary": "ok", "details": {}},
        "event_risk": {"asset_class": "crypto", "blocked_entry": False, "severity": "none", "reasons": [], "details": {}},
        "enable_vision": False
    }

    events = []
    current_event = ""
    async with AsyncClient(transport=ASGITransport(app=app), base_url="http://test") as ac:
        async with ac.stream("POST", "/analyze/stream", json=payload) as resp:
            assert resp.status_code == 200
            async for line in resp.aiter_lines():
                if line.startswith("event: "):
                    current_event = line.split(": ", 1)[1]
                elif line.startswith("data: "):
                    data = json.loads(line.split(": ", 1)[1])
                    events.append((current_event, data))

    names = [e for e, _ in events]
    assert "router" in names
    assert "analyst" in names
    assert "debate" in names
    assert "final" in names
    final = [d for e, d in events if e == "final"][-1]
    assert final["signal"]["action"] == "BUY"
