import asyncio
import json
from datetime import datetime, timezone
from unittest.mock import AsyncMock, patch

import pytest

from scavenger.ai.evaluator import AIEvaluator
from scavenger.ai.models import AIConfig
from scavenger.models import Listing, Profile


def make_profile() -> Profile:
    return Profile(
        id="sony", name="Sony A-mount Glass",
        keywords=["sony", ["a-mount", "alpha mount"]],
        negative_keywords=["broken"],
        sources=["ebay"], price_min=50.0, price_max=800.0,
    )


def make_listing(i: int) -> Listing:
    now = datetime.now(timezone.utc)
    return Listing(
        id=f"listing-{i}", profile_id="sony", source_id="ebay",
        title="Sony 85mm A-mount lens", description="Great condition",
        price=249.99, url=f"https://ebay.com/{i}",
        first_seen=now, last_seen=now, relevance_score=80.0,
    )


def _mock_response(content: str):
    mock = AsyncMock()
    mock.choices = [AsyncMock()]
    mock.choices[0].message.content = content
    return mock


def _mock_json_response(data):
    return _mock_response(json.dumps(data))


@pytest.fixture
async def evaluator():
    config = AIConfig(enabled=True)
    ev = AIEvaluator(config)
    await ev.start()
    yield ev
    await ev.stop()


@patch("scavenger.ai.evaluator.acompletion")
async def test_evaluate_batch_cancellation_propagates(mock_acomp, evaluator):
    """Cancelling an in-flight evaluate_batch raises CancelledError in the
    caller instead of being swallowed into a passthrough result, and leaves
    the worker pool intact and usable afterward."""
    started = asyncio.Event()
    release = asyncio.Event()

    async def slow_call(*args, **kwargs):
        started.set()
        await release.wait()
        return _mock_json_response([])

    mock_acomp.side_effect = slow_call

    listings = [make_listing(0)]
    task = asyncio.create_task(evaluator.evaluate_batch(make_profile(), listings))
    await started.wait()
    task.cancel()

    with pytest.raises(asyncio.CancelledError):
        await task
    assert task.cancelled()

    # release the in-flight worker call so it can settle against the
    # already-cancelled future without raising
    release.set()
    await asyncio.sleep(0)
    await asyncio.sleep(0)

    assert all(not t.done() for t in evaluator._worker_tasks)

    # queue must still work for subsequent calls after a cancellation
    mock_acomp.side_effect = None
    mock_acomp.return_value = _mock_json_response(
        [{"id": listings[0].id, "relevant": True, "reason": "ok", "notable": None, "escalate": False}]
    )
    results = await evaluator.evaluate_batch(make_profile(), listings)
    assert results[listings[0].id].relevant is True


@patch("scavenger.ai.evaluator.acompletion")
async def test_evaluate_batch_model_exception_yields_passthrough(mock_acomp, evaluator):
    mock_acomp.side_effect = Exception("connection failed")
    listings = [make_listing(0), make_listing(1)]
    results = await evaluator.evaluate_batch(make_profile(), listings)
    assert set(results.keys()) == {l.id for l in listings}
    assert all(r.relevant is True for r in results.values())  # never drop on error


@patch("scavenger.ai.evaluator.acompletion")
async def test_evaluate_batch_happy_path(mock_acomp, evaluator):
    listings = [make_listing(0), make_listing(1)]
    batch_response = [
        {"id": l.id, "relevant": True, "reason": "match", "notable": None, "escalate": False}
        for l in listings
    ]
    mock_acomp.return_value = _mock_json_response(batch_response)
    results = await evaluator.evaluate_batch(make_profile(), listings)
    assert len(results) == 2
    assert all(r.relevant is True for r in results.values())


