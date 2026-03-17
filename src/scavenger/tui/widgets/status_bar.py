from datetime import datetime, timezone
from textual.app import ComposeResult
from textual.timer import Timer
from textual.widget import Widget
from textual.widgets import Static

SPINNER = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
SOURCE_COLORS = {"ebay": "yellow", "craigslist": "magenta", "facebook": "blue"}


def _format_age(delta_sec: int) -> str:
    if delta_sec < 60:
        return f"{delta_sec}s ago"
    if delta_sec < 3600:
        return f"{delta_sec // 60}m ago"
    return f"{delta_sec // 3600}h ago"


def _format_time(dt: datetime) -> str:
    local = dt.astimezone()
    return local.strftime("%H:%M:%S")


class StatusBar(Widget):
    DEFAULT_CSS = """
    StatusBar {
        dock: bottom;
        height: 1;
        max-height: 1;
        overflow: hidden;
        background: $panel;
        padding: 0 1;
    }
    StatusBar #status-left { dock: left; width: auto; max-height: 1; overflow: hidden; }
    StatusBar #status-right { dock: right; width: auto; max-height: 1; overflow: hidden; }
    """

    def __init__(self) -> None:
        super().__init__()
        self.daemon_reachable: bool = True
        self.new_today: int = 0
        self._last_poll_ts: datetime | None = None
        self._source_states: list[dict] = []
        self._active_polls: list[str] = []
        self._spinner_idx: int = 0
        self._spinner_timer: Timer | None = None

    def compose(self) -> ComposeResult:
        yield Static(self._left(), id="status-left")
        yield Static(self._right(), id="status-right")

    def _spin(self) -> None:
        self._spinner_idx += 1
        self._refresh()

    def _start_spinner(self) -> None:
        if self._spinner_timer is not None:
            return
        self._spinner_timer = self.set_interval(0.1, self._spin)

    def _stop_spinner(self) -> None:
        if self._spinner_timer:
            self._spinner_timer.stop()
            self._spinner_timer = None

    def _left(self) -> str:
        parts = []

        # Daemon indicator
        if self.daemon_reachable:
            parts.append("[green]●[/] daemon")
        else:
            parts.append("[red]●[/] daemon [red]down[/]")

        # Polling activity — driven by daemon's active_polls
        if self._active_polls:
            frame = SPINNER[self._spinner_idx % len(SPINNER)]
            polling_sources = " ".join(
                f"[{SOURCE_COLORS.get(s, 'white')} bold]{s[:2].upper()}[/]"
                for s in self._active_polls
            )
            parts.append(f"[bold yellow]{frame}[/] polling {polling_sources}")

        # Per-source last poll times
        if self._source_states:
            src_parts = []
            now = datetime.now(timezone.utc)
            for src in self._source_states:
                pid = src["plugin_id"]
                # Skip sources shown in active polls
                if pid in self._active_polls:
                    continue
                color = SOURCE_COLORS.get(pid, "white")
                last = src.get("last_polled")
                errors = src.get("consecutive_errors", 0)
                if errors > 0:
                    src_parts.append(f"[{color}]{pid[:2].upper()}[/][red]!{errors}[/]")
                elif last:
                    dt = datetime.fromisoformat(last)
                    delta = int((now - dt).total_seconds())
                    src_parts.append(f"[{color}]{pid[:2].upper()}[/][dim] {_format_age(delta)}[/]")
                else:
                    src_parts.append(f"[{color} dim]{pid[:2].upper()}[/]")
            if src_parts:
                parts.append(" ".join(src_parts))

        # New count
        if self.new_today > 0:
            parts.append(f"[bold cyan]{self.new_today}[/] new")
        else:
            parts.append("[dim]0 new[/]")

        # Last poll timestamp
        if self._last_poll_ts:
            parts.append(f"[dim]last: {_format_time(self._last_poll_ts)}[/]")

        return " " + "  ".join(parts)

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

    def set_active_polls(self, active: list[str]) -> None:
        was_polling = bool(self._active_polls)
        self._active_polls = active
        if active and not was_polling:
            self._start_spinner()
        elif not active and was_polling:
            self._stop_spinner()
        self._refresh()

    def set_last_poll(self, ts: datetime) -> None:
        self._last_poll_ts = ts
        self._refresh()

    def set_source_states(self, states: list[dict]) -> None:
        self._source_states = states
        self._refresh()

    def set_new_count(self, count: int) -> None:
        self.new_today = count
        self._refresh()
