# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

Scavenger is a continuous web intelligence terminal — a daemon+TUI system that polls marketplace sites (eBay, Craigslist) for listings matching user-defined interest profiles, scores them with keyword matching and optional local LLM evaluation, stores results in SQLite, and presents them in a Textual-based terminal UI.

## Commands

```bash
# Install (uses uv)
uv sync --all-extras

# Run tests
uv run pytest

# Run a single test
uv run pytest tests/test_scoring.py::test_score_exact

# Run the TUI (requires config at ~/.config/scavenger/config.toml)
uv run scavenger

# Start the background daemon
uv run scavenger-ctl start

# Daemon control
uv run scavenger-ctl status|stop|poll <profile>|list-profiles

# Scraping requires Chrome with remote debugging
google-chrome-stable --remote-debugging-port=9222 --user-data-dir="$HOME/Library/Application Support/scavenger/chrome" &
```

## Architecture

**Two processes, shared DB:**
- **Daemon** (`scavenger-ctl start`) — polls sources on schedule, scores and evaluates listings, writes to SQLite. Listens on a Unix domain socket for commands from `scavenger-ctl` and the TUI.
- **TUI** (`scavenger`) — reads from the same SQLite DB on a 2-second poll loop. Sends commands to the daemon over its socket (re-poll, shutdown).

**Plugin system:** `plugins/base.py` defines a `Protocol` — plugins implement `fetch(profile) -> list[Listing]` and `supports_geo() -> bool`. Current plugins: `ebay`, `craigslist`. Both use Playwright CDP against a real Chrome session (not headless) to avoid bot detection.

**Scoring pipeline (daemon):**
1. Plugin fetches raw listings
2. `db.get_existing_ids()` bulk-skips known listings
3. `scoring.score_listing()` does keyword matching (all keywords must match somewhere; title hits weighted higher). Returns 0-100; listings scoring 0 are dropped.
4. Optional AI evaluation via local Ollama (`ai/evaluator.py`) — batch mode, background worker queue. Irrelevant listings filtered out. Falls back to passthrough on any error.

**Config:** TOML at `~/.config/scavenger/config.toml`. Sections: `[global]` (db path, socket path, log level, home_zip), `[[profiles]]` (keywords, sources, price range, poll interval), `[ai]` (model, escalation settings).

**Key models:** `Listing` (pydantic, statuses: new/seen/saved/dismissed/snoozed), `Profile` (search config with keyword groups — list items are OR'd within a group, all groups must match).

## Testing

- `asyncio_mode = "auto"` — async tests just work, no decorator needed
- Tests use `respx` for HTTP mocking and `aiosqlite` in-memory DBs
- `pytest-asyncio` for async fixture/test support
- No Playwright in tests — plugin tests mock at the HTTP/page level

## Key Conventions

- Listing IDs are content hashes of the URL (`dedup.content_hash`)
- AI evaluation always falls back to passthrough (never drops a listing due to model errors)
- Plugins must close pages in `finally` blocks — never close the browser context (it's the user's live Chrome)
- Profile keywords support OR-groups: `["thing"]` matches literally, `[["variant1", "variant2"]]` matches any variant. All top-level keyword entries must match.
- Secrets are SOPS-only: `ANTHROPIC_API_KEY` comes from the environment (`sops exec-env`) or `~/.config/scavenger/secrets.sops.yaml` (age-encrypted, `sops edit` to change). Plaintext `~/.config/scavenger/.env` is ignored with a warning — never add plaintext key files or plaintext keys in config.toml. `SCAVENGER_SECRETS_FILE` overrides the secrets path (tests use it with a fake `sops` shim).

<!-- code-graph-mcp:begin v2 -->
## Code Graph (repo-wide AST index)

AST + FTS + vector index of the whole repo — prefer over multi-round Grep/Read for
structural queries (LSP only sees open files; this sees everything). Fastest path = Bash CLI:

| Intent | Command |
|--------|---------|
| Who calls X / what X calls | `code-graph-mcp callgraph X` |
| Impact before editing a fn | `code-graph-mcp impact X` |
| Unfamiliar dir / module | `code-graph-mcp overview <dir>` |
| Symbol source / signature | `code-graph-mcp show X` |
| Concept search (no exact name) | `code-graph-mcp search "…"` (vector: MCP `semantic_code_search`) |
| grep + AST context | `code-graph-mcp grep "pat" [paths] [-t lang] [-g glob] [-c]` |

Still use Grep for literal strings/regex in non-code files; still Read files you'll edit.
Full command + MCP-tool table: `.claude/plugin_code_graph_mcp.md`
<!-- code-graph-mcp:end -->
