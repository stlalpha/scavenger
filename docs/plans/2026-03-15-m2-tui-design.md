# M2: TUI Shell Design

**Date:** 2026-03-15
**Status:** Approved
**Milestone:** M2 — TUI Shell + KGP Thumbnails

---

## Overview

M2 builds the Textual TUI. The app reads SQLite directly (daemon-independent), polls for new listings every 2 seconds via a background asyncio task, and updates reactively. KGP thumbnail rendering is included for result cards (thumbnails only — detail panel hero images are M3).

---

## Architecture

### Data Flow

```
DataLayer (background asyncio task, polls SQLite every 2s)
    → reactive Listings list
    → reactive ProfileStats dict (unread counts per profile)
    → widgets updated via Textual message passing
```

`DataLayer` is a plain async class owned by the app, not a widget. Widgets never query SQLite directly — they receive data via app-level messages.

### New Files

```
src/scavenger/
├── tui/
│   ├── __init__.py
│   ├── app.py                   # ScavengerApp(App) — root Textual application
│   ├── screens/
│   │   └── main.py              # MainScreen — default view
│   ├── widgets/
│   │   ├── profile_sidebar.py   # left panel — profiles + unread counts
│   │   ├── results_feed.py      # center panel — listing cards
│   │   ├── detail_panel.py      # right panel — full listing detail
│   │   └── status_bar.py        # bottom bar — daemon status + stats
│   └── data.py                  # DataLayer — async polling + reactive state
```

`src/scavenger/main.py` (currently a stub) becomes the real TUI entry point.

---

## Layout

Textual CSS grid, minimum 100 columns wide.

```
┌─────────────────────────────────────────────────────────┐
│ PROFILES          │ RESULTS FEED        │ DETAIL PANEL  │
│ (20%)             │ (40%)               │ (40%)         │
│                   │                     │               │
│ ● Sony Glass  (3) │ ▶ Sony 85mm f/1.4   │ Sony 85mm     │
│ ○ IBM AS/400  (0) │   $249 · eBay · 2m  │ $249.99       │
│ ○ SGI Octane  (1) │                     │               │
│                   │ ▶ Sony A-mount 50mm  │ ★ Zeiss       │
│   [polling]       │   $399 · eBay · 5m  │   variant     │
│                   │                     │               │
│                   │ ○ Sigma 35mm f/1.4  │ Description.. │
│                   │   $200 · CL · 8m    │               │
├───────────────────┴─────────────────────┴───────────────┤
│ daemon: running · last poll: 12s · 3 new today · [?]   │
└─────────────────────────────────────────────────────────┘
```

### Profile Sidebar (left, 20%)

- Each profile: name, unread badge, status dot (● polling / ○ paused / ✕ error)
- Focused profile filters the Results Feed
- `p` → "profile editor coming in future release" notification (stub)

### Results Feed (center, 40%)

- Sorted `first_seen DESC` by default
- Each card: thumbnail (KGP, 8×16 cells), title (truncated), price, source badge, age ("2m", "1h", "3d")
- `★` prefix if `ai_evaluation.notable` is set
- `▶` indicates selected card
- `j`/`↓` and `k`/`↑` navigate

### Detail Panel (right, 40%)

- Full title, price, source, URL
- AI reason (if present) in "AI Notes" section
- AI notable as highlighted badge if present
- Full description
- No images in M2 (M3 adds hero image)
- Action hints shown at bottom

### Status Bar (bottom, 1 line)

- Daemon reachability (green/red)
- Last poll timestamp
- New-today count
- `[?]` key hint

---

## Keybindings

| Key | Action |
|-----|--------|
| `j` / `↓` | Navigate results down |
| `k` / `↑` | Navigate results up |
| `Enter` | Expand detail panel for focused result |
| `o` | Open URL in system browser (`xdg-open`) |
| `s` | Save to watchlist (status → saved) |
| `d` | Dismiss (status → dismissed, hidden from feed) |
| `n` | Snooze (status → snoozed_until, re-surfaces after 1h) |
| `Tab` / `→` | Cycle panel focus forward |
| `Shift+Tab` / `←` | Cycle panel focus backward |
| `r` | Force re-poll focused profile (socket command) |
| `?` | Key binding help overlay |
| `q` | Quit TUI (daemon keeps running) |
| `Q` | Quit TUI + send shutdown to daemon |

`/`, `f` — stubbed with "coming soon" notification in M2.

---

## KGP Thumbnails

- Library: `term-image` (handles KGP detection + encoding)
- Mode: unicode placeholder — rendered into a fixed-size `Static` widget (8 rows × 16 cols)
- Download: async, on first display, into `~/.cache/scavenger/images/` with content-hash filenames
- Fallback: `□` placeholder on download failure or non-KGP terminal, no error shown
- Kitty-native primary target; degrades gracefully elsewhere

---

## DataLayer (`tui/data.py`)

```python
class DataLayer:
    def __init__(self, db: Database): ...

    async def start(self) -> None:
        """Start background polling task."""

    async def stop(self) -> None: ...

    async def get_listings(
        self, profile_id: str | None, limit: int = 100
    ) -> list[Listing]: ...

    async def get_profile_stats(self) -> dict[str, int]:
        """Returns {profile_id: unread_count}."""

    async def mark_status(self, listing_id: str, status: str) -> None:
        """Update listing status (seen, saved, dismissed, snoozed_until:<ts>)."""
```

Polls every 2s. Sends `DataUpdated` message to the app when new listings detected.

---

## Testing Strategy

Textual headless test driver (`App.run_test()`). Behavior tests only — no visual snapshots.

| Test | What it covers |
|------|---------------|
| `test_feed_shows_listings_for_profile` | DataLayer mock → feed renders correct count |
| `test_jk_navigation_updates_selection` | j/k keys move highlight |
| `test_arrow_key_navigation` | ↑/↓ keys move highlight |
| `test_dismiss_updates_db_and_removes_from_feed` | d key → status=dismissed → listing gone |
| `test_open_fires_xdg_open` | o key → xdg-open called with correct URL (mocked) |
| `test_tab_cycles_panel_focus` | Tab advances focus through sidebar → feed → detail |
| `test_status_bar_daemon_unreachable` | socket missing → status bar shows red/unreachable |
| `test_detail_panel_shows_ai_notes` | listing with ai_evaluation → reason shown in detail |
| `test_notable_badge_in_feed` | listing with notable → ★ shown in card |

---

## Dependencies to Add

```toml
# pyproject.toml
"textual>=0.60",
"term-image>=0.7",
```
