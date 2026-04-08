"""Draggable splitter bars for resizing adjacent panes."""
from textual.widget import Widget
from textual.events import MouseDown, MouseMove, MouseUp


class VSplitter(Widget):
    """Vertical splitter — drag left/right to resize siblings."""

    can_focus = False

    def render(self) -> str:
        return ""

    DEFAULT_CSS = """
    VSplitter {
        width: 1;
        max-width: 1;
        height: 100%;
        background: #222;
    }
    VSplitter:hover {
        background: #555;
    }
    VSplitter.-dragging {
        background: #fd971f;
    }
    """

    def __init__(self, left_id: str, right_id: str, **kwargs) -> None:
        super().__init__(**kwargs)
        self._left_id = left_id
        self._right_id = right_id
        self._dragging = False
        self._start_x: int = 0
        self._initial_left: int = 0

    def on_mouse_down(self, event: MouseDown) -> None:
        try:
            left = self.screen.query_one(f"#{self._left_id}")
        except Exception:
            return
        self._dragging = True
        self._start_x = event.screen_x
        self._initial_left = left.size.width
        self.add_class("-dragging")
        self.capture_mouse()
        event.stop()
        event.prevent_default()

    def on_mouse_move(self, event: MouseMove) -> None:
        if not self._dragging:
            return
        event.stop()
        event.prevent_default()
        dx = event.screen_x - self._start_x
        if dx == 0:
            return
        new_left = self._initial_left + dx
        if new_left < 14:
            return
        try:
            left = self.screen.query_one(f"#{self._left_id}")
            right = self.screen.query_one(f"#{self._right_id}")
        except Exception:
            return
        remaining = self.parent.size.width - new_left - 1
        if remaining < 20:
            return
        left.styles.width = new_left
        right.styles.width = "1fr"

    def on_mouse_up(self, event: MouseUp) -> None:
        if self._dragging:
            self._dragging = False
            self.remove_class("-dragging")
            self.release_mouse()
            event.stop()


class HSplitter(Widget):
    """Horizontal splitter — drag up/down to resize siblings."""

    can_focus = False

    def render(self) -> str:
        return ""

    DEFAULT_CSS = """
    HSplitter {
        height: 1;
        max-height: 1;
        width: 100%;
        background: #222;
    }
    HSplitter:hover {
        background: #555;
    }
    HSplitter.-dragging {
        background: #fd971f;
    }
    """

    def __init__(self, top_id: str, bottom_id: str, **kwargs) -> None:
        super().__init__(**kwargs)
        self._top_id = top_id
        self._bottom_id = bottom_id
        self._dragging = False
        self._start_y: int = 0
        self._initial_bottom: int = 0

    def on_mouse_down(self, event: MouseDown) -> None:
        try:
            bottom = self.screen.query_one(f"#{self._bottom_id}")
        except Exception:
            return
        self._dragging = True
        self._start_y = event.screen_y
        self._initial_bottom = bottom.size.height
        self.add_class("-dragging")
        self.capture_mouse()
        event.stop()
        event.prevent_default()

    def on_mouse_move(self, event: MouseMove) -> None:
        if not self._dragging:
            return
        event.stop()
        event.prevent_default()
        dy = event.screen_y - self._start_y
        if dy == 0:
            return
        new_bottom = self._initial_bottom - dy
        if new_bottom < 3:
            return
        try:
            top = self.screen.query_one(f"#{self._top_id}")
            bottom = self.screen.query_one(f"#{self._bottom_id}")
        except Exception:
            return
        remaining = self.parent.size.height - new_bottom - 1
        if remaining < 5:
            return
        top.styles.height = "1fr"
        bottom.styles.height = new_bottom

    def on_mouse_up(self, event: MouseUp) -> None:
        if self._dragging:
            self._dragging = False
            self.remove_class("-dragging")
            self.release_mouse()
            event.stop()
