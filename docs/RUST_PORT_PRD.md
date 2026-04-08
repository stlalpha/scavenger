# Scavenger Rust Port — Product Requirements Document

## Goal

Port Scavenger from Python to Rust. Ship a single static binary that combines the daemon, TUI, and CLI. No runtime dependencies, no Python, no venv, no uv. Just `./scavenger`.

## Why

- **Startup**: Python cold-starts are slow. The TUI takes seconds to appear. Rust starts instantly.
- **Memory**: Python process sits at 100MB+. A Rust binary doing the same work will use 10-20MB.
- **Distribution**: One binary. `curl | tar` install. No Python version conflicts, no broken venvs, no dependency hell.
- **Concurrency**: The daemon runs multiple scrapers, AI evaluators, and a scheduler concurrently. Rust's async model (tokio) is purpose-built for this. No GIL.
- **Chrome leakage**: The Python Playwright bridge spawns a Node subprocess that leaks console output from the Chrome session (the Facebook token dump problem). A direct CDP implementation in Rust controls exactly what gets logged.

## What Stays the Same

- TOML config format and location (`~/.config/scavenger/config.toml`) — existing configs must work unchanged
- SQLite database schema and location (`~/.local/share/scavenger/scavenger.db`) — existing DBs must be readable
- Unix domain socket protocol (JSON-over-newline) between daemon and TUI/ctl
- Scoring algorithm (keyword groups with AND/OR semantics, negative keywords, price range scoring)
- AI evaluation pipeline: local Ollama for filter pass, Anthropic API for escalation
- Listing dedup via SHA-256 of normalized URL
- Chrome CDP scraping model (connect to user's real Chrome session on port 9222)

## Architecture

### Single Binary, Three Modes

```
scavenger              # launch TUI (default)
scavenger daemon       # start background daemon
scavenger ctl status   # daemon control commands
scavenger ctl stop
scavenger ctl poll <profile>
scavenger ctl list-profiles
```

Subcommand routing via `clap`. No separate `scavenger-ctl` binary.

### Crate Structure

```
scavenger/
├── Cargo.toml
├── src/
│   ├── main.rs              # clap dispatch
│   ├── config.rs            # TOML parsing (toml crate)
│   ├── models.rs            # Listing, Profile structs (serde)
│   ├── db.rs                # SQLite via rusqlite (sync) or sqlx (async)
│   ├── dedup.rs             # URL normalization + SHA-256
│   ├── scoring.rs           # keyword matching + scoring
│   ├── daemon/
│   │   ├── mod.rs           # Daemon orchestrator
│   │   ├── scheduler.rs     # tokio-based poll scheduler
│   │   └── socket.rs        # Unix domain socket server
│   ├── plugins/
│   │   ├── mod.rs           # Plugin trait
│   │   ├── browser.rs       # Chrome CDP client (chromiumoxide or direct)
│   │   ├── ebay.rs
│   │   ├── craigslist.rs
│   │   └── facebook.rs
│   ├── ai/
│   │   ├── mod.rs           # Evaluator with worker pool
│   │   ├── prompts.rs       # Prompt templates
│   │   └── models.rs        # AIEvaluation, AIConfig
│   └── tui/
│       ├── mod.rs           # App struct, event loop
│       ├── widgets/
│       │   ├── profile_sidebar.rs
│       │   ├── results_feed.rs
│       │   ├── detail_panel.rs
│       │   ├── log_panel.rs
│       │   ├── status_bar.rs
│       │   ├── splitter.rs
│       │   └── thumbnail.rs
│       └── screens/
│           ├── main.rs
│           └── add_profile.rs
```

### Key Dependencies

| Python | Rust Replacement | Notes |
|--------|-----------------|-------|
| Textual | ratatui + crossterm | Terminal UI framework |
| Playwright (CDP) | chromiumoxide or direct websocket CDP | No Node.js subprocess |
| aiosqlite | rusqlite (with bundled SQLite) or sqlx-sqlite | Compiles SQLite into the binary |
| httpx | reqwest | HTTP client with connection pooling |
| litellm (Ollama) | reqwest to Ollama's OpenAI-compatible API directly | No abstraction layer needed |
| litellm (Anthropic) | anthropic-rs or direct reqwest to API | |
| pydantic | serde + serde_json | Serialization/validation |
| tomllib | toml (serde) | TOML parsing |
| click | clap | CLI argument parsing |
| APScheduler | tokio::time + custom scheduler | Interval-based job scheduling |
| Pillow (images) | image crate | Thumbnail processing |
| beautifulsoup4 | scraper (CSS selectors on HTML) | HTML parsing for detail pages |

### CDP / Browser Integration

The Python version uses Playwright's Node.js bridge to talk Chrome DevTools Protocol. This is the source of the console leakage and adds ~100MB of Node runtime.

Rust approach:
- Use `chromiumoxide` crate for CDP over websocket
- Connect to `http://localhost:9222` same as now
- Direct control over what console output gets captured vs discarded
- Page lifecycle: open tab, navigate, query DOM, extract data, close tab
- Stealth: set `navigator.webdriver = false` via CDP `Page.addScriptToEvaluateOnNewDocument`

### TUI

Ratatui replaces Textual. The current layout:

```
┌──────────┬─────────────────┬──────────────────┐
│ PROFILES │    LISTINGS     │     DETAIL       │
│ (sidebar)│    (feed)       │    (panel)       │
│          │                 │                  │
├──────────┴─────────────────┴──────────────────┤
│                    LOG                         │
├────────────────────────────────────────────────┤
│                 STATUS BAR                     │
└────────────────────────────────────────────────┘
```

Key TUI behaviors to preserve:
- Resizable splitters (mouse drag)
- Inline image rendering (iTerm2/Kitty/Sixel protocols via ratatui-image)
- Profile switching with unread counts
- Listing cards with status-dependent styling (new/seen/saved/dismissed)
- Detail panel with hero image, AI evaluation display, keybind hints
- Status bar with active poll indicators and keybind reference
- Log panel showing daemon activity
- Keyboard navigation (j/k, enter, s/d/a, o to open browser, q to quit)
- Dark tonal palette (#111 through #1e1e1e)

### AI Evaluation Pipeline

```
                    ┌─────────────┐
  listings ────────►│ Worker Pool │──────► results
  (batch of 10)     │ (3 workers) │
                    │             │
                    │ Each worker:│
                    │  POST to    │
                    │  Ollama API │
                    │  /v1/chat/  │
                    │  completions│
                    └──────┬──────┘
                           │
                    escalation keywords match?
                           │
                    ┌──────▼──────┐
                    │  Anthropic  │
                    │  Haiku API  │
                    └─────────────┘
```

- Hit Ollama's OpenAI-compatible endpoint directly with reqwest (no litellm abstraction)
- Parse JSON responses with serde
- Worker pool via `tokio::sync::Semaphore` (simpler than queue+workers)
- Escalation calls to Anthropic API with 1s delay between calls

### Image Handling

- Thumbnail cache at `~/.cache/scavenger/images/` (same location)
- Download with reqwest, persistent client with connection pooling
- In-memory LRU cache (128 entries) for resolved URL→path mappings
- Terminal image display via `ratatui-image` (supports iTerm2, Kitty, Sixel)
- Cache eviction: 30-day TTL on disk, LRU in memory

## Migration Strategy

### Phase 1: Core + Daemon (no TUI)

Build the daemon first. This is where the performance matters most and has the cleanest boundaries.

1. `models.rs` — Listing, Profile structs with serde
2. `config.rs` — TOML config loading, same format
3. `dedup.rs` — URL normalization + SHA-256
4. `scoring.rs` — keyword matching (port the regex logic)
5. `db.rs` — SQLite with rusqlite, same schema
6. `plugins/browser.rs` — CDP connection to Chrome
7. `plugins/ebay.rs`, `craigslist.rs`, `facebook.rs` — scraping logic
8. `ai/` — Ollama + Anthropic API clients, prompt templates, worker pool
9. `daemon/` — scheduler, socket server, orchestrator
10. `main.rs` — clap CLI for `daemon` and `ctl` subcommands

Deliverable: `scavenger daemon` and `scavenger ctl` work identically to `scavenger-ctl start/stop/status/poll`.

### Phase 2: TUI

Port the terminal interface. This is the larger surface area but can reference the working daemon.

1. App skeleton with ratatui + crossterm event loop
2. Layout: sidebar, feed, detail, log, status bar
3. Splitter widgets with mouse drag
4. Profile sidebar with selection and unread counts
5. Results feed with listing cards
6. Detail panel with image display
7. Log panel reading daemon output
8. Status bar with keybinds
9. DB polling loop (2-second refresh)
10. Socket commands to daemon (re-poll, shutdown)

Deliverable: `scavenger` launches the TUI, reads from the same DB, talks to daemon over socket.

### Phase 3: Profile Management TUI

The add/edit profile screen. Lower priority — can use config file editing initially.

### Phase 4: Polish

- Cross-compile for Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64
- GitHub Actions CI for builds + releases
- Shell completion generation (clap)
- Man page generation
- Config migration tool (validate existing TOML works)

## Performance Targets

| Metric | Python (current) | Rust (target) |
|--------|------------------|---------------|
| Binary size | ~500MB (venv) | <15MB |
| Cold start (TUI) | 2-3s | <100ms |
| Memory (daemon idle) | ~120MB | <20MB |
| Memory (TUI) | ~150MB | <30MB |
| Batch eval (10 listings) | sequential through litellm | 3 concurrent direct HTTP |
| DB query (100 listings) | ~50ms (aiosqlite) | <5ms (rusqlite) |

## Non-Goals

- Plugin API for third-party scrapers (can add later, not in v1)
- Windows support (Chrome CDP on Windows is a different beast)
- Headless browser bundling (still requires user's Chrome)
- GUI (stays terminal-only)
- Config file format changes (TOML stays TOML)

## Risks

- **chromiumoxide maturity**: The crate is maintained but not as battle-tested as Playwright. May need to fall back to raw CDP websocket messages for some operations. Mitigation: the DOM queries we need are simple (querySelector, innerText, getAttribute).
- **ratatui vs Textual**: Textual is higher-level (CSS-like styling, built-in widgets). Ratatui is lower-level but more performant. The TUI port will require more manual layout code. Mitigation: the current TUI layout is well-defined and stable.
- **Image rendering in terminal**: `ratatui-image` supports fewer protocols than what Textual's Rich does. Need to verify iTerm2 support works correctly. Mitigation: fall back to no-image mode gracefully.
- **Ollama API compatibility**: Currently goes through litellm which handles quirks. Direct API calls may surface edge cases. Mitigation: Ollama's OpenAI-compatible endpoint is well-documented and stable.

## Success Criteria

- All existing TOML configs parse without modification
- Existing SQLite databases are readable (no migration needed)
- All current keyboard shortcuts work identically
- Visual appearance matches the current TUI design
- Daemon discovers and scores listings at least as reliably as Python version
- Single binary, zero runtime dependencies beyond Chrome
