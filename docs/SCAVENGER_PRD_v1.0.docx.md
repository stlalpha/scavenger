

**SCAVENGER**

Continuous Web Intelligence Terminal

Product Requirements Document

Version 1.0  •  March 2026  •  DRAFT

# **1\. Executive Summary**

SCAVENGER is a terminal-native, continuously-running web intelligence application that monitors the internet for items of personal interest to the user, surfacing relevant listings, posts, and articles in real time with rich inline image previews. Built as a first-class TUI (Text User Interface) with native Kitty Graphics Protocol support, it presents discovered content with the immediacy of a live dashboard and the depth of a research tool — without ever leaving the terminal.

The application is designed for users who have persistent, highly-specific collecting or tracking interests — vintage cameras, retro hardware, niche vehicles, specialist parts — and who lose opportunities because they are not monitoring the right sources at the right time. SCAVENGER runs as a persistent background process, polling configurable sources on configurable schedules, and alerts the user in-terminal as well as via optional system notifications.

# **2\. Problem Statement**

## **2.1 The Opportunity Gap**

Rare and collectible items — vintage Sony A-mount glass, AS/400 hardware, specific vehicle trim levels — surface briefly on eBay, Craigslist, regional classifieds, forums, and auction sites before being claimed. The committed collector must either manually check dozens of sources multiple times per day or rely on coarse saved-search email alerts that lack context, images, or cross-source aggregation.

## **2.2 Existing Solution Failures**

* Email alert systems (eBay, Craigslist) have high latency and no image preview

* Browser-based dashboards require context switching away from the terminal workflow

* RSS aggregators lack visual context and require per-source setup outside a unified config

* No existing TUI tool integrates image display natively for listing-browsing workflows

| CONTEXT: Target user: power users who live in the terminal, have multiple active collecting interests, and need a persistent ambient monitoring layer that does not interrupt flow but surfaces signals the moment they appear. |
| :---- |

# **3\. Goals and Non-Goals**

## **3.1 Goals**

* Continuously poll user-defined sources for matches against user-defined interest profiles

* Display results in a rich, navigable TUI with inline image rendering via Kitty Graphics Protocol

* Support Kitty terminal natively and degrade gracefully (sixel, block characters) in others

* Allow users to define interests as named profiles combining keywords, price bands, condition flags, and source lists

* Deliver in-terminal alerts for new matches without requiring the user to be actively viewing the app

* Provide one-key actions: open in browser, save to watchlist, dismiss, snooze

* Run as a persistent tmux/screen-compatible background process

* Export and sync interest profiles and match history across machines via a single TOML config

## **3.2 Non-Goals**

* Automated purchasing or bid submission

* A GUI or web front-end — this is terminal-only by design

* General-purpose web browsing or scraping at arbitrary scale

* Mobile client (terminal access via SSH is acceptable)

# **4\. User Personas**

| Attribute | Profile |
| :---- | :---- |
| Name | The Terminal Collector |
| Environment | CachyOS / macOS daily driver; Kitty terminal; tmux sessions; Proxmox home lab |
| Interests | Vintage Sony A-mount cameras/lenses, IBM AS/400 / iSeries hardware, retro SGI workstations, specific vehicle models, dressage photography equipment |
| Pain points | Loses eBay listings within hours; Craigslist posts disappear before she sees them; email alerts lack images and context; too many sources to monitor manually |
| Success metric | Catches 90%+ of relevant listings within 15 minutes of posting; buys at least one item per month that would otherwise have been missed |

# **5\. Feature Specifications**

## **5.1 Interest Profile System**

Interest profiles are the core configuration primitive. Each profile is a named TOML block defining what to hunt and where.

### **5.1.1 Profile Schema**

| Field | Type | Description |
| :---- | :---- | :---- |
| name | string | Human-readable label shown in TUI |
| keywords | string\[\] | Required match terms; supports AND/OR/NOT logic and regex |
| negative\_keywords | string\[\] | Terms that disqualify a result (e.g. "broken", "parts only") |
| sources | string\[\] | Source plugin IDs to query for this profile |
| price\_min / price\_max | float? | Optional USD price band filter |
| condition | enum\[\]? | new | used | for\_parts | any |
| location\_radius\_mi | int? | For geo-capable sources; restrict to radius from home ZIP |
| poll\_interval\_sec | int | Per-profile override of global polling interval |
| alert\_priority | high | normal | low | Controls notification urgency and sort rank |
| enabled | bool | Toggle profile without deleting it |
| tags | string\[\]? | Arbitrary tags for filtering the results view |

## **5.2 Source Plugin Architecture**

