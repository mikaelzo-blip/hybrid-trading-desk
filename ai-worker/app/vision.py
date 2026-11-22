from __future__ import annotations

import base64
import io
import os
from typing import Any

os.environ.setdefault("MPLBACKEND", "Agg")

try:
    import matplotlib
    matplotlib.use("Agg", force=True)
    import matplotlib.pyplot as plt  # noqa: E402
    import mplfinance as mpf  # noqa: E402
    import pandas as pd  # noqa: E402
    _VISION_DEPS_OK = True
except Exception:  # noqa: BLE001
    plt = None  # type: ignore[assignment]
    mpf = None  # type: ignore[assignment]
    pd = None  # type: ignore[assignment]
    _VISION_DEPS_OK = False

from app.config import Settings, get_settings  # noqa: E402
from app.json_repair import repair_and_validate  # noqa: E402
from app.llm_router import LLMRouter  # noqa: E402
from app.schemas import VisionAnalysisOutput  # noqa: E402


def capability_check(settings: Settings | None = None) -> tuple[bool, str]:
    s = settings or get_settings()
    if not s.enable_chart_vision:
        return False, "ENABLE_CHART_VISION=false"
    if not s.vision_model:
        return False, "VISION_MODEL kosong"
    return True, "ok"


def render_candlestick_png(candles: list[dict[str, Any]], ticker: str, timeframe: str) -> bytes:
    if not _VISION_DEPS_OK:
        raise RuntimeError("vision dependencies not installed")
    rows = []
    for c in candles[-120:]:
        rows.append({
            "Date": pd.to_datetime(c.get("open_time") or c.get("time") or c.get("timestamp")),
            "Open": float(c.get("open", 0)),
            "High": float(c.get("high", 0)),
            "Low": float(c.get("low", 0)),
            "Close": float(c.get("close", 0)),
            "Volume": float(c.get("volume", 0)),
        })
    if len(rows) < 20:
        raise ValueError("not enough candles for vision")
    df = pd.DataFrame(rows).set_index("Date")
    df = df.tail(120)

    addplots = [
        mpf.make_addplot(df["Close"].ewm(span=20, adjust=False).mean(), color="#22d3ee", width=0.8),
        mpf.make_addplot(df["Close"].ewm(span=50, adjust=False).mean(), color="#f59e0b", width=0.8),
    ]
    buf = io.BytesIO()
    mpf.plot(
        df,
        type="candle",
        style="charles",
        volume=True,
        addplot=addplots,
        title=f"{ticker} {timeframe}",
        tight_layout=True,
        figratio=(16, 9),
        figscale=0.8,
        savefig=dict(fname=buf, dpi=100, format="png", bbox_inches="tight"),
        closefig=True,
    )
    plt.close("all")
    data = buf.getvalue()
    # Compress if too large by reducing dpi/size already small; ensure < 500KB
    if len(data) > 500_000:
        buf2 = io.BytesIO()
        mpf.plot(
            df.tail(80),
            type="candle",
            style="charles",
            volume=True,
            addplot=[
                mpf.make_addplot(df.tail(80)["Close"].ewm(span=20, adjust=False).mean(), color="#22d3ee", width=0.7),
                mpf.make_addplot(df.tail(80)["Close"].ewm(span=50, adjust=False).mean(), color="#f59e0b", width=0.7),
            ],
            title=f"{ticker} {timeframe}",
            tight_layout=True,
            figratio=(16, 9),
            figscale=0.6,
            savefig=dict(fname=buf2, dpi=72, format="png", bbox_inches="tight"),
            closefig=True,
        )
        plt.close("all")
        data = buf2.getvalue()
    return data


async def analyze_chart_vision(
    *,
    candles: list[dict[str, Any]],
    ticker: str,
    timeframe: str,
    context_json: str,
    router: LLMRouter | None = None,
    settings: Settings | None = None,
) -> dict[str, Any]:
    s = settings or get_settings()
    ok, reason = capability_check(s)
    if not ok:
        return {"status": "skipped_unsupported", "reason": reason}

    router = router or LLMRouter(s)
    try:
        png = render_candlestick_png(candles, ticker, timeframe)
    except Exception as e:  # noqa: BLE001
        return {"status": "skipped_unsupported", "reason": f"chart render failed: {e}"}

    b64 = base64.b64encode(png).decode("ascii")
    data_url = f"data:image/png;base64,{b64}"
    system = (
        "You are a Chart Vision Analyst. Analyze the candlestick chart image and context. "
        "Return ONLY raw valid JSON matching the schema."
    )
    user_text = (
        f"Ticker: {ticker}\nTimeframe: {timeframe}\nContext JSON:\n{context_json}\n\n"
        "EXACT JSON SCHEMA:\n"
        "{\n"
        '  "visual_trend": "strong_uptrend|weak_uptrend|ranging|weak_downtrend|strong_downtrend",\n'
        '  "chart_patterns": ["string"],\n'
        '  "key_zones": {"support": "...", "resistance": "..."},\n'
        '  "price_action_bias": "bullish|neutral|bearish",\n'
        '  "trap_warnings": ["string"],\n'
        '  "visual_summary": "Ringkasan 2-3 kalimat dalam Bahasa Indonesia"\n'
        "}"
    )
    messages = [
        {"role": "system", "content": system},
        {
            "role": "user",
            "content": [
                {"type": "text", "text": user_text},
                {"type": "image_url", "image_url": {"url": data_url}},
            ],
        },
    ]

    async def _repair(prompt: str, _malformed: str) -> str:
        r = await router.quick_complete(prompt, system="Return only corrected raw JSON.")
        return r.text

    try:
        resp = await router.complete(messages, temperature=0.0, max_tokens=700)
        out = await repair_and_validate(resp.text, VisionAnalysisOutput, _repair)
        return {"status": "completed", "provider": resp.provider, "model": resp.model, "analysis": out.model_dump()}
    except Exception as e:  # noqa: BLE001
        err = str(e).lower()
        if "429" in err or "rate" in err or "unsupported" in err or "image" in err:
            return {"status": "skipped_unsupported", "reason": str(e)}
        return {"status": "skipped_unsupported", "reason": f"vision parse/call failed: {e}"}
