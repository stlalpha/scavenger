import json
import pytest
import respx
import httpx
from pathlib import Path
from scavenger.ai.evaluator import NoopEvaluator, AIEvaluator
from scavenger.ai.models import AIConfig, AIEvaluation
from scavenger.models import Profile, Listing
from datetime import datetime, timezone

FIXTURES = Path(__file__).parent / "fixtures"
OLLAMA_URL = "http://localhost:11434/v1/chat/completions"


def make_profile() -> Profile:
    return Profile(
        id="sony", name="Sony A-mount Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken"],
        sources=["ebay"], price_min=50.0, price_max=800.0,
    )


def make_listing(**overrides) -> Listing:
    now = datetime.now(timezone.utc)
    return Listing(**{
        "id": "abc", "profile_id": "sony", "source_id": "ebay",
        "title": "Sony 85mm A-mount lens", "description": "Great condition",
        "price": 249.99, "url": "https://ebay.com/1",
        "first_seen": now, "last_seen": now, "relevance_score": 80.0,
        **overrides,
    })


def litellm_response(evaluation: dict) -> httpx.Response:
    return httpx.Response(200, json={
        "choices": [{"message": {"content": json.dumps(evaluation)}}]
    })


# --- NoopEvaluator ---

async def test_noop_evaluator_returns_passthrough():
    ev = await NoopEvaluator().evaluate(make_profile(), make_listing())
    assert ev.relevant is True
    assert ev.escalate is False


# --- AIEvaluator ---

@respx.mock
async def test_evaluator_parses_valid_response():
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    respx.post(OLLAMA_URL).mock(return_value=litellm_response(fixture))
    config = AIConfig(enabled=True)
    ev = await AIEvaluator(config).evaluate(make_profile(), make_listing())
    assert ev.relevant is True
    assert ev.notable is not None
    assert "Zeiss" in ev.notable


@respx.mock
async def test_evaluator_returns_passthrough_on_malformed_json():
    respx.post(OLLAMA_URL).mock(return_value=httpx.Response(200, json={
        "choices": [{"message": {"content": "not json at all"}}]
    }))
    config = AIConfig(enabled=True)
    ev = await AIEvaluator(config).evaluate(make_profile(), make_listing())
    assert ev.relevant is True  # passthrough — never drop on error


@respx.mock
async def test_evaluator_returns_passthrough_on_http_error():
    respx.post(OLLAMA_URL).mock(return_value=httpx.Response(503))
    config = AIConfig(enabled=True)
    ev = await AIEvaluator(config).evaluate(make_profile(), make_listing())
    assert ev.relevant is True


@respx.mock
async def test_evaluator_returns_passthrough_on_timeout():
    respx.post(OLLAMA_URL).mock(side_effect=httpx.TimeoutException("timeout"))
    config = AIConfig(enabled=True)
    ev = await AIEvaluator(config).evaluate(make_profile(), make_listing())
    assert ev.relevant is True


@respx.mock
async def test_evaluator_not_relevant_discards():
    respx.post(OLLAMA_URL).mock(return_value=litellm_response({
        "relevant": False, "reason": "Wrong mount", "notable": None, "escalate": False
    }))
    config = AIConfig(enabled=True)
    ev = await AIEvaluator(config).evaluate(make_profile(), make_listing())
    assert ev.relevant is False


@respx.mock
async def test_escalation_second_call_fires_when_conditions_met():
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    escalation_response = {**fixture, "escalate": True}
    respx.post(OLLAMA_URL).mock(side_effect=[
        litellm_response(fixture),
        litellm_response(escalation_response),
    ])
    config = AIConfig(enabled=True, escalation_enabled=True, escalation_min_keyword_score=70.0)
    ev = await AIEvaluator(config).evaluate(make_profile(), make_listing())
    assert ev.escalate is True


@respx.mock
async def test_escalation_preserves_filter_reason():
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    escalation_response = {"relevant": True, "reason": "Escalation reason", "notable": "Even more notable", "escalate": True}
    respx.post(OLLAMA_URL).mock(side_effect=[
        litellm_response(fixture),
        litellm_response(escalation_response),
    ])
    config = AIConfig(enabled=True, escalation_enabled=True, escalation_min_keyword_score=70.0)
    ev = await AIEvaluator(config).evaluate(make_profile(), make_listing())
    # Filter's reason is preserved
    assert ev.reason == fixture["reason"]
    # Escalation's escalate flag is used
    assert ev.escalate is True


@respx.mock
async def test_escalation_second_call_skipped_when_disabled():
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    respx.post(OLLAMA_URL).mock(return_value=litellm_response(fixture))
    config = AIConfig(enabled=True, escalation_enabled=False)
    ev = await AIEvaluator(config).evaluate(make_profile(), make_listing())
    assert respx.calls.call_count == 1


@respx.mock
async def test_escalation_skipped_when_keyword_score_too_low():
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    respx.post(OLLAMA_URL).mock(return_value=litellm_response(fixture))
    config = AIConfig(enabled=True, escalation_enabled=True, escalation_min_keyword_score=70.0)
    low_score_listing = make_listing(relevance_score=50.0)
    ev = await AIEvaluator(config).evaluate(make_profile(), low_score_listing)
    assert respx.calls.call_count == 1  # no escalation call
