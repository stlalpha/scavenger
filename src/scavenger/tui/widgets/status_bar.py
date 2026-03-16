from datetime import datetime, timezone
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import Static


class StatusBar(Widget):
    DEFAULT_CSS = """
    StatusBar {
        dock: bottom;
        height: 1;
        background: $panel-darken-1;
        padding: 0 1;
    }
    """

    def __init__(self) -> None:
        super().__init__()
        self.daemon_reachable: bool = True
        self.new_today: int = 0
        self._last_poll: str = "—"

    def compose(self) -> ComposeResult:
        yield Static(self._text(), id="status-text")

    def _text(self) -> str:
        icon = "●" if self.daemon_reachable else "✕"
        status = "running" if self.daemon_reachable else "unreachable"
        return f"{icon} daemon: {status}  ·  last poll: {self._last_poll}  ·  {self.new_today} new today  ·  [?] help  [q] quit"

    def _refresh(self) -> None:
        try:
            self.query_one("#status-text", Static).update(self._text())
        except Exception:
            pass

    def set_daemon_status(self, reachable: bool) -> None:
        self.daemon_reachable = reachable
        self._refresh()

    def set_last_poll(self, ts: datetime) -> None:
        delta = int((datetime.now(timezone.utc) - ts).total_seconds())
        self._last_poll = f"{delta}s ago"
        self._refresh()

    def set_new_count(self, count: int) -> None:
        self.new_today = count
        self._refresh()
