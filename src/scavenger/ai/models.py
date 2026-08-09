from __future__ import annotations
import logging
import os
import subprocess
from pydantic import BaseModel, model_validator

logger = logging.getLogger(__name__)


def _sops_extract_key(key: str) -> str:
    """Decrypt one key from the sops-encrypted secrets file.

    Shells out to the user's ``sops`` binary so their real key
    infrastructure (age, PGP, KMS) is honored. Returns "" when the file is
    absent, sops is unavailable, or decryption fails — the missing key is
    surfaced through evaluator behavior, never a crash.
    """
    secrets = os.environ.get(
        "SCAVENGER_SECRETS_FILE",
        os.path.expanduser("~/.config/scavenger/secrets.sops.yaml"),
    )
    if not os.path.isfile(secrets):
        return ""
    try:
        out = subprocess.run(
            ["sops", "decrypt", "--extract", f'["{key}"]', secrets],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        logger.warning("sops not runnable (%s) — cannot read %s", e, secrets)
        return ""
    if out.returncode != 0:
        logger.warning(
            "sops failed to decrypt %s: %s", secrets, out.stderr.strip()
        )
        return ""
    return out.stdout.strip()


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
        """Secrets are sops-only: the ANTHROPIC_API_KEY env var (as injected
        by ``sops exec-env``) or the sops-encrypted secrets file. A legacy
        plaintext ~/.config/scavenger/.env is ignored with a loud warning.
        """
        if self.anthropic_api_key:
            logger.warning(
                "anthropic_api_key is set in plaintext in config.toml — move it"
                " to ~/.config/scavenger/secrets.sops.yaml (sops-encrypted)"
            )
        if not self.anthropic_api_key:
            self.anthropic_api_key = os.environ.get("ANTHROPIC_API_KEY", "")
        if not self.anthropic_api_key:
            self.anthropic_api_key = _sops_extract_key("anthropic_api_key")
        legacy = os.path.expanduser("~/.config/scavenger/.env")
        if os.path.isfile(legacy):
            logger.warning(
                "plaintext %s is IGNORED — secrets are sops-only now; move the"
                " key into ~/.config/scavenger/secrets.sops.yaml (`sops edit`"
                " it) and delete the .env file",
                legacy,
            )
        return self
