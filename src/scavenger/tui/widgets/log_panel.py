import asyncio
import logging
from pathlib import Path
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import Static, RichLog

logger = logging.getLogger(__name__)

DEFAULT_LOG = Path("~/.local/share/scavenger/daemon.log").expanduser()


class LogPanel(Widget):
    """Tails the daemon log file and streams it into a scrolling panel."""

    DEFAULT_CSS = """
    LogPanel {
        width: 100%;
        height: 100%;
        background: $surface;
    }
    LogPanel #log-header {
        dock: top;
        height: 3;
        padding: 1 1 0 1;
        color: $text-muted;
        text-style: bold;
        background: $surface;
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
        yield Static("LOG", id="log-header")
        yield RichLog(highlight=True, markup=False, wrap=True, max_lines=500, id="log-output")

    def on_mount(self) -> None:
        self._tail_task = asyncio.create_task(self._tail())

    def on_unmount(self) -> None:
        if self._tail_task:
            self._tail_task.cancel()

    async def _tail(self) -> None:
        """Poll the log file for new content."""
        log = self.query_one("#log-output", RichLog)

        # Load last 30 lines on startup
        if self._log_path.exists():
            try:
                text = self._log_path.read_text()
                self._last_size = len(text.encode())
                lines = text.strip().split("\n")
                for line in lines[-30:]:
                    log.write(self._colorize(line))
            except Exception:
                pass

        while True:
            await asyncio.sleep(1.0)
            try:
                if not self._log_path.exists():
                    continue
                size = self._log_path.stat().st_size
                if size <= self._last_size:
                    if size < self._last_size:
                        # File was truncated/rotated
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
        """Add Rich markup color based on log level."""
        if " ERROR " in line:
            return f"[bold red]{line}[/]"
        if " WARNING " in line:
            return f"[yellow]{line}[/]"
        if " INFO " in line and ("found" in line or "Escalating" in line):
            return f"[green]{line}[/]"
        if "bot detection" in line.lower() or "Bot block" in line:
            return f"[bold yellow]{line}[/]"
        return f"[dim]{line}[/]"
