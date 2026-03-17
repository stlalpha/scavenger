import json
from unittest.mock import AsyncMock, patch
import pytest
from pathlib import Path
from scavenger.ai.evaluator import NoopEvaluator, AIEvaluator
from scavenger.ai.models import AIConfig, AIEvaluation
from scavenger.models import Profile, Listing
from datetime import datetime, timezone

FIXTURES = Path(__file__).parent / "fixtures"


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


def _mock_response(content: str):
    """Create a mock litellm response."""
    mock = AsyncMock()
    mock.choices = [AsyncMock()]
    mock.choices[0].message.content = content
    return mock


def _mock_json_response(data: dict):
    return _mock_response(json.dumps(data))


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


@patch("scavenger.ai.evaluator.acompletion")
async def test_evaluator_parses_valid_response(mock_acomp, evaluator):
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    mock_acomp.return_value = _mock_json_response(fixture)
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert ev.relevant is True
    assert ev.notable is not None
    assert "Zeiss" in ev.notable


@patch("scavenger.ai.evaluator.acompletion")
async def test_evaluator_returns_passthrough_on_malformed_json(mock_acomp, evaluator):
    mock_acomp.return_value = _mock_response("not json at all")
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert ev.relevant is True  # passthrough — never drop on error


@patch("scavenger.ai.evaluator.acompletion")
async def test_evaluator_returns_passthrough_on_error(mock_acomp, evaluator):
    mock_acomp.side_effect = Exception("connection failed")
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert ev.relevant is True


@patch("scavenger.ai.evaluator.acompletion")
async def test_evaluator_not_relevant_discards(mock_acomp, evaluator):
    mock_acomp.return_value = _mock_json_response({
        "relevant": False, "reason": "Wrong mount", "notable": None, "escalate": False
    })
    ev = await evaluator.evaluate(make_profile(), make_listing())
    assert ev.relevant is False


@patch("scavenger.ai.evaluator.acompletion")
async def test_escalation_second_call_fires_when_conditions_met(mock_acomp):
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    escalation_response = {**fixture, "escalate": True}
    # First call = filter (Ollama), second = escalation (Anthropic)
    mock_acomp.side_effect = [
        _mock_json_response(fixture),
        _mock_json_response(escalation_response),
    ]
    config = AIConfig(enabled=True, escalation_enabled=True, anthropic_api_key="test-key")
    ev = AIEvaluator(config)
    await ev.start()
    result = await ev.evaluate(make_profile(), make_listing())
    await ev.stop()
    assert result.escalate is True
    assert mock_acomp.call_count == 2


@patch("scavenger.ai.evaluator.acompletion")
async def test_escalation_uses_frontier_reason(mock_acomp):
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    escalation_response = {"relevant": True, "reason": "Escalation reason", "notable": "Even more notable", "escalate": True}
    mock_acomp.side_effect = [
        _mock_json_response(fixture),
        _mock_json_response(escalation_response),
    ]
    config = AIConfig(enabled=True, escalation_enabled=True, anthropic_api_key="test-key")
    ev = AIEvaluator(config)
    await ev.start()
    result = await ev.evaluate(make_profile(), make_listing())
    await ev.stop()
    assert result.reason == "Escalation reason"
    assert result.notable == "Even more notable"
    assert result.escalate is True


@patch("scavenger.ai.evaluator.acompletion")
async def test_escalation_second_call_skipped_when_disabled(mock_acomp, evaluator):
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    mock_acomp.return_value = _mock_json_response(fixture)
    await evaluator.evaluate(make_profile(), make_listing())
    assert mock_acomp.call_count == 1  # filter only, no escalation


@patch("scavenger.ai.evaluator.acompletion")
async def test_escalation_skipped_when_keyword_score_too_low(mock_acomp, evaluator):
    fixture = json.loads((FIXTURES / "ai_evaluation_sony.json").read_text())
    fixture["escalate"] = True
    mock_acomp.return_value = _mock_json_response(fixture)
    low_score_listing = make_listing(relevance_score=50.0)
    await evaluator.evaluate(make_profile(), low_score_listing)
    assert mock_acomp.call_count == 1  # filter only