Sources are discrete, independently-maintained plugin modules. The core ships with a standard library of plugins; users may add custom plugins via the plugin directory.

### **5.2.1 Bundled Source Plugins (v1.0)**

| Plugin ID | Source | Notes |
| :---- | :---- | :---- |
| ebay | eBay | RSS feed \+ unofficial search API; price/condition filters; image extraction |
| craigslist | Craigslist | RSS per-city per-category; geo radius support; rotating city list |
| facebook\_marketplace | FB Marketplace | Playwright-based headless scrape; requires FB credentials in secrets store |
| hifi\_shark | HiFiShark | Audio/electronics aggregator; good for vintage gear |
| usedphotopro | UsedPhotoPro | Camera-specific marketplace |
| mpb | MPB | Camera gear secondary market; structured condition grading |
| reddit\_rss | Reddit | Subreddit RSS for classified subs (r/photomarket, r/synths, etc.) |
| lemon\_squeezy | Bring a Trailer | Vehicle auction listings; keyword \+ make/model structured search |
| autotrader | AutoTrader | Vehicle classifieds; structured make/model/year/price filters |
| hibid | HiBid | Estate and auction aggregator; strong for retro hardware |
| generic\_rss | Any RSS/Atom feed | User-supplied URL; keyword filtering applied post-fetch |

| ARCH: Plugin interface: each plugin implements fetch(profile) \-\> \[\]Listing. The core handles scheduling, deduplication, and storage. Plugins are single Python files dropped into \~/.config/scavenger/plugins/. |
| :---- |

## **5.3 TUI Layout and Navigation**

The TUI is built with Textual (Python) as the primary framework. The layout adapts to terminal dimensions with a minimum supported width of 100 columns.

### **5.3.1 Screen Layout**

| Panel | Description |
| :---- | :---- |
| Profile Sidebar (left) | Collapsible list of all interest profiles with live unread counts and status indicator (polling / paused / error) |
| Results Feed (center) | Chronological or score-ranked list of matched listings; each card shows title, price, source badge, age, and thumbnail |
| Detail Panel (right) | Full listing detail: large inline image(s) via Kitty protocol, full description, pricing history if available, direct URL, action buttons |
| Status Bar (bottom) | Global poll status, last-seen timestamp per source, match rate stats, key binding cheatsheet |
| Alert Toast (overlay) | Non-blocking ephemeral overlay when a high-priority match arrives; auto-dismisses after configurable timeout |

### **5.3.2 Key Bindings**

| Key | Action |
| :---- | :---- |
| j / k | Navigate results up/down (vim-style) |
| Enter | Expand detail panel for focused result |
| o | Open listing URL in system browser |
| s | Save to watchlist |
| d | Dismiss / mark seen |
| n | Snooze — re-surface listing after configurable interval |
| f | Filter feed by profile, source, price, or tag |
| /  | Full-text search within cached results |
| p | Open profile editor |
| r | Force immediate re-poll of focused profile |
| Tab | Cycle between panel focus |
| ? | Show full key binding help overlay |
| q | Quit (daemon continues running) |
| Q | Quit and stop daemon |

## **5.4 Kitty Graphics Protocol Integration**

Image display is a first-class feature. When running inside Kitty, SCAVENGER uses the Kitty Graphics Protocol (KGP) for pixel-perfect inline image rendering. In other terminals, it attempts sixel output, then falls back to half-block Unicode approximation.

### **5.4.1 Image Rendering Pipeline**

* On listing fetch, the source plugin extracts the primary image URL

* Images are downloaded asynchronously and cached to \~/.cache/scavenger/images/ with content-hash filenames

* At render time, terminal type is detected via $TERM and terminfo capabilities

* For Kitty: images are encoded as PNG and transmitted via the KGP chunked transfer protocol using the unicode placeholder method for layout-stable rendering inside Textual widgets

* Image dimensions are computed from available cell dimensions using Kitty's pixel-per-cell query (\\x1b\[14t)

* Thumbnail in result card: constrained to 8 rows × 16 columns of terminal cells

* Detail panel hero image: fills available panel width up to 60 columns

* Multiple listing images: horizontal scroll strip below hero

| IMPL: KGP unicode placeholder mode is required when embedding images inside Textual widget trees, as direct terminal writes conflict with Textual's rendering cycle. This approach has been validated with Textual 0.60+. |
| :---- |

### **5.4.2 Terminal Compatibility Matrix**

