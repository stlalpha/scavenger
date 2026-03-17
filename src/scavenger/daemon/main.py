import asyncio
import logging
import signal
from datetime import datetime, timezone

from scavenger.config import AppConfig, load_config
from scavenger.daemon.scheduler import PollScheduler
from scavenger.daemon.socket_server import SocketServer
from scavenger.db import Database
from scavenger.models import Profile, Listing
from scavenger.plugins.ebay import EbayPlugin
from scavenger.plugins.craigslist import CraigslistPlugin
from scavenger.plugins.facebook import FacebookPlugin
from scavenger.scoring import score_listing
from scavenger.ai.evaluator import AIEvaluator, NoopEvaluator
from scavenger.ai.models import AIConfig

logger = logging.getLogger(__name__)

def _make_plugins(config: AppConfig) -> dict:
    home_zip = config.global_config.home_zip
    return {
        EbayPlugin.plugin_id: EbayPlugin(),
        CraigslistPlugin.plugin_id: CraigslistPlugin(home_zip=home_zip),
        FacebookPlugin.plugin_id: FacebookPlugin(),
    }


class Daemon:
    def __init__(self, config: AppConfig, ai_config: AIConfig | None = None, config_path: str | None = None):
        self._config = config
        self._config_path = config_path
        self._db = Database(config.db_path)
        self._scheduler = PollScheduler()
        self._socket_server = SocketServer(config.socket_path)
        self._plugins = _make_plugins(config)
        self._evaluator = AIEvaluator(ai_config) if (ai_config and ai_config.enabled) else NoopEvaluator()
        self._active_polls: set[str] = set()  # source_ids currently being polled

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
            self._active_polls.add(source_id)
            try:
                fetched = await plugin.fetch(profile)
                # Skip known listings, score the rest
                known_ids = await self._db.get_existing_ids(
                    [listing.id for listing in fetched]
                )
                scored = []
                for listing in fetched:
                    if listing.id in known_ids:
                        continue
                    listing.relevance_score = score_listing(
                        profile, listing.title, listing.description, listing.price
                    )
                    if listing.relevance_score > 0.0:
                        scored.append(listing)
                # Batch AI evaluation — only new listings
                evaluations = await self._evaluator.evaluate_batch(profile, scored)
                for listing in scored:
                    evaluation = evaluations.get(listing.id)
                    if evaluation and not evaluation.relevant:
                        continue
                    if evaluation:
                        listing.ai_evaluation = evaluation.model_dump_json()
                    if await self._db.upsert_listing(listing):
                        new_listings.append(listing)
                await self._db.update_source_state(source_id, last_polled=datetime.now(timezone.utc))
            except Exception:
                logger.exception("Poll failed for %s/%s", profile.id, source_id)
                state = await self._db.get_source_state(source_id)
                current_errors = state["consecutive_errors"] if state else 0
                existing_last_polled = None
                if state and state.get("last_polled"):
                    existing_last_polled = datetime.fromisoformat(state["last_polled"])
                await self._db.update_source_state(
                    source_id,
                    last_polled=existing_last_polled,
                    consecutive_errors=current_errors + 1,
                )
            finally:
                self._active_polls.discard(source_id)
        return new_listings

    async def run(self) -> None:
        logging.basicConfig(
            level=self._config.log_level,
            format="%(asctime)s %(levelname)s %(name)s: %(message)s",
            datefmt="%H:%M:%S",
            force=True,
        )
        await self._db.init()
        await self._db.migrate()  # apply schema migrations
        await self._evaluator.start()
        await self._scheduler.start()
        stop_event = asyncio.Event()
        loop = asyncio.get_running_loop()
        for sig in (signal.SIGINT, signal.SIGTERM):
            loop.add_signal_handler(sig, stop_event.set)
        # Register handlers BEFORE starting the socket so no command arrives unhandled
        self._socket_server.register_status_handler(self._handle_status)
        self._socket_server.register_poll_handler(self._handle_poll_command)
        self._socket_server.register_reload_handler(self._handle_reload)
        self._socket_server.register_shutdown_handler(stop_event.set)
        await self._socket_server.start()
        self._register_profiles()
        logger.info(
            "Daemon started — socket: %s, profiles: %d",
            self._config.socket_path,
            len([p for p in self._config.profiles if p.enabled]),
        )
        await stop_event.wait()
        await self.shutdown()

    def _handle_status(self) -> dict:
        return {
            "state": "running",
            "active_polls": sorted(self._active_polls),
            "profiles": [p.id for p in self._config.profiles if p.enabled],
        }

    async def _handle_reload(self) -> None:
        """Re-read config and register any new profiles."""
        if not self._config_path:
            logger.warning("No config path — cannot reload")
            return
        from pathlib import Path
        try:
            new_config = load_config(Path(self._config_path))
        except Exception as e:
            logger.warning("Reload failed: %s", e)
            return
        existing_ids = {p.id for p in self._config.profiles}
        added = 0
        for profile in new_config.profiles:
            if profile.id not in existing_ids and profile.enabled:
                self._config.profiles.append(profile)
                self._scheduler.add_profile(profile, callback=self._make_callback(profile))
                added += 1
                logger.info("Loaded new profile: %s", profile.name)
        if added:
            logger.info("Reload: added %d new profile(s)", added)

    async def _handle_poll_command(self, profile_id: str) -> None:
        profile = next((p for p in self._config.profiles if p.id == profile_id), None)
        if profile:
            await self._poll_profile(profile)

    async def shutdown(self) -> None:
        await self._scheduler.stop()
        await self._evaluator.stop()
        await self._socket_server.stop()
        await self._db.close()
