import pytest
from scavenger.ai.models import AIEvaluation, AIConfig


def test_ai_evaluation_defaults():
    ev = AIEvaluation(relevant=True, reason="Good match", notable=None, escalate=False)
    assert ev.relevant is True
    assert ev.notable is None
    assert ev.escalate is False


def test_ai_evaluation_passthrough():
    """The safe fallback when model output is unusable."""
    ev = AIEvaluation.passthrough()
    assert ev.relevant is True
    assert ev.reason == ""
    assert ev.notable is None
    assert ev.escalate is False


def test_ai_evaluation_roundtrip_json():
    ev = AIEvaluation(relevant=True, reason="Nice lens", notable="Zeiss variant", escalate=True)
    restored = AIEvaluation.model_validate_json(ev.model_dump_json())
    assert restored == ev


def test_ai_config_defaults():
    config = AIConfig()
    assert config.enabled is False
    assert config.filter_model == "qwen3.5:9b"
    assert config.escalation_enabled is False
    assert config.escalation_min_keyword_score == 70.0


def test_ai_config_custom():
    config = AIConfig(
        enabled=True,
        litellm_base_url="http://192.168.1.10:4000",
        filter_model="ollama/llama3.2:3b",
        escalation_enabled=True,
        escalation_model="anthropic/claude-haiku-4-5",
    )
    assert config.enabled is True
    assert "192.168.1.10" in config.litellm_base_url


def test_listing_has_ai_evaluation_field():
    from datetime import datetime, timezone
    from scavenger.models import Listing
    now = datetime.now(timezone.utc)
    listing = Listing(
        id="abc", profile_id="p1", source_id="ebay",
        title="Sony lens", url="https://ebay.com/1",
        first_seen=now, last_seen=now, relevance_score=80.0,
    )
    assert listing.ai_evaluation is None
