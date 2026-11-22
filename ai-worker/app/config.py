from __future__ import annotations

import os
from dataclasses import dataclass, field
from functools import lru_cache

from dotenv import load_dotenv

load_dotenv()


def _bool(name: str, default: bool = False) -> bool:
    raw = os.getenv(name)
    if raw is None:
        return default
    return raw.strip().lower() in {"1", "true", "yes", "on"}


def _int(name: str, default: int) -> int:
    raw = os.getenv(name)
    if raw is None or not raw.strip():
        return default
    try:
        return int(raw)
    except ValueError:
        return default


def _csv(name: str, default: list[str] | None = None) -> list[str]:
    raw = os.getenv(name)
    if raw is None or not raw.strip():
        return list(default or [])
    return [part.strip() for part in raw.split(",") if part.strip()]


@dataclass(slots=True)
class ProviderConfig:
    name: str
    base_url: str | None
    api_key: str | None
    model: str | None

    @property
    def enabled(self) -> bool:
        # ollama may not need api key; others need model and key/base depending on provider
        if not self.model:
            return False
        if self.name == "ollama":
            return bool(self.base_url)
        return bool(self.api_key or self.base_url)


@dataclass(slots=True)
class Settings:
    app_env: str = os.getenv("APP_ENV", "development")
    log_level: str = os.getenv("LOG_LEVEL", "INFO")
    ai_worker_port: int = _int("AI_WORKER_PORT", 8000)
    rust_core_base_url: str = os.getenv("RUST_CORE_BASE_URL", "http://localhost:8080")

    llm_primary_provider: str = os.getenv("LLM_PRIMARY_PROVIDER", "opencode_zen")
    llm_fallback_providers: list[str] = field(
        default_factory=lambda: _csv("LLM_FALLBACK_PROVIDERS", ["opencode_go"])
    )
    free_tier_mode: bool = _bool("FREE_TIER_MODE", True)
    llm_max_concurrency: int = max(1, _int("LLM_MAX_CONCURRENCY", 1))
    llm_node_delay_ms: int = max(0, _int("LLM_NODE_DELAY_MS", 1500))
    llm_request_timeout_secs: int = max(1, _int("LLM_REQUEST_TIMEOUT_SECS", 60))

    vision_model: str | None = os.getenv("VISION_MODEL") or None
    enable_chart_vision: bool = _bool("ENABLE_CHART_VISION", False)

    scout_max_deep_dive: int = max(1, _int("SCOUT_MAX_DEEP_DIVE", 3))
    max_debate_rounds: int = max(1, _int("MAX_DEBATE_ROUNDS", 1))

    def provider(self, name: str) -> ProviderConfig:
        n = name.strip().lower()
        mapping = {
            "opencode_zen": ProviderConfig(
                name="opencode_zen",
                base_url=os.getenv("OPENCODE_ZEN_BASE_URL"),
                api_key=os.getenv("OPENCODE_ZEN_API_KEY"),
                model=os.getenv("OPENCODE_ZEN_MODEL"),
            ),
            "opencode_go": ProviderConfig(
                name="opencode_go",
                base_url=os.getenv("OPENCODE_GO_BASE_URL"),
                api_key=os.getenv("OPENCODE_GO_API_KEY"),
                model=os.getenv("OPENCODE_GO_MODEL"),
            ),
            "gemini_free": ProviderConfig(
                name="gemini_free",
                base_url=os.getenv("GEMINI_BASE_URL"),
                api_key=os.getenv("GEMINI_API_KEY"),
                model=os.getenv("GEMINI_MODEL"),
            ),
            "groq_free": ProviderConfig(
                name="groq_free",
                base_url=os.getenv("GROQ_BASE_URL", "https://api.groq.com/openai/v1"),
                api_key=os.getenv("GROQ_API_KEY"),
                model=os.getenv("GROQ_MODEL"),
            ),
            "openrouter_free": ProviderConfig(
                name="openrouter_free",
                base_url=os.getenv("OPENROUTER_BASE_URL", "https://openrouter.ai/api/v1"),
                api_key=os.getenv("OPENROUTER_API_KEY"),
                model=os.getenv("OPENROUTER_MODEL"),
            ),
            "ollama": ProviderConfig(
                name="ollama",
                base_url=os.getenv("OLLAMA_BASE_URL", "http://localhost:11434/v1"),
                api_key=os.getenv("OLLAMA_API_KEY", "ollama"),
                model=os.getenv("OLLAMA_MODEL"),
            ),
        }
        return mapping.get(n, ProviderConfig(name=n, base_url=None, api_key=None, model=None))

    def provider_chain(self) -> list[ProviderConfig]:
        names = [self.llm_primary_provider, *self.llm_fallback_providers]
        seen: set[str] = set()
        chain: list[ProviderConfig] = []
        for name in names:
            n = name.strip().lower()
            if not n or n in seen:
                continue
            seen.add(n)
            cfg = self.provider(n)
            if cfg.enabled:
                chain.append(cfg)
        return chain


@lru_cache(maxsize=1)
def get_settings() -> Settings:
    return Settings()
