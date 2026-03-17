import asyncio
import logging
from pathlib import Path
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import Static, RichLog

logger = logging.getLogger(__name__)

DEFAULT_LOG = Path("~/.local/share/scavenger/daemon.log").expanduser()
MAX_LINES = 500


class LogPanel(Widget):
    """Tails the daemon log file with color-coded output."""

    can_focus = True

    DEFAULT_CSS = """
    LogPanel {
        width: 100%;
        height: 100%;
        background: $surface;
        border-top: hkey $panel-darken-2;
    }
    LogPanel #log-header {
        dock: top;
        height: 1;
        padding: 0 1;
        color: $text-muted;
        text-style: bold;
        background: $panel;
    }
    LogPanel RichLog {
        height: 1fr;
        padding: 0 1;
        background: $surface;
        scrollbar-size: 1 1;
    }
    """

    def __init__(self, log_path: Path = DEFAULT_LOG) -> None:
        super().__init__()
        self._log_path = log_path
        self._tail_task: asyncio.Task | None = None
        self._last_size: int = 0

    def compose(self) -> ComposeResult:
        yield Static(" LOG", id="log-header")
        yield RichLog(highlight=False, markup=True, wrap=True, max_lines=MAX_LINES, id="log-output")

    def on_mount(self) -> None:
        self._tail_task = asyncio.create_task(self._tail())

    def on_unmount(self) -> None:
        if self._tail_task:
            self._tail_task.cancel()

    async def _tail(self) -> None:
        log = self.query_one("#log-output", RichLog)

        if self._log_path.exists():
            try:
                text = self._log_path.read_text()
                self._last_size = len(text.encode())
                lines = text.strip().split("\n")
                for line in lines[-40:]:
                    log.write(self._colorize(line))
            except Exception:
                pass

        while True:
            await asyncio.sleep(0.8)
            try:
                if not self._log_path.exists():
                    continue
                size = self._log_path.stat().st_size
                if size <= self._last_size:
                    if size < self._last_size:
                        self._last_size = 0
                    continue
                with open(self._log_path, "rb") as f:
                    f.seek(self._last_size)
                    new_data = f.read()
                    self._last_size = f.tell()
                for line in new_data.decode(errors="replace").strip().split("\n"):
                    if line.strip():
                        log.write(self._colorize(line))
            except asyncio.CancelledError:
                return
            except Exception as e:
                logger.debug("Log tail error: %s", e)

    @staticmethod
    def _colorize(line: str) -> str:
        # Escape Rich markup in the raw log line
        safe = line.replace("[", "\\[")

        if " ERROR " in line:
            return f"[bold red]{safe}[/]"
        if " WARNING " in line:
            if "bot" in line.lower() or "Bot block" in line:
                return f"[bold yellow on dark_red] {safe} [/]"
            return f"[yellow]{safe}[/]"
        if " INFO " in line:
            if "Escalating" in line:
                return f"[bold magenta]  {safe}[/]"
            if "found" in line and "listings" in line:
                return f"[green]{safe}[/]"
            if "filter call:" in line or "frontier call:" in line:
                return f"[cyan]{safe}[/]"
            if "filter response:" in line or "frontier response:" in line:
                return f"[bold cyan]{safe}[/]"
            if "Loaded new profile" in line or "Reload:" in line:
                return f"[bold green]{safe}[/]"
            if "Daemon started" in line or "Connected to Chrome" in line:
                return f"[bold]{safe}[/]"
            if "executed successfully" in line:
                return f"[dim green]{safe}[/]"
            return f"[dim]{safe}[/]"
        if "DeprecationWarning" in line or "node --trace" in line:
            return ""  # suppress node noise
        return f"[dim]{safe}[/]"
