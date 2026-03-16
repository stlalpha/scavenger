import asyncio
import logging
import random
from datetime import datetime, timezone
from typing import Callable, Awaitable

from apscheduler.schedulers.asyncio import AsyncIOScheduler
from scavenger.models import Profile, Listing

logger = logging.getLogger(__name__)
PollCallback = Callable[[Profile], Awaitable[list[Listing]]]


class PollScheduler:
    def __init__(self):
        self._scheduler = AsyncIOScheduler()
        self._callbacks: dict[str, PollCallback] = {}
        self._profiles: dict[str, Profile] = {}
        self._running: bool = False

    @property
    def running(self) -> bool:
        return self._running

    async def start(self) -> None:
        self._scheduler.start()
        self._running = True

    async def stop(self) -> None:
        self._scheduler.shutdown(wait=False)
        self._running = False

    def add_profile(self, profile: Profile, callback: PollCallback) -> None:
        if not profile.enabled:
            return
        self._callbacks[profile.id] = callback
        self._profiles[profile.id] = profile
        interval = int(profile.poll_interval_sec * (1 + random.uniform(-0.1, 0.1)))
        self._scheduler.add_job(
            self._run_poll, "interval", seconds=interval,
            args=[profile.id], id=profile.id, replace_existing=True,
            next_run_time=datetime.now(timezone.utc),
        )

    def remove_profile(self, profile_id: str) -> None:
        if self._scheduler.get_job(profile_id):
            self._scheduler.remove_job(profile_id)
        self._callbacks.pop(profile_id, None)
        self._profiles.pop(profile_id, None)

    def has_job(self, profile_id: str) -> bool:
        return self._scheduler.get_job(profile_id) is not None

    async def trigger_now(self, profile_id: str) -> None:
        if profile_id not in self._callbacks:
            logger.warning("No callback for profile: %s", profile_id)
            return
        asyncio.create_task(self._run_poll(profile_id))

    async def _run_poll(self, profile_id: str) -> None:
        callback = self._callbacks.get(profile_id)
        profile = self._profiles.get(profile_id)
        if not callback or not profile:
            return
        try:
            await callback(profile)
        except Exception:
            logger.exception("Poll failed for profile: %s", profile_id)
