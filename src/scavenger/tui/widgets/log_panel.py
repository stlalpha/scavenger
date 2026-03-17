import asyncio
import logging
from pathlib import Path
from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import Static, TextArea

logger = logging.getLogger(__name__)

DEFAULT_LOG = Path("~/.local/share/scavenger/daemon.log").expanduser()
MAX_LINES = 500


class LogPanel(Widget):
    """Tails the daemon log file into a selectable, scrolling panel."""

    can_focus = True

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
    LogPanel TextArea {
        height: 1fr;
        background: $surface;
    }
    """

    def __init__(self, log_path: Path = DEFAULT_LOG) -> None:
        super().__init__()
        self._log_path = log_path
        self._tail_task: asyncio.Task | None = None
        self._last_size: int = 0
        self._auto_scroll: bool = True

    def compose(self) -> ComposeResult:
        yield Static("LOG", id="log-header")
        yield TextArea("", read_only=True, show_line_numbers=False, id="log-output")

    def on_mount(self) -> None:
        ta = self.query_one("#log-output", TextArea)
        ta.theme = "monokai"
        self._tail_task = asyncio.create_task(self._tail())

    def on_unmount(self) -> None:
        if self._tail_task:
            self._tail_task.cancel()

    async def _tail(self) -> None:
        """Poll the log file for new content."""
        ta = self.query_one("#log-output", TextArea)

        # Load last 50 lines on startup
        if self._log_path.exists():
            try:
                text = self._log_path.read_text()
                self._last_size = len(text.encode())
                lines = text.strip().split("\n")
                initial = "\n".join(lines[-50:])
                ta.load_text(initial)
                self._scroll_to_end(ta)
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
                        self._last_size = 0
                    continue
                with open(self._log_path, "rb") as f:
                    f.seek(self._last_size)
                    new_data = f.read()
                    self._last_size = f.tell()
                new_lines = new_data.decode(errors="replace").rstrip("\n")
                if new_lines:
                    # Append to end
                    end = ta.document.end
                    ta.insert(f"\n{new_lines}", location=end)
                    # Trim if too long
                    line_count = ta.document.line_count
                    if line_count > MAX_LINES:
                        trim = line_count - MAX_LINES
                        ta.delete(
                            (0, 0),
                            (trim, 0),
                        )
                    if self._auto_scroll:
                        self._scroll_to_end(ta)
            except asyncio.CancelledError:
                return
            except Exception as e:
                logger.debug("Log tail error: %s", e)

    @staticmethod
    def _scroll_to_end(ta: TextArea) -> None:
        end = ta.document.end
        ta.move_cursor(end)