@patch("scavenger.ai.evaluator.acompletion")
async def test_stop_with_deep_backlog_returns_promptly(mock_acomp):
    """stop() must cancel workers and drain the queue rather than grinding
    through every queued chunk — each of which would be a real (slow) model
    call — before returning."""

    async def slow_call(*args, **kwargs):
        await asyncio.sleep(5)
        return _mock_json_response([])

    mock_acomp.side_effect = slow_call

    config = AIConfig(enabled=True)
    ev = AIEvaluator(config)
    await ev.start()

    # Enough listings to build many BATCH_SIZE chunks; NUM_WORKERS pick up
    # the first few, the rest sit queued behind them.
    listings = [make_listing(i) for i in range(200)]
    task = asyncio.create_task(ev.evaluate_batch(make_profile(), listings))
    await asyncio.sleep(0.05)  # let workers pick up their first chunks

    start = asyncio.get_event_loop().time()
    await asyncio.wait_for(ev.stop(), timeout=1.0)
    elapsed = asyncio.get_event_loop().time() - start
    assert elapsed < 1.0

    # stop() resolves every pending job with passthrough rather than hanging
    # the caller or dropping listings.
    results = await asyncio.wait_for(task, timeout=1.0)
    assert set(results.keys()) == {l.id for l in listings}
    assert all(r.relevant is True for r in results.values())


@patch("scavenger.ai.evaluator.acompletion")
async def test_cancelled_evaluate_batch_skips_queued_model_calls(mock_acomp, evaluator):
    """Once a caller cancels evaluate_batch, any of its chunks still sitting
    in the queue (not yet picked up by a worker) must not trigger a model
    call at all."""
    started = asyncio.Event()
    release = asyncio.Event()
    call_count = 0

    async def slow_call(*args, **kwargs):
        nonlocal call_count
        call_count += 1
        started.set()
        await release.wait()
        return _mock_json_response([])

    mock_acomp.side_effect = slow_call

    # NUM_WORKERS == 3, BATCH_SIZE == 10 — 5 chunks means 3 get claimed by
    # workers immediately and 2 remain queued.
    listings = [make_listing(i) for i in range(50)]
    task = asyncio.create_task(evaluator.evaluate_batch(make_profile(), listings))
    await started.wait()
    calls_before_cancel = call_count
    task.cancel()

    with pytest.raises(asyncio.CancelledError):
        await task
    assert task.cancelled()

    release.set()
    await asyncio.sleep(0)
    await asyncio.sleep(0)

    # No additional model calls beyond what was already in-flight at the
    # moment of cancellation — the queued chunks were skipped, not run.
    assert call_count == calls_before_cancel


@patch("scavenger.ai.evaluator.acompletion")
async def test_evaluate_batch_after_stop_returns_passthrough_promptly(mock_acomp):
    """A call to evaluate_batch that arrives after stop() has already run
    must not queue jobs no worker will ever resolve — it should return
    passthrough for every listing immediately, never hang."""
    config = AIConfig(enabled=True)
    ev = AIEvaluator(config)
    await ev.start()
    await ev.stop()

    listings = [make_listing(0), make_listing(1)]
    results = await asyncio.wait_for(ev.evaluate_batch(make_profile(), listings), timeout=1.0)
    assert set(results.keys()) == {l.id for l in listings}
    assert all(r.relevant is True for r in results.values())
    mock_acomp.assert_not_called()


@patch("scavenger.ai.evaluator.acompletion")
async def test_evaluate_batch_stop_interleave_resolves_promptly(mock_acomp, evaluator):
    """If stop() runs concurrently while evaluate_batch is still queuing
    chunks, jobs put after the drain must still resolve to passthrough
    rather than hanging forever."""
    mock_acomp.side_effect = AssertionError("model must not be called once stopped")

    listings = [make_listing(i) for i in range(50)]
    task = asyncio.create_task(evaluator.evaluate_batch(make_profile(), listings))
    await asyncio.sleep(0)  # let it start queuing
    await evaluator.stop()

    results = await asyncio.wait_for(task, timeout=1.0)
    assert set(results.keys()) == {l.id for l in listings}
    assert all(r.relevant is True for r in results.values())
