import pytest

from app.json_repair import extract_json_object, parse_and_validate, repair_and_validate, repair_or_hold, strip_think_tags
from app.schemas import AgentSignalOutput, ScoutTriageOutput


def test_strip_think_tags_removes_multiline_block():
    raw = "<think>internal reasoning</think>{\"a\":1}"
    assert strip_think_tags(raw) == "{\"a\":1}"


def test_extract_json_from_fenced_output():
    raw = """Here you go
```json
{"action":"HOLD","confidence":0.2,"entry_price":"0","stop_loss":"0","take_profit":"0","reasoning_summary":"ok"}
```"""
    obj = extract_json_object(raw)
    assert obj is not None
    assert '"action":"HOLD"' in obj


def test_parse_and_validate_agent_signal():
    raw = '{"action":"BUY","confidence":0.7,"entry_price":"100.5","stop_loss":"99","take_profit":"103","reasoning_summary":"test"}'
    out = parse_and_validate(raw, AgentSignalOutput)
    assert out.action.value == "BUY"
    assert out.entry_price == "100.5"


@pytest.mark.asyncio
async def test_repair_and_validate_uses_one_repair_call():
    calls = {"n": 0}

    async def repair_call(prompt: str, malformed: str) -> str:
        calls["n"] += 1
        return '{"action":"HOLD","confidence":0.0,"entry_price":"0","stop_loss":"0","take_profit":"0","reasoning_summary":"fixed"}'

    bad = "<think>oops</think>not json"
    out = await repair_and_validate(bad, AgentSignalOutput, repair_call=repair_call)
    assert calls["n"] == 1
    assert out.action.value == "HOLD"


@pytest.mark.asyncio
async def test_repair_or_hold_falls_back_safely():
    async def bad_repair(prompt: str, malformed: str) -> str:
        return "still bad"

    out = await repair_or_hold("<<<>>>", repair_call=bad_repair)
    assert out.action.value == "HOLD"
    assert out.confidence == 0.0
    assert out.entry_price == "0"


def test_parse_scout_triage_output():
    raw = '{"selected_candidates":[{"ticker":"BTCUSDT","triage_rank":1,"priority_score":0.9,"triage_notes":"ok"}],"rejected_summary":[]}'
    out = parse_and_validate(raw, ScoutTriageOutput)
    assert out.selected_candidates[0].ticker == "BTCUSDT"
