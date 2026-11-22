from __future__ import annotations

import json
from collections.abc import AsyncIterator

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import StreamingResponse

from app.schemas import AnalyzeRequest, ScoutTriageRequest
from app.trading_agents_runner import run_analysis
from app.triage_scout import triage_candidates

app = FastAPI(title="Hybrid Trading Desk AI Worker", version="0.1.0")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


@app.get("/health")
async def health() -> dict[str, str]:
    return {"status": "ok"}


def _format_sse(event: str, data: dict) -> str:
    return f"event: {event}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n"


async def _stream(req: AnalyzeRequest) -> AsyncIterator[str]:
    async for ev in run_analysis(req):
        yield _format_sse(ev["event"], ev["data"])


@app.post("/analyze/stream")
async def analyze_stream(req: AnalyzeRequest) -> StreamingResponse:
    return StreamingResponse(_stream(req), media_type="text/event-stream")


@app.post("/scout/triage")
async def scout_triage(req: ScoutTriageRequest) -> dict:
    out = await triage_candidates(req)
    return out.model_dump(mode="json")
