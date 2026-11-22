from __future__ import annotations

import json
import re
from typing import Awaitable, Callable, TypeVar

from pydantic import BaseModel, ValidationError

from app.schemas import AgentSignalOutput, hold_fallback

T = TypeVar("T", bound=BaseModel)

_THINK_RE = re.compile(r"<think>.*?</think>", re.IGNORECASE | re.DOTALL)
_FENCE_RE = re.compile(r"```(?:json)?\s*(.*?)```", re.IGNORECASE | re.DOTALL)


def strip_think_tags(text: str) -> str:
    return _THINK_RE.sub("", text or "")


def strip_markdown_fences(text: str) -> str:
    raw = text or ""
    m = _FENCE_RE.search(raw)
    if m:
        return m.group(1).strip()
    # also handle unclosed fence
    if "```" in raw:
        return raw.replace("```json", "").replace("```", "").strip()
    return raw.strip()


def extract_json_object(text: str) -> str | None:
    raw = strip_markdown_fences(strip_think_tags(text))
    start = raw.find("{")
    if start == -1:
        return None
    depth = 0
    in_str = False
    escape = False
    for i in range(start, len(raw)):
        ch = raw[i]
        if in_str:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif ch == '"':
                in_str = False
            continue
        if ch == '"':
            in_str = True
        elif ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return raw[start : i + 1]
    return None


def parse_and_validate(text: str, model: type[T]) -> T:
    candidate = extract_json_object(text)
    if candidate is None:
        raise ValueError("no JSON object found")
    data = json.loads(candidate)
    return model.model_validate(data)


async def repair_and_validate(
    raw_output: str,
    model: type[T],
    repair_call: Callable[[str, str], Awaitable[str]] | None = None,
) -> T:
    cleaned = strip_think_tags(raw_output)
    try:
        return parse_and_validate(cleaned, model)
    except (ValueError, json.JSONDecodeError, ValidationError) as err:
        if repair_call is None:
            raise
        prompt = (
            "Fix the following malformed output so it strictly matches the required JSON schema. "
            "Return ONLY the corrected raw JSON object, nothing else.\n"
            f"Validation error: {err}\n"
            f"Malformed output: {cleaned[:4000]}"
        )
        repaired = await repair_call(prompt, cleaned)
        return parse_and_validate(repaired, model)


async def repair_or_hold(
    raw_output: str,
    repair_call: Callable[[str, str], Awaitable[str]] | None = None,
) -> AgentSignalOutput:
    try:
        return await repair_and_validate(raw_output, AgentSignalOutput, repair_call)
    except Exception:
        return hold_fallback(
            "Output LLM gagal divalidasi secara skema; fallback aman ke HOLD."
        )
