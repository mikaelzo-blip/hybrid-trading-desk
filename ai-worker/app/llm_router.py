from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Any

import httpx
from openai import APIConnectionError, APIStatusError, APITimeoutError, AsyncOpenAI, RateLimitError

from app.config import ProviderConfig, Settings, get_settings


@dataclass(slots=True)
class LLMResponse:
    text: str
    provider: str
    model: str


class LLMRouter:
    def __init__(self, settings: Settings | None = None) -> None:
        self.settings = settings or get_settings()
        self._semaphore = asyncio.Semaphore(self.settings.llm_max_concurrency)
        self._last_call_ts: float | None = None

    def _client_for(self, provider: ProviderConfig) -> AsyncOpenAI:
        kwargs: dict[str, Any] = {
            "api_key": provider.api_key or "not-needed",
            "timeout": self.settings.llm_request_timeout_secs,
        }
        if provider.base_url:
            kwargs["base_url"] = provider.base_url
        return AsyncOpenAI(**kwargs)

    async def _pace(self) -> None:
        delay_ms = self.settings.llm_node_delay_ms
        if delay_ms <= 0:
            return
        now = asyncio.get_running_loop().time()
        if self._last_call_ts is not None:
            elapsed_ms = (now - self._last_call_ts) * 1000
            wait_ms = delay_ms - elapsed_ms
            if wait_ms > 0:
                await asyncio.sleep(wait_ms / 1000)
        self._last_call_ts = asyncio.get_running_loop().time()

    @staticmethod
    def _is_fallback_error(err: Exception) -> bool:
        if isinstance(err, (RateLimitError, APITimeoutError, APIConnectionError, asyncio.TimeoutError, httpx.TimeoutException)):
            return True
        if isinstance(err, APIStatusError):
            return err.status_code == 429 or err.status_code >= 500
        # some wrappers expose status_code
        status = getattr(err, "status_code", None)
        if isinstance(status, int) and (status == 429 or status >= 500):
            return True
        return False

    async def complete(
        self,
        messages: list[dict[str, str]],
        *,
        temperature: float = 0.2,
        max_tokens: int = 800,
    ) -> LLMResponse:
        chain = self.settings.provider_chain()
        if not chain:
            raise RuntimeError("No enabled LLM providers configured")

        last_err: Exception | None = None
        for provider in chain:
            try:
                async with self._semaphore:
                    await self._pace()
                    client = self._client_for(provider)
                    resp = await client.chat.completions.create(
                        model=provider.model or "",
                        messages=messages,
                        temperature=temperature,
                        max_tokens=max_tokens,
                    )
                text = ""
                if resp.choices:
                    msg = resp.choices[0].message
                    text = msg.content or ""
                return LLMResponse(text=text, provider=provider.name, model=provider.model or "")
            except Exception as err:  # noqa: BLE001
                last_err = err
                if self._is_fallback_error(err):
                    continue
                raise
        assert last_err is not None
        raise last_err

    async def quick_complete(
        self,
        prompt: str,
        *,
        system: str = "You are a precise assistant. Return only the requested output.",
    ) -> LLMResponse:
        return await self.complete(
            [{"role": "system", "content": system}, {"role": "user", "content": prompt}],
            temperature=0.0,
            max_tokens=800,
        )
