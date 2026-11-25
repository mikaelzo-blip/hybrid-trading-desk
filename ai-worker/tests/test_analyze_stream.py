import json

import pytest
from httpx import ASGITransport, AsyncClient

from app.main import app
from app.llm_router import LLMResponse


class FakeLLM:
    def __init__(self, initial_final_text=None):
        self.initial_final_text = initial_final_text
        self.repair_attempts = 0

    async def complete(self, messages, temperature=0.2, max_tokens=800):
        user = messages[-1]["content"]
        if "ROLE: MARKET analyst" in user or "ROLE: NEWS analyst" in user or "ROLE: SOCIAL analyst" in user or "ROLE: FUNDAMENTALS analyst" in user:
            return LLMResponse(text="- analisis singkat", provider="opencode_zen", model="zen")
        if "ROLE: Bull debater" in user:
            return LLMResponse(text="Bull case", provider="opencode_zen", model="zen")
        if "ROLE: Bear debater" in user:
            return LLMResponse(text="Bear case", provider="opencode_zen", model="zen")
        # final
        if self.initial_final_text is not None:
            if len(messages) == 2:
                return LLMResponse(text=self.initial_final_text, provider="opencode_zen", model="zen")
            self.repair_attempts += 1
            assert "Ticker: BTCUSDT" in messages[1]["content"]
            assert '"close": "100.0"' in messages[1]["content"]
            assert 'Required JSON schema:' in user and 'AgentSignalOutput' in user
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


@pytest.mark.asyncio
@pytest.mark.parametrize("initial_final_text", [None, "", '{"action":"BUY"'])
async def test_analyze_stream_emits_expected_events(monkeypatch, initial_final_text):
    monkeypatch.setenv("LLM_PRIMARY_PROVIDER", "opencode_zen")
    monkeypatch.setenv("OPENCODE_ZEN_API_KEY", "test")
    monkeypatch.setenv("OPENCODE_ZEN_BASE_URL", "https://zen.example/v1")
    monkeypatch.setenv("OPENCODE_ZEN_MODEL", "zen")

    from app.config import get_settings
    get_settings.cache_clear()

    import app.trading_agents_runner as runner_mod
    from app.config import Settings

    llm = FakeLLM(initial_final_text)
    monkeypatch.setattr(runner_mod, "get_settings", lambda: Settings(free_tier_mode=False))
    monkeypatch.setattr(runner_mod, "LLMRouter", lambda settings=None: llm)

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
    assert llm.repair_attempts == (0 if initial_final_text is None else 1)


@pytest.mark.asyncio
@pytest.mark.parametrize(
    ("blocked", "malformed", "calls", "action"),
    [(False, False, 1, "BUY"), (False, True, 1, "HOLD"), (True, False, 0, "HOLD")],
)
async def test_free_tier_budgets_one_call_and_preserves_event_risk(
    monkeypatch, blocked, malformed, calls, action
):
    import app.trading_agents_runner as runner
    from app.config import Settings
    from app.schemas import AnalyzeRequest

    monkeypatch.setattr(runner, "get_settings", lambda: Settings(free_tier_mode=True))

    class CountedLLM(FakeLLM):
        requests = 0

        async def complete(self, *args, **kwargs):
            self.requests += 1
            return await super().complete(*args, **kwargs)

    llm = CountedLLM(initial_final_text="invalid JSON" if malformed else None)
    req = AnalyzeRequest(
        asset_class="us_stock",
        ticker="AAPL",
        timeframe="15m",
        exec_tf_features={"close": "100.0"},
        market_regime={"bias": "bullish"},
        event_risk={
            "asset_class": "us_stock",
            "blocked_entry": blocked,
            "reasons": ["earnings"] if blocked else [],
        },
        enable_vision=True,
    )
    events = [event async for event in runner.run_analysis(req, llm)]

    assert llm.requests == calls
    assert events[-1]["event"] == "final"
    assert events[-1]["data"]["signal"]["action"] == action
    assert not any(e["event"] in {"analyst", "debate", "error"} for e in events)
    if not blocked:
        assert any(
            e["event"] == "vision" and e["data"]["status"] == "skipped_free_tier"
            for e in events
        )
