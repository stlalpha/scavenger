import asyncio
import logging
from pathlib import Path
from textual.app import App
from textual.binding import Binding
from scavenger.config import AppConfig
from scavenger.db import Database
from scavenger.tui.data import DataLayer
from scavenger.tui.messages import DataUpdated
from scavenger.tui.screens.main import MainScreen

logger = logging.getLogger(__name__)
POLL_INTERVAL = 2.0


class ScavengerApp(App):
    TITLE = "SCAVENGER"

    CSS = """
    Screen {
        background: $background;
    }
    """

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
        Binding("a", "add_profile", "Add profile", show=False),
        Binding("e", "edit_profile", "Edit profile", show=False),
        Binding("r", "repoll", "Re-poll", show=False),
        Binding("?", "show_help", "Help", show=False),
    ]

    def __init__(self, config: AppConfig, config_path: Path | None = None) -> None:
        super().__init__()
        self._config = config
        self._config_path = config_path
        self._db: Database | None = None
        self._data_layer: DataLayer | None = None
        self._active_profile_id: str | None = (
            config.profiles[0].id if config.profiles else None
        )

    def on_mount(self) -> None:
        self.push_screen(MainScreen(profiles=self._config.profiles))
        self.run_worker(self._poll(), exclusive=True)
        self._poll_timer = self.set_interval(POLL_INTERVAL, self._poll)

    async def _poll(self) -> None:
        if self._data_layer is None:
            return
        from datetime import datetime, timezone
        from scavenger.tui.widgets.status_bar import StatusBar
        try:
            listings = await self._data_layer.get_listings(
                profile_id=self._active_profile_id, limit=100
            )
            stats = await self._data_layer.get_profile_stats()
            # Post to the current screen, not the app — messages don't bubble down
            self.screen.post_message(DataUpdated(listings=listings, profile_stats=stats))
            # Check daemon socket (in thread to avoid blocking event loop)
            daemon_status = await asyncio.to_thread(self._check_daemon)
            last_source_poll = await self._data_layer.get_last_source_poll()
            source_states = await self._data_layer.get_source_states()
            try:
                bar = self.screen.query_one(StatusBar)
                bar.set_daemon_status(daemon_status["up"])
                bar.set_active_polls(daemon_status["active_polls"])
                bar.set_source_states(source_states)
                if last_source_poll:
                    bar.set_last_poll(last_source_poll)
            except Exception:
                pass
            # Show which profiles the daemon knows about
            try:
                from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
                sidebar = self.screen.query_one(ProfileSidebar)
                sidebar.set_daemon_profiles(daemon_status.get("profiles", []))
            except Exception:
                pass
        except Exception as e:
            logger.warning("Poll error: %s", e)

    def _check_daemon(self) -> dict:
        """Query daemon status. Returns {"up": bool, "active_polls": list[str]}."""
        import socket as _socket, json
        try:
            s = _socket.socket(_socket.AF_UNIX, _socket.SOCK_STREAM)
            s.settimeout(1.0)
            s.connect(str(self._config.socket_path))
            s.sendall(json.dumps({"command": "status"}).encode() + b"\n")
            data = b""
            while not data.endswith(b"\n"):
                chunk = s.recv(4096)
                if not chunk:
                    break
                data += chunk
            s.close()
            resp = json.loads(data)
            if resp.get("status") == "ok":
                return {
                    "up": True,
                    "active_polls": resp.get("data", {}).get("active_polls", []),
                }
        except Exception:
            pass
        return {"up": False, "active_polls": []}

    async def on_load(self) -> None:
        self._db = Database(self._config.db_path)
        await self._db.init()
        await self._db.migrate()
        self._data_layer = DataLayer(self._db)

    async def on_unmount(self) -> None:
        if hasattr(self, "_poll_timer"):
            self._poll_timer.stop()
        if self._db:
            await self._db.close()

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

    def _active_listing(self) -> "Listing | None":
        from scavenger.tui.widgets.detail_panel import DetailPanel
        from scavenger.tui.widgets.results_feed import ResultsFeed
        try:
            detail = self.screen.query_one(DetailPanel)
            if detail.current_listing:
                return detail.current_listing
        except Exception:
            pass
        try:
            feed = self.screen.query_one(ResultsFeed)
            return feed.focused_listing
        except Exception:
            return None

    def action_open_url(self) -> None:
        import subprocess, sys
        listing = self._active_listing()
        if listing:
            opener = "open" if sys.platform == "darwin" else "xdg-open"
            subprocess.Popen([opener, listing.url])

    def action_dismiss_listing(self) -> None:
        self._mark_focused("dismissed")

    def action_save_listing(self) -> None:
        self._mark_focused("saved")

    def action_snooze_listing(self) -> None:
        self._mark_focused("snoozed")

    def _mark_focused(self, status: str) -> None:
        listing = self._active_listing()
        if listing and self._data_layer:
            listing_id = listing.id
            async def _do() -> None:
                await self._data_layer.mark_status(listing_id, status)  # type: ignore[union-attr]
                await self._poll()
            self.run_worker(_do(), exclusive=False)

    def action_add_profile(self) -> None:
        from scavenger.tui.screens.add_profile import ProfileFormScreen
        self.push_screen(ProfileFormScreen(), callback=self._on_profile_form_result)

    def action_edit_profile(self) -> None:
        from scavenger.tui.screens.add_profile import ProfileFormScreen
        profile = next((p for p in self._config.profiles if p.id == self._active_profile_id), None)
        if not profile:
            self.notify("No profile selected", severity="warning")
            return
        self.push_screen(ProfileFormScreen(profile=profile), callback=self._on_profile_form_result)

    def _on_profile_form_result(self, result: dict | None) -> None:
        if result is None:
            return
        from scavenger.config import append_profile, update_profile, delete_profile, ConfigError
        from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
        config_path = self._config_path or Path("~/.config/scavenger/config.toml").expanduser()
        action = result.pop("_action", "create")

        if action == "delete":
            try:
                delete_profile(config_path, result["id"])
            except ConfigError as e:
                self.notify(f"Delete failed: {e}", severity="error")
                return
            self._config.profiles = [p for p in self._config.profiles if p.id != result["id"]]
            # Delete associated listings from DB
            if self._data_layer:
                async def _clean() -> None:
                    count = await self._data_layer._db.delete_profile_listings(result["id"])
                    if count:
                        self.notify(f"Removed {count} listings")
                self.run_worker(_clean(), exclusive=False)
            # Switch away from deleted profile before rebuilding UI
            if self._active_profile_id == result["id"]:
                self.set_active_profile(self._config.profiles[0].id if self._config.profiles else None)
            self._send_daemon_command({"command": "reload"})
            self.notify(f"Profile '{result['name']}' deleted")
            # Rebuild sidebar and refresh feed in sequence
            self.run_worker(self._rebuild_and_poll(), exclusive=True)
            return

        if action == "update":
            try:
                profile = update_profile(config_path, result)
            except ConfigError as e:
                self.notify(f"Update failed: {e}", severity="error")
                return
            self._config.profiles = [profile if p.id == profile.id else p for p in self._config.profiles]
            self._send_daemon_command({"command": "reload"})
            self.notify(f"Profile '{profile.name}' updated")
            self.run_worker(self._rebuild_and_poll(), exclusive=True)
            return

        # action == "create"
        try:
            profile = append_profile(config_path, result)
        except ConfigError as e:
            self.notify(f"Failed: {e}", severity="error")
            return
        self._config.profiles.append(profile)
        try:
            sidebar = self.screen.query_one(ProfileSidebar)
            self.run_worker(sidebar.add_profile(profile), exclusive=False)
        except Exception:
            pass
        self._send_daemon_command({"command": "reload"})
        self.notify(f"Profile '{profile.name}' created — polling started")
        self.set_active_profile(profile.id)
        self.run_worker(self._trigger_daemon_poll(), exclusive=True)

    async def _rebuild_and_poll(self) -> None:
        """Rebuild the sidebar then refresh listings. Runs as a single worker to avoid races."""
        from scavenger.tui.widgets.profile_sidebar import ProfileSidebar
        from scavenger.tui.widgets.results_feed import ResultsFeed
        try:
            sidebar = self.screen.query_one(ProfileSidebar)
            await sidebar.rebuild(self._config.profiles)
        except Exception:
            pass
        try:
            feed = self.screen.query_one(ResultsFeed)
            feed.invalidate_fingerprint()
        except Exception:
            pass
        await self._poll()

    def _send_daemon_command(self, command: dict) -> dict | None:
        import json as _json, socket
        try:
            s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            s.settimeout(5.0)
            s.connect(str(self._config.socket_path))
            s.sendall(_json.dumps(command).encode() + b"\n")
            data = s.recv(4096)
            s.close()
            return _json.loads(data)
        except Exception:
            return None

    def action_repoll(self) -> None:
        self.run_worker(self._trigger_daemon_poll(), exclusive=True)

    async def _trigger_daemon_poll(self) -> None:
        """Tell the daemon to poll the active profile, then refresh the TUI."""
        profile_id = self._active_profile_id
        if not profile_id:
            return
        result = await asyncio.to_thread(
            self._send_daemon_command, {"command": "poll", "profile_id": profile_id}
        )
        if result is None:
            self.notify("Daemon not reachable", severity="warning")
        await self._poll()

    def action_show_help(self) -> None:
        self.notify(
            "j/k ↑↓ navigate  o open  s save  d dismiss  n snooze  a add  r repoll  q quit"
        )
