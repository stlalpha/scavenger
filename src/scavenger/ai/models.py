from __future__ import annotations
from pydantic import BaseModel


class AIEvaluation(BaseModel):
    relevant: bool
    reason: str
    notable: str | None
    escalate: bool

    @classmethod
    def passthrough(cls) -> AIEvaluation:
        """Safe fallback — never drop a listing due to model error."""
        return cls(relevant=True, reason="", notable=None, escalate=False)


class AIConfig(BaseModel):
    enabled: bool = False
    litellm_base_url: str = "http://localhost:4000"
    filter_model: str = "ollama/qwen2.5:7b"
    escalation_model: str = "anthropic/claude-haiku-4-5"
    escalation_enabled: bool = False
    escalation_min_keyword_score: float = 70.0
    api_key: str = "noop"
    filter_timeout_sec: float = 10.0
    escalation_timeout_sec: float = 15.0
