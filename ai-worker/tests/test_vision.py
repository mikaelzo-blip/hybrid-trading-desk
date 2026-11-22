import base64

import pytest

pytest.importorskip("mplfinance")
pytest.importorskip("pandas")

from app.llm_router import LLMResponse
from app.vision import analyze_chart_vision, capability_check, render_candlestick_png


class FakeSettings:
    enable_chart_vision = True
    vision_model = "test-vision-model"


class DisabledSettings:
    enable_chart_vision = False
    vision_model = None


class FakeRouter429:
    async def complete(self, messages, *, temperature=0.2, max_tokens=800):
        raise Exception("429 rate limit")

    async def quick_complete(self, prompt, *, system=""):
        return LLMResponse(text="{}", provider="test", model="test")


def _sample_candles(n=100):
    out = []
    price = 100.0
    for i in range(n):
        o = price
        c = price + (0.5 if i % 2 == 0 else -0.2)
        h = max(o, c) + 0.3
        l = min(o, c) - 0.3
        out.append({
            "open_time": f"2026-01-01T{i:02d}:00:00Z" if i < 24 else f"2026-01-02T{i-24:02d}:00:00Z",
            "open": o, "high": h, "low": l, "close": c, "volume": 1000 + i,
        })
        price = c
    return out


def test_render_candlestick_png_under_500kb():
    png = render_candlestick_png(_sample_candles(100), "BTCUSDT", "1h")
    assert png.startswith(b"\x89PNG")
    assert len(png) < 500_000


@pytest.mark.asyncio
async def test_vision_graceful_degradation_when_model_missing():
    res = await analyze_chart_vision(
        candles=_sample_candles(100),
        ticker="BTCUSDT",
        timeframe="1h",
        context_json="{}",
        router=FakeRouter429(),
        settings=DisabledSettings(),
    )
    assert res["status"] == "skipped_unsupported"


@pytest.mark.asyncio
async def test_vision_graceful_degradation_on_429():
    res = await analyze_chart_vision(
        candles=_sample_candles(100),
        ticker="BTCUSDT",
        timeframe="1h",
        context_json="{}",
        router=FakeRouter429(),
        settings=FakeSettings(),
    )
    assert res["status"] == "skipped_unsupported"