| Terminal | Image Mode | Notes |
| :---- | :---- | :---- |
| Kitty | KGP (native) | Full fidelity; pixel-perfect; primary target |
| WezTerm | KGP (compatible) | KGP support via WezTerm's implementation; tested |
| iTerm2 | iTerm2 inline protocol | Detected via TERM\_PROGRAM; separate code path |
| Foot, mlterm, etc. | Sixel | Capability detected via terminfo; quality varies |
| Any other | Unicode half-blocks | Very low fidelity; color approximation only |

## **5.5 Alerting and Notification System**

SCAVENGER operates in a continuous background mode. Alerting must work whether the user has the TUI open or not.

* In-TUI alerts: toast overlay with listing thumbnail, title, price, and source; keybinding to jump to listing

* System notifications: libnotify (Linux) / osascript (macOS) with listing title and price; clicking opens the TUI focused on that listing

* Sound alerts: optional; configurable per-priority level via mpv/paplay

* Urgent keyword escalation: certain keywords (e.g. "AS/400", "A99") can be flagged as escalation triggers, promoting a normal-priority match to high-priority

* Quiet hours: configurable time window during which notifications are suppressed but matches continue to accumulate

* Alert deduplication: a listing seen once does not re-alert unless its price drops below a configured threshold

## **5.6 Daemon Architecture**

The poller runs as a detachable background daemon, separate from the TUI process, communicating via a local Unix socket.

| Component | Responsibility |
| :---- | :---- |
| scavenger-daemon | Polling scheduler, source plugin executor, deduplication, SQLite write, notification dispatch |
| scavenger (TUI) | Reads from SQLite, subscribes to daemon push events over Unix socket, renders UI |
| SQLite DB | Persistent store for listings, watchlist, dismissed items, image cache metadata |
| scavenger-ctl | CLI control interface: start/stop daemon, list profiles, trigger poll, export watchlist |

| ARCH: The daemon is managed by a systemd user unit (Linux) or launchd plist (macOS). scavenger-ctl install writes the appropriate service file and enables it. |
| :---- |

## **5.7 Deduplication and Match Quality**

* Content-hash deduplication prevents the same listing from appearing twice regardless of source

* URL normalization strips tracking parameters before hashing

* Cross-source deduplication: same item listed on eBay and Craigslist surfaces as a single result with multiple source badges

* Relevance scoring: matches are scored 0–100 based on keyword density, field match (title \> description), price band fit, and recency; configurable sort weight

* False-positive suppression: user can flag a result as irrelevant; SCAVENGER learns negative patterns per profile using lightweight term frequency heuristics

## **5.8 Watchlist and History**

* Saved items persist indefinitely in the SQLite store

* Watchlist view (w key) shows saved items with current availability status re-checked on demand

* Price history graph (ASCII sparkline) shown for listings that have been seen multiple times

* Export watchlist to CSV, JSON, or Markdown table via scavenger-ctl export

* Configurable match history retention (default: 30 days, unlimited for watchlisted items)

# **6\. Configuration**

## **6.1 Config File Location**

Primary config: \~/.config/scavenger/config.toml. Secrets (API keys, credentials) stored separately in \~/.config/scavenger/secrets.toml with 0600 permissions, or optionally in the system keyring.

## **6.2 Example Profile Block**

| \[\[profiles\]\] name \= "Sony A-mount Glass" keywords \= \["sony", "a-mount", \["85mm", "135mm", "300mm"\]\] negative\_keywords \= \["broken", "parts", "cracked", "fungus"\] sources \= \["ebay", "usedphotopro", "mpb", "reddit\_rss"\] price\_min \= 50.0 price\_max \= 800.0 alert\_priority \= "high" poll\_interval\_sec \= 900 escalation\_keywords \= \["A99", "A mount"\] enabled \= true |
| :---- |

# **7\. Technical Architecture**

## **7.1 Technology Stack**

| Layer | Technology | Rationale |
| :---- | :---- | :---- |
| TUI Framework | Textual (Python) | Async-native, widget system, rich layout engine, active development |
| Graphics | Kitty Graphics Protocol \+ term-image lib | Best-in-class terminal image fidelity; KGP unicode placeholder for Textual compat |
| Persistence | SQLite via aiosqlite | Zero-dependency, file-portable, async-friendly |
| Scheduler | APScheduler (async) | Per-profile interval scheduling with jitter |
| HTTP | httpx (async) \+ BeautifulSoup | Async HTTP with connection pooling; HTML parsing for scrape-based sources |
| Browser automation | Playwright (async) | Required for JS-heavy sources (FB Marketplace); optional dependency |
| IPC | Unix domain socket (asyncio) | Low-latency push from daemon to TUI; no external broker |
| Notifications | notify2 / plyer | Cross-platform desktop notification dispatch |
| Config | TOML (tomllib / tomli-w) | Human-readable, stdlib in Python 3.11+ |
| Package | uv / PyPI | pipx-installable; uv for dependency management |

