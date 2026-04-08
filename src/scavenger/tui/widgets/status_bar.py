from datetime import datetime, timezone
from textual.app import ComposeResult
from textual.timer import Timer
from textual.widget import Widget
from textual.widgets import Static

SPIN = "⣾⣽⣻⢿⡿⣟⣯⣷"
SRC_CLR = {"ebay": "#e6db74", "craigslist": "#f92672", "facebook": "#66d9ef"}


def _age(sec: int) -> str:
    if sec < 60:
        return f"{sec}s"
    if sec < 3600:
        return f"{sec // 60}m"
    return f"{sec // 3600}h"


class StatusBar(Widget):
    DEFAULT_CSS = """
    StatusBar {
        dock: bottom;
        height: 1;
        max-height: 1;
        overflow: hidden;
        background: #111;
        color: #75715e;
        padding: 0 1;
    }
    StatusBar #st-l { dock: left; width: auto; max-height: 1; overflow: hidden; }
    StatusBar #st-r { dock: right; width: auto; max-height: 1; overflow: hidden; }
    """

    def __init__(self) -> None:
        super().__init__()
        self.daemon_up: bool = True
        self.new_count: int = 0
        self._last_ts: datetime | None = None
        self._sources: list[dict] = []
        self._active: list[str] = []
        self._poll_interval: int = 0
        self._si: int = 0
        self._st: Timer | None = None
        self._last_text: str = ""

    def compose(self) -> ComposeResult:
        yield Static(self._left(), id="st-l")
        yield Static(self._right(), id="st-r")

    def _spin(self) -> None:
        self._si += 1
        self._refresh()

    def _left(self) -> str:
        p = []
        p.append("[#a6e22e]●[/]" if self.daemon_up else "[#f92672]● down[/]")

        if self._active:
            f = SPIN[self._si % len(SPIN)]
            srcs = " ".join(f"[{SRC_CLR.get(s, '#75715e')} bold]{s[:2]}[/]" for s in self._active)
            p.append(f"[#fd971f]{f}[/] {srcs}")

        if self._sources:
            sp = []
            now = datetime.now(timezone.utc)
            for s in self._sources:
                pid = s["plugin_id"]
                if pid in self._active:
                    continue
                clr = SRC_CLR.get(pid, "#75715e")
                last = s.get("last_polled")
                err = s.get("consecutive_errors", 0)
                if err > 0:
                    sp.append(f"[{clr}]{pid[:2]}[/][#f92672]!{err}[/]")
                elif last:
                    d = int((now - datetime.fromisoformat(last)).total_seconds())
                    sp.append(f"[{clr}]{pid[:2]}[/] [#3a3a3a]{_age(d)}[/]")
            if sp:
                p.append(" ".join(sp))

        if self.new_count > 0:
            p.append(f"[bold #66d9ef]{self.new_count}[/] [#75715e]new[/]")

        if self._last_ts:
            ago = int((datetime.now(timezone.utc) - self._last_ts).total_seconds())
            p.append(f"[#3a3a3a]last {_age(ago)} ago[/]")
            if self._poll_interval > 0 and not self._active:
                remaining = max(0, self._poll_interval - ago)
                if remaining > 0:
                    p.append(f"[#3a3a3a]next {_age(remaining)}[/]")
                else:
                    p.append("[#fd971f]due[/]")

        return " " + " [#3a3a3a]·[/] ".join(p)

    def _right(self) -> str:
        return (
            "[#fd971f]a[/][#555]dd[/] "
            "[#fd971f]e[/][#555]dit[/] "
            "[#fd971f]r[/][#555]epoll[/] "
            "[#fd971f]?[/][#555]help[/] "
            "[#fd971f]q[/][#555]uit[/]"
        )

    def _refresh(self) -> None:
        try:
            text = self._left()
            if text != self._last_text:
                self._last_text = text
                self.query_one("#st-l", Static).update(text)
        except Exception:
            pass

    def set_daemon_status(self, up: bool) -> None:
        self.daemon_up = up
        self._refresh()

    def set_active_polls(self, active: list[str]) -> None:
        was = bool(self._active)
        self._active = active
        if active and not was:
            if not self._st:
                self._st = self.set_interval(0.1, self._spin)
        elif not active and was:
            if self._st:
                self._st.stop()
                self._st = None
        self._refresh()

    def set_last_poll(self, ts: datetime) -> None:
        self._last_ts = ts
        self._refresh()

    def set_source_states(self, states: list[dict]) -> None:
        self._sources = states
        self._refresh()

    def set_poll_interval(self, interval_sec: int) -> None:
        self._poll_interval = interval_sec

    def set_new_count(self, count: int) -> None:
        self.new_count = count
        self._refresh()
