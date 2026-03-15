import asyncio
import logging
import signal
from datetime import datetime, timezone

from scavenger.config import AppConfig
from scavenger.daemon.scheduler import PollScheduler
from scavenger.daemon.socket_server import SocketServer
from scavenger.db import Database
from scavenger.models import Profile, Listing
from scavenger.plugins.ebay import EbayPlugin
from scavenger.plugins.craigslist import CraigslistPlugin
from scavenger.scoring import score_listing

logger = logging.getLogger(__name__)

BUNDLED_PLUGINS = {
    EbayPlugin.plugin_id: EbayPlugin(),
    CraigslistPlugin.plugin_id: CraigslistPlugin(),
}


class Daemon:
    def __init__(self, config: AppConfig):
        self._config = config
        self._db = Database(config.db_path)
        self._scheduler = PollScheduler()
        self._socket_server = SocketServer(config.socket_path)
        self._plugins = dict(BUNDLED_PLUGINS)

    def _register_profiles(self) -> None:
        for profile in self._config.profiles:
            if profile.enabled:
                self._scheduler.add_profile(profile, callback=self._make_callback(profile))

    def _make_callback(self, profile: Profile):
        async def callback(p: Profile) -> list[Listing]:
            return await self._poll_profile(p)
        return callback

    async def _poll_profile(self, profile: Profile) -> list[Listing]:
        new_listings = []
        for source_id in profile.sources:
            plugin = self._plugins.get(source_id)
            if plugin is None:
                logger.warning("Unknown plugin: %s", source_id)
                continue
            try:
                fetched = await plugin.fetch(profile)
                for listing in fetched:
                    listing.relevance_score = score_listing(
                        profile, listing.title, listing.description, listing.price
                    )
                    if listing.relevance_score == 0.0:
                        continue
                    if await self._db.upsert_listing(listing):
                        new_listings.append(listing)
                await self._db.update_source_state(source_id, last_polled=datetime.now(timezone.utc))
            except Exception:
                logger.exception("Poll failed for %s/%s", profile.id, source_id)
                state = await self._db.get_source_state(source_id)
                current_errors = state["consecutive_errors"] if state else 0
                await self._db.update_source_state(source_id, consecutive_errors=current_errors + 1)
        return new_listings

    async def run(self) -> None:
        await self._db.init()
        await self._scheduler.start()
        await self._socket_server.start()
        self._socket_server.register_poll_handler(self._handle_poll_command)
        self._register_profiles()
        logger.info("Daemon started")
        stop_event = asyncio.Event()
        loop = asyncio.get_running_loop()
        for sig in (signal.SIGINT, signal.SIGTERM):
            loop.add_signal_handler(sig, stop_event.set)
        await stop_event.wait()
        await self.shutdown()

    async def _handle_poll_command(self, profile_id: str) -> None:
        profile = next((p for p in self._config.profiles if p.id == profile_id), None)
        if profile:
            await self._poll_profile(profile)

    async def shutdown(self) -> None:
        await self._scheduler.stop()
        await self._socket_server.stop()
        await self._db.close()