## **7.2 Data Model (SQLite Schema Overview)**

* listings: id, profile\_id, source\_id, title, description, price, currency, condition, url, image\_urls (JSON), location, first\_seen, last\_seen, content\_hash, relevance\_score, status (new|seen|saved|dismissed|snoozed\_until)

* profiles: id, name, config\_json, enabled, created\_at, updated\_at

* sources: id, plugin\_id, last\_polled, consecutive\_errors, rate\_limit\_until

* price\_history: listing\_id, price, observed\_at

* image\_cache: url\_hash, local\_path, width, height, fetched\_at

## **7.3 Rate Limiting and Ethical Scraping**

* Per-source configurable minimum request intervals (default: 30s between requests to any single domain)

* Exponential backoff on HTTP errors with jitter

* Robots.txt compliance checked on first access per domain; result cached

* User-agent string identifies the application: Scavenger/1.0 (+https://github.com/user/scavenger)

* Optional Tor routing per source for sources with aggressive IP blocking

# **8\. Development Milestones**

| M\# | Name | Deliverables |
| :---- | :---- | :---- |
| M1 | Core Engine | Config parsing, SQLite schema, daemon skeleton, eBay \+ Craigslist plugins, deduplication, basic CLI output |
| M2 | TUI Shell | Textual layout with Profile Sidebar, Results Feed, Status Bar; navigation keybindings; no images yet |
| M3 | Kitty Graphics | Terminal detection, image download pipeline, KGP unicode placeholder rendering in result cards and detail panel |
| M4 | Alerting | Toast overlays, system notifications (Linux \+ macOS), quiet hours, escalation keywords |
| M5 | Source Expansion | Reddit RSS, HiFiShark, UsedPhotoPro, MPB, HiBid, generic RSS, Bring a Trailer plugins |
| M6 | Watchlist \+ Export | Watchlist CRUD, price history sparklines, CSV/JSON/Markdown export, scavenger-ctl install |
| M7 | Polish \+ Release | FB Marketplace plugin (Playwright), false-positive suppression learning, pypi packaging, README, demo GIF |

# **9\. Risks and Mitigations**

| Risk | Likelihood | Mitigation |
| :---- | :---- | :---- |
| Source anti-scraping measures break plugins | High | Plugin versioning; community plugin repo; graceful error handling with per-source backoff |
| Kitty KGP unicode placeholder breaks in future Textual versions | Medium | Abstract image rendering behind ImageRenderer interface; fallback paths already implemented |
| FB Marketplace Playwright flow breaks on UI changes | High | FB plugin is optional/contrib; mark as experimental; community-maintained |
| SQLite write contention between daemon and TUI | Low | WAL journal mode; TUI reads only; writes exclusively via daemon |
| Image cache unbounded growth | Medium | LRU eviction with configurable max size (default: 2GB); evict images for dismissed/expired listings first |

# **10\. Appendix — Sample Interest Profiles**

## **Vintage Sony A-mount Cameras and Lenses**

* Keywords: sony, \[a-mount, alpha mount\], \[a99, a77, a65, a900, a850\]

* Sources: ebay, usedphotopro, mpb, reddit\_rss (r/photomarket)

* Price: $50 – $1,200  |  Priority: high

## **IBM AS/400 and iSeries Hardware**

* Keywords: \[as/400, as400, iseries, i-series\], \[9406, 9401, 9402, 9404\], ibm

* Negative: "parts only", "no hdd", "password locked"

* Sources: ebay, hibid, craigslist, generic\_rss

* Price: $0 – $500  |  Priority: high  |  Escalation: AS/400, iSeries

## **Retro SGI Workstations**

* Keywords: sgi, \[indigo, indy, octane, fuel, tezro, onyx\], silicon graphics

* Sources: ebay, hibid, reddit\_rss (r/VintageComputers)

* Priority: normal

## **Specific Vehicle Search (Example)**

* Keywords: \["e30", "bmw 325"\], \["2002", "m3", "touring"\]

* Sources: craigslist, autotrader, lemon\_squeezy (Bring a Trailer)

* Price: $3,000 – $25,000  |  Location radius: 300 mi  |  Priority: normal

| TIP: Profiles ship as example templates in \~/.config/scavenger/examples/. Running scavenger init launches an interactive profile wizard that pre-populates common collecting categories. |
| :---- |

SCAVENGER PRD  •  v1.0 DRAFT  •  March 2026  •  Confidential