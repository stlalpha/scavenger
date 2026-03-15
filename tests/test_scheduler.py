import asyncio
import pytest
from unittest.mock import AsyncMock
from scavenger.daemon.scheduler import PollScheduler
from scavenger.models import Profile


def make_profile(**overrides) -> Profile:
    return Profile(**{
        "id": "test", "name": "Test", "keywords": ["test"],
        "negative_keywords": [], "sources": ["ebay"],
        "poll_interval_sec": 60, "enabled": True, **overrides
    })


async def test_scheduler_starts_and_stops():
    s = PollScheduler()
    await s.start()
    assert s.running
    await s.stop()
    assert not s.running


async def test_add_profile_creates_job():
    s = PollScheduler()
    await s.start()
    s.add_profile(make_profile(), callback=AsyncMock())
    assert s.has_job("test")
    await s.stop()


async def test_disabled_profile_not_scheduled():
    s = PollScheduler()
    await s.start()
    s.add_profile(make_profile(enabled=False), callback=AsyncMock())
    assert not s.has_job("test")
    await s.stop()


async def test_remove_profile_removes_job():
    s = PollScheduler()
    await s.start()
    s.add_profile(make_profile(), callback=AsyncMock())
    s.remove_profile("test")
    assert not s.has_job("test")
    await s.stop()


async def test_trigger_now_calls_callback():
    s = PollScheduler()
    await s.start()
    mock_cb = AsyncMock(return_value=[])
    profile = make_profile()
    s.add_profile(profile, callback=mock_cb)
    await s.trigger_now("test")
    await asyncio.sleep(0.05)
    mock_cb.assert_called_once_with(profile)
    await s.stop()
