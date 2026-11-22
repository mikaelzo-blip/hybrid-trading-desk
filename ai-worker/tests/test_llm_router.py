import pytest
from openai import RateLimitError

from app.config import Settings
from app.llm_router import LLMRouter


class FakeCompletions:
    def __init__(self, behavior):
        self.behavior = behavior
        self.calls = 0

    async def create(self, **kwargs):
        self.calls += 1
        return await self.behavior(kwargs)


class FakeChat:
    def __init__(self, behavior):
        self.completions = FakeCompletions(behavior)


class FakeClient:
    def __init__(self, behavior):
        self.chat = FakeChat(behavior)


@pytest.mark.asyncio
async def test_llm_router_falls_back_on_429(monkeypatch):
    monkeypatch.setenv("LLM_PRIMARY_PROVIDER", "opencode_zen")
    monkeypatch.setenv("LLM_FALLBACK_PROVIDERS", "opencode_go")
    monkeypatch.setenv("OPENCODE_ZEN_API_KEY", "test")
    monkeypatch.setenv("OPENCODE_ZEN_BASE_URL", "https://zen.example/v1")
    monkeypatch.setenv("OPENCODE_ZEN_MODEL", "zen-model")
    monkeypatch.setenv("OPENCODE_GO_API_KEY", "test")
    monkeypatch.setenv("OPENCODE_GO_BASE_URL", "https://go.example/v1")
    monkeypatch.setenv("OPENCODE_GO_MODEL", "go-model")
    monkeypatch.setenv("FREE_TIER_MODE", "true")
    monkeypatch.setenv("LLM_MAX_CONCURRENCY", "1")
    monkeypatch.setenv("LLM_NODE_DELAY_MS", "0")

    settings = Settings()
    router = LLMRouter(settings)

    async def zen_behavior(kwargs):
        req = httpx.Request("POST", "https://zen.example/v1/chat/completions")
        resp = httpx.Response(429, request=req, json={"error": {"message": "rate limited"}})
        raise RateLimitError("rate limited", response=resp, body=resp.json())

    async def go_behavior(kwargs):
        class Msg:
            content = '{"action":"HOLD","confidence":0.0,"entry_price":"0","stop_loss":"0","take_profit":"0","reasoning_summary":"ok"}'
        class Choice:
            message = Msg()
        class Resp:
            choices = [Choice()]
        return Resp()

    zen_client = FakeClient(zen_behavior)
    go_client = FakeClient(go_behavior)

    def fake_client_for(provider):
        return zen_client if provider.name == "opencode_zen" else go_client

    monkeypatch.setattr(router, "_client_for", fake_client_for)

    out = await router.complete([{"role": "user", "content": "hi"}])
    assert out.provider == "opencode_go"
    assert zen_client.chat.completions.calls == 1
    assert go_client.chat.completions.calls == 1


import httpx  # noqa: E402
