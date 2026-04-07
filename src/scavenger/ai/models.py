from __future__ import annotations
import os
from pydantic import BaseModel, model_validator


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
    # Local model (Ollama) for cheap filter pass
    litellm_base_url: str = "http://localhost:11434/v1"
    filter_model: str = "qwen3.5:9b"
    filter_timeout_sec: float = 30.0
    # Frontier model (Anthropic) for escalation + suggestions
    escalation_model: str = "claude-haiku-4-5-20251001"
    escalation_enabled: bool = False
    escalation_min_keyword_score: float = 70.0
    escalation_timeout_sec: float = 30.0
    anthropic_api_key: str = ""
    api_key: str = "noop"  # legacy, unused

    @model_validator(mode="after")
    def _resolve_env_keys(self) -> "AIConfig":
        if not self.anthropic_api_key:
            self.anthropic_api_key = os.environ.get("ANTHROPIC_API_KEY", "")
        if not self.anthropic_api_key:
            dotenv = os.path.expanduser("~/.config/scavenger/.env")
            if os.path.isfile(dotenv):
                try:
                    with open(dotenv) as f:
                        for line in f:
                            line = line.strip()
                            if line.startswith("ANTHROPIC_API_KEY=") and not line.startswith("#"):
                                val = line.split("=", 1)[1].strip()
                                if val:
                                    self.anthropic_api_key = val
                                    break
                except OSError:
                    pass
        return self
