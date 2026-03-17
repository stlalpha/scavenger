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
    """Tails the daemon log with monokai-colored output."""

    can_focus = True

    DEFAULT_CSS = """
    LogPanel {
        width: 100%;
        height: 100%;
        background: #1a1a1a;
        border-top: solid #333;
    }
    LogPanel #log-hdr {
        dock: top;
        height: 1;
        padding: 0 1;
        background: #252525;
        color: #75715e;
    }
    LogPanel RichLog {
        height: 1fr;
        padding: 0 1;
        background: #1a1a1a;
        scrollbar-size: 1 1;
    }
    """

    def __init__(self, log_path: Path = DEFAULT_LOG) -> None:
        super().__init__()
        self._log_path = log_path
        self._tail_task: asyncio.Task | None = None
        self._last_size: int = 0

    def compose(self) -> ComposeResult:
        yield Static("╶ log", id="log-hdr")
        yield RichLog(highlight=False, markup=True, wrap=True, max_lines=MAX_LINES, id="log-out")

    def on_mount(self) -> None:
        self._tail_task = asyncio.create_task(self._tail())

    def on_unmount(self) -> None:
        if self._tail_task:
            self._tail_task.cancel()

    async def _tail(self) -> None:
        rl = self.query_one("#log-out", RichLog)

        if self._log_path.exists():
            try:
                text = self._log_path.read_text()
                self._last_size = len(text.encode())
                for line in text.strip().split("\n")[-40:]:
                    rl.write(self._c(line))
            except Exception:
                pass

        while True:
            await asyncio.sleep(0.8)
            try:
                if not self._log_path.exists():
                    continue
                sz = self._log_path.stat().st_size
                if sz <= self._last_size:
                    if sz < self._last_size:
                        self._last_size = 0
                    continue
                with open(self._log_path, "rb") as f:
                    f.seek(self._last_size)
                    new = f.read()
                    self._last_size = f.tell()
                for line in new.decode(errors="replace").strip().split("\n"):
                    if line.strip():
                        rl.write(self._c(line))
            except asyncio.CancelledError:
                return
            except Exception as e:
                logger.debug("Log tail error: %s", e)

    @staticmethod
    def _c(line: str) -> str:
        s = line.replace("[", "\\[")
        if "DeprecationWarning" in line or "node --trace" in line:
            return ""
        if " ERROR " in line:
            return f"[bold #f92672]{s}[/]"
        if " WARNING " in line:
            if "bot" in line.lower() or "Bot block" in line:
                return f"[bold #f92672 on #3a1a1a] ▸ {s}[/]"
            return f"[#fd971f]{s}[/]"
        if " INFO " in line:
            if "Escalating" in line:
                return f"[#f92672]  ★ {s}[/]"
            if "found" in line and "listings" in line:
                return f"[#a6e22e]{s}[/]"
            if "filter call:" in line or "frontier call:" in line:
                return f"[#66d9ef]{s}[/]"
            if "filter response:" in line or "frontier response:" in line:
                return f"[bold #66d9ef]{s}[/]"
            if "Loaded new profile" in line or "Reload:" in line:
                return f"[bold #a6e22e]{s}[/]"
            if "Daemon started" in line or "Connected to Chrome" in line:
                return f"[#f8f8f2]{s}[/]"
            if "executed successfully" in line:
                return f"[#3a3a3a]{s}[/]"
            return f"[#75715e]{s}[/]"
        return f"[#3a3a3a]{s}[/]"
