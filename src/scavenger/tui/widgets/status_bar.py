from datetime import datetime, timezone
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import Static


class StatusBar(Widget):
    DEFAULT_CSS = """
    StatusBar {
        dock: bottom;
        height: 1;
        background: $panel;
        padding: 0 1;
    }
    StatusBar #status-left { dock: left; width: auto; }
    StatusBar #status-right { dock: right; width: auto; text-align: right; }
    """

    def __init__(self) -> None:
        super().__init__()
        self.daemon_reachable: bool = True
        self.new_today: int = 0
        self._last_poll: str = "—"

    def compose(self) -> ComposeResult:
        yield Static(self._left(), id="status-left")
        yield Static(self._right(), id="status-right")

    def _left(self) -> str:
        if self.daemon_reachable:
            daemon = "[green]●[/] daemon"
        else:
            daemon = "[red]●[/] daemon [red]unreachable[/]"
        poll = f"[dim]polled {self._last_poll}[/]"
        new = f"[bold cyan]{self.new_today}[/] new" if self.new_today > 0 else "[dim]0 new[/]"
        return f" {daemon}  {poll}  {new}"

    def _right(self) -> str:
        return "[dim]\\[?] help  \\[q] quit  \\[Q] quit+stop [/]"

    def _refresh(self) -> None:
        try:
            self.query_one("#status-left", Static).update(self._left())
        except Exception:
            pass

    def set_daemon_status(self, reachable: bool) -> None:
        self.daemon_reachable = reachable
        self._refresh()

    def set_last_poll(self, ts: datetime) -> None:
        delta = int((datetime.now(timezone.utc) - ts).total_seconds())
        if delta < 60:
            self._last_poll = f"{delta}s ago"
        elif delta < 3600:
            self._last_poll = f"{delta // 60}m ago"
        else:
            self._last_poll = f"{delta // 3600}h ago"
        self._refresh()

    def set_new_count(self, count: int) -> None:
        self.new_today = count
        self._refresh()
