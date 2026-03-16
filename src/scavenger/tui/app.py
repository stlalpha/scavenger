import asyncio
import logging
from textual.app import App, ComposeResult
from textual.binding import Binding
from scavenger.config import AppConfig
from scavenger.db import Database
from scavenger.tui.data import DataLayer
from scavenger.tui.messages import DataUpdated
from scavenger.tui.screens.main import MainScreen

logger = logging.getLogger(__name__)
POLL_INTERVAL = 2.0


class ScavengerApp(App):
    BINDINGS = [
        Binding("q", "quit_tui", "Quit"),
        Binding("Q", "quit_all", "Quit+Stop"),
        Binding("tab", "focus_next_panel", "Next panel"),
        Binding("shift+tab", "focus_prev_panel", "Prev panel"),
        Binding("right", "focus_next_panel", "Next panel", show=False),
        Binding("left", "focus_prev_panel", "Prev panel", show=False),
        Binding("o", "open_url", "Open", show=False),
        Binding("s", "save_listing", "Save", show=False),
        Binding("d", "dismiss_listing", "Dismiss", show=False),
        Binding("n", "snooze_listing", "Snooze", show=False),
        Binding("r", "repoll", "Re-poll", show=False),
        Binding("?", "show_help", "Help", show=False),
    ]

    def __init__(self, config: AppConfig) -> None:
        super().__init__()
        self._config = config
        self._db: Database | None = None
        self._data_layer: DataLayer | None = None
        self._active_profile_id: str | None = (
            config.profiles[0].id if config.profiles else None
        )

    def on_mount(self) -> None:
        self.set_interval(POLL_INTERVAL, self._poll)

    async def _poll(self) -> None:
        if self._data_layer is None:
            return
        try:
            listings = await self._data_layer.get_listings(
                profile_id=self._active_profile_id, limit=100
            )
            stats = await self._data_layer.get_profile_stats()
            self.post_message(DataUpdated(listings=listings, profile_stats=stats))
        except Exception as e:
            logger.warning("Poll error: %s", e)

    async def on_load(self) -> None:
        self._db = Database(self._config.db_path)
        await self._db.init()
        await self._db.migrate()
        self._data_layer = DataLayer(self._db)

    async def on_unmount(self) -> None:
        if self._db:
            await self._db.close()

    def compose(self) -> ComposeResult:
        yield MainScreen(profiles=self._config.profiles)

    def set_active_profile(self, profile_id: str | None) -> None:
        self._active_profile_id = profile_id

    def action_quit_tui(self) -> None:
        self.exit()

    def action_quit_all(self) -> None:
        import socket, json
        try:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            s.settimeout(2.0)
            s.connect(str(self._config.socket_path))
            s.sendall(json.dumps({"command": "shutdown"}).encode() + b"\n")
            s.close()
        except Exception:
            pass
        self.exit()

    def action_focus_next_panel(self) -> None:
        self.screen.focus_next()

    def action_focus_prev_panel(self) -> None:
        self.screen.focus_previous()

    def action_open_url(self) -> None:
        from scavenger.tui.widgets.results_feed import ResultsFeed
        import subprocess
        feed = self.query_one(ResultsFeed)
        if feed._listings and 0 <= feed.cursor < len(feed._listings):
            subprocess.Popen(["xdg-open", feed._listings[feed.cursor].url])

    def action_dismiss_listing(self) -> None:
        self._mark_focused("dismissed")

    def action_save_listing(self) -> None:
        self._mark_focused("saved")

    def action_snooze_listing(self) -> None:
        from datetime import datetime, timezone, timedelta
        until = datetime.now(timezone.utc) + timedelta(hours=1)
        self._mark_focused(f"snoozed_until:{until.isoformat()}")

    def _mark_focused(self, status: str) -> None:
        from scavenger.tui.widgets.results_feed import ResultsFeed
        feed = self.query_one(ResultsFeed)
        if feed._listings and 0 <= feed.cursor < len(feed._listings) and self._data_layer:
            listing_id = feed._listings[feed.cursor].id
            asyncio.create_task(self._data_layer.mark_status(listing_id, status))
            asyncio.create_task(self._poll())

    def action_repoll(self) -> None:
        asyncio.create_task(self._poll())

    def action_show_help(self) -> None:
        self.notify(
            "j/k ↑↓ navigate  o open  s save  d dismiss  n snooze  r repoll  q quit"
        )
