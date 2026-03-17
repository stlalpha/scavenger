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
OLLAMA_URL = "http://localhost:11434/api/chat"
ANTHROPIC_URL = "https://api.anthropic.com/v1/messages"


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


def ollama_response(evaluation: dict) -> httpx.Response:
    return httpx.Response(200, json={
        "message": {"role": "assistant", "content": json.dumps(evaluation)}
    })


def anthropic_response(evaluation: dict) -> httpx.Response:
    return httpx.Response(200, json={
        "content": [{"type": "text", "text": json.dumps(evaluation)}],
        "role": "assistant",
    })


# --- NoopEvaluator ---

async def test_noop_evaluator_returns_passthrough():
    ev = await NoopEvaluator().evaluate(make_profile(), make_listing())
    assert ev.relevant is True
    assert ev.escalate is False


# --- AIEvaluator ---


@pytest.fixture
async def evaluator():
    config = AIConfig(enabled=True)
    ev = AIEvaluator(config)
    await ev.start()
    yield ev
    await ev.stop()


@respx.mock
async def test_evaluator_parses_valid_response(evaluator):
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    respx.post(OLLAMA_URL).mock(return_value=ollama_response(fixture))
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert ev.relevant is True
    assert ev.notable is not None
    assert "Zeiss" in ev.notable


@respx.mock
async def test_evaluator_returns_passthrough_on_malformed_json(evaluator):
    respx.post(OLLAMA_URL).mock(return_value=httpx.Response(200, json={
        "message": {"role": "assistant", "content": "not json at all"}
    }))
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert ev.relevant is True  # passthrough — never drop on error


@respx.mock
async def test_evaluator_returns_passthrough_on_http_error(evaluator):
    respx.post(OLLAMA_URL).mock(return_value=httpx.Response(503))
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert ev.relevant is True


@respx.mock
async def test_evaluator_returns_passthrough_on_timeout(evaluator):
    respx.post(OLLAMA_URL).mock(side_effect=httpx.TimeoutException("timeout"))
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert ev.relevant is True


@respx.mock
async def test_evaluator_not_relevant_discards(evaluator):
    respx.post(OLLAMA_URL).mock(return_value=ollama_response({
        "relevant": False, "reason": "Wrong mount", "notable": None, "escalate": False
    }))
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert ev.relevant is False


@respx.mock
async def test_escalation_second_call_fires_when_conditions_met():
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    escalation_response = {**fixture, "escalate": True}
    respx.post(OLLAMA_URL).mock(return_value=ollama_response(fixture))
    respx.post(ANTHROPIC_URL).mock(return_value=anthropic_response(escalation_response))
    config = AIConfig(enabled=True, escalation_enabled=True, escalation_min_keyword_score=70.0, anthropic_api_key="test-key")
    ev = AIEvaluator(config)
    await ev.start()
    result = await ev.evaluate(make_profile(), make_listing())
    await ev.stop()
    assert result.escalate is True


@respx.mock
async def test_escalation_uses_frontier_reason():
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    escalation_response = {"relevant": True, "reason": "Escalation reason", "notable": "Even more notable", "escalate": True}
    respx.post(OLLAMA_URL).mock(return_value=ollama_response(fixture))
    respx.post(ANTHROPIC_URL).mock(return_value=anthropic_response(escalation_response))
    config = AIConfig(enabled=True, escalation_enabled=True, escalation_min_keyword_score=70.0, anthropic_api_key="test-key")
    ev = AIEvaluator(config)
    await ev.start()
    result = await ev.evaluate(make_profile(), make_listing())
    await ev.stop()
    assert result.reason == "Escalation reason"
    assert result.notable == "Even more notable"
    assert result.escalate is True


@respx.mock
async def test_escalation_second_call_skipped_when_disabled(evaluator):
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    respx.post(OLLAMA_URL).mock(return_value=ollama_response(fixture))
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert respx.calls.call_count == 1


@respx.mock
async def test_escalation_skipped_when_keyword_score_too_low(evaluator):
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    respx.post(OLLAMA_URL).mock(return_value=ollama_response(fixture))
    low_score_listing = make_listing(relevance_score=50.0)
    ev = await evaluator.evaluate(make_profile(), low_score_listing)
    assert respx.calls.call_count == 1  # no escalation call
