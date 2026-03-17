import json as _json
import os
import re

from scavenger.models import Profile
from textual.app import ComposeResult
from textual.screen import ModalScreen
from textual.containers import Vertical, Horizontal, VerticalScroll
from textual.widgets import Static, Input, Checkbox, Button, Select
from textual.binding import Binding

def _extract_json(text: str) -> str:
    """Extract the first JSON object from model output, ignoring fences and trailing text."""
    text = text.strip()
    # Strip opening fence
    if text.startswith("```"):
        nl = text.find("\n")
        if nl != -1:
            text = text[nl + 1:]
    # Strip closing fence
    if text.rstrip().endswith("```"):
        text = text[: text.rfind("```")]
    text = text.strip()
    # Find the first { and its matching }
    start = text.find("{")
    if start == -1:
        return text
    depth = 0
    in_str = False
    escape = False
    for i, ch in enumerate(text[start:], start):
        if escape:
            escape = False
            continue
        if ch == "\\":
            escape = True
            continue
        if ch == '"':
            in_str = not in_str
            continue
        if in_str:
            continue
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return text[start : i + 1]
    return text[start:]


PRIORITY_OPTIONS = [("High", "high"), ("Normal", "normal"), ("Low", "low")]
POLL_OPTIONS = [
    ("5 min", "300"),
    ("15 min", "900"),
    ("30 min", "1800"),
    ("1 hour", "3600"),
    ("2 hours", "7200"),
]


def _kw_to_str(keywords: list[str | list[str]]) -> str:
    parts = []
    for k in keywords:
        if isinstance(k, list):
            parts.append("|".join(k))
        else:
            parts.append(k)
    return ", ".join(parts)


class ProfileFormScreen(ModalScreen[dict | None]):
    """Modal form to create or edit a search profile.

    Pass `profile` to edit an existing one. Omit for new profile.
    Result dict includes `_action`: "create", "update", or "delete".
    """

    BINDINGS = [
        Binding("escape", "cancel", "Cancel"),
    ]

    DEFAULT_CSS = """
    ProfileFormScreen {
        align: center middle;
    }

    #ap-dialog {
        width: 64;
        height: auto;
        max-height: 90%;
        background: $surface;
        border: double $accent;
        padding: 1 2;
    }

    #ap-title {
        text-style: bold;
        width: 100%;
        text-align: center;
        color: $accent;
        margin-bottom: 1;
    }

    #ap-scroll {
        height: 1fr;
    }

    .fg {
        height: auto;
        margin-bottom: 1;
    }
    .fg Static.fl {
        color: $text;
        text-style: bold;
        margin: 0;
        padding: 0;
    }
    .fg Static.fh {
        color: $text-disabled;
        margin: 0;
        padding: 0;
    }
    .fg Input { margin: 0; }
    .fg Select { margin: 0; }

    .fg .src-checks {
        height: auto;
        margin: 0;
        padding: 0;
    }
    .fg .src-checks Checkbox {
        width: auto;
        margin: 0 2 0 0;
        padding: 0;
    }

    .fg .pair { height: auto; }
    .fg .pair .half {
        width: 1fr;
        height: auto;
        margin: 0;
        padding: 0 1 0 0;
    }
    .fg .pair .half Select { width: 100%; }
    .fg .pair .half Input { width: 100%; }

    .fg .price-pair { height: auto; }
    .fg .price-pair Input { width: 1fr; }
    .fg .price-pair .price-sep {
        width: 5;
        content-align: center middle;
        padding: 1 0 0 0;
    }

    .fg .name-row { height: auto; }
    .fg .name-row Input { width: 1fr; }
    .fg .name-row Button {
        width: auto;
        min-width: 14;
        margin: 0 0 0 1;
    }

    #ap-buttons {
        margin-top: 1;
        height: auto;
        align: right middle;
    }
    #ap-buttons Button {
        margin: 0 0 0 1;
    }
    #ap-spacer {
        width: 1fr;
    }
    #btn-delete {
        margin: 0;
    }
    """

    def __init__(self, profile: Profile | None = None) -> None:
        super().__init__()
        self._editing = profile

    def compose(self) -> ComposeResult:
        p = self._editing
        editing = p is not None
        title = f"EDIT — {p.name}" if editing else "NEW PROFILE"

        with Vertical(id="ap-dialog"):
            yield Static(title, id="ap-title")
            with VerticalScroll(id="ap-scroll"):

                with Vertical(classes="fg"):
                    yield Static("Name", classes="fl")
                    with Horizontal(classes="name-row"):
                        yield Input(
                            value=p.name if editing else "",
                            placeholder="e.g. Vintage 35mm Film Cameras",
                            id="input-name",
                        )
                        yield Button("AI Suggest", variant="warning", id="btn-suggest")

                with Vertical(classes="fg"):
                    yield Static("Keywords", classes="fl")
                    yield Static("comma-separated — use | for OR: ae-1|at-1 matches either", classes="fh")
                    yield Input(
                        value=_kw_to_str(p.keywords) if editing else "",
                        placeholder="e.g. canon, ae-1|at-1, 35mm",
                        id="input-keywords",
                    )

                with Vertical(classes="fg"):
                    yield Static("Negative keywords", classes="fl")
                    yield Static("listings matching any of these are dropped", classes="fh")
                    yield Input(
                        value=", ".join(p.negative_keywords) if editing else "",
                        placeholder="e.g. broken, parts only",
                        id="input-negatives",
                    )

                with Vertical(classes="fg"):
                    yield Static("Sources", classes="fl")
                    with Horizontal(classes="src-checks"):
                        yield Checkbox("eBay", "ebay" in p.sources if editing else True, id="src-ebay")
                        yield Checkbox("Facebook", "facebook" in p.sources if editing else True, id="src-facebook")
                        yield Checkbox("Craigslist", "craigslist" in p.sources if editing else True, id="src-craigslist")

                with Vertical(classes="fg"):
                    yield Static("Price range", classes="fl")
                    with Horizontal(classes="price-pair"):
                        yield Input(
                            value=str(int(p.price_min)) if editing and p.price_min is not None else "",
                            placeholder="min $",
                            id="input-price-min",
                            type="number",
                        )
                        yield Static("to", classes="price-sep")
                        yield Input(
                            value=str(int(p.price_max)) if editing and p.price_max is not None else "",
                            placeholder="max $",
                            id="input-price-max",
                            type="number",
                        )

                with Vertical(classes="fg"):
                    with Horizontal(classes="pair"):
                        with Vertical(classes="half"):
                            yield Static("Poll every", classes="fl")
                            yield Select(
                                POLL_OPTIONS,
                                value=str(p.poll_interval_sec) if editing else "900",
                                id="select-poll-interval",
                            )
                        with Vertical(classes="half"):
                            yield Static("Alert priority", classes="fl")
                            yield Select(
                                PRIORITY_OPTIONS,
                                value=p.alert_priority if editing else "normal",
                                id="select-priority",
                            )

                with Vertical(classes="fg"):
                    yield Static("Tags", classes="fl")
                    yield Static("for organizing profiles", classes="fh")
                    yield Input(
                        value=", ".join(p.tags) if editing else "",
                        placeholder="e.g. vintage, lenses",
                        id="input-tags",
                    )

                with Vertical(classes="fg"):
                    yield Static("Escalation keywords", classes="fl")
                    yield Static("trigger deeper AI eval when these appear in a listing", classes="fh")
                    yield Input(
                        value=", ".join(p.escalation_keywords) if editing else "",
                        placeholder="e.g. rare, mint, NOS",
                        id="input-escalation",
                    )

                with Vertical(classes="fg"):
                    yield Static("Location radius (miles)", classes="fl")
                    yield Static("distance from home_zip for geo sources", classes="fh")
                    yield Input(
                        value=str(p.location_radius_mi) if editing and p.location_radius_mi else "",
                        placeholder="e.g. 50",
                        id="input-radius",
                        type="integer",
                    )

            with Horizontal(id="ap-buttons"):
                yield Button("Cancel", variant="default", id="btn-cancel")
                yield Button("Save" if editing else "Create", variant="primary", id="btn-save")
                if editing:
                    yield Static("", id="ap-spacer")
                    yield Button("Delete", variant="error", id="btn-delete")

    def action_cancel(self) -> None:
        self.dismiss(None)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-cancel":
            self.dismiss(None)
        elif event.button.id == "btn-save":
            self._submit()
        elif event.button.id == "btn-suggest":
            self._do_suggest()
        elif event.button.id == "btn-delete":
            self._confirm_delete()

    def _confirm_delete(self) -> None:
        if not self._editing:
            return
        self.dismiss({"_action": "delete", "id": self._editing.id, "name": self._editing.name})

    def _do_suggest(self) -> None:
        name = self.query_one("#input-name", Input).value.strip()
        if not name:
            self.notify("Enter a name first — describe what you're looking for", severity="warning")
            return
        context = {"name": name}
        kw = self.query_one("#input-keywords", Input).value.strip()
        if kw:
            context["keywords_so_far"] = kw
        neg = self.query_one("#input-negatives", Input).value.strip()
        if neg:
            context["negative_keywords_so_far"] = neg
        pmin = self.query_one("#input-price-min", Input).value.strip()
        pmax = self.query_one("#input-price-max", Input).value.strip()
        if pmin:
            context["price_min"] = pmin
        if pmax:
            context["price_max"] = pmax
        esc = self.query_one("#input-escalation", Input).value.strip()
        if esc:
            context["escalation_keywords_so_far"] = esc
        sources = []
        if self.query_one("#src-ebay", Checkbox).value:
            sources.append("ebay")
        if self.query_one("#src-facebook", Checkbox).value:
            sources.append("facebook")
        if self.query_one("#src-craigslist", Checkbox).value:
            sources.append("craigslist")
        if sources:
            context["sources"] = sources
        self.notify("Asking AI for suggestions...")
        self.run_worker(self._suggest(context), exclusive=True)

    async def _suggest(self, context: dict) -> None:
        from scavenger.config import load_ai_config
        from pathlib import Path

        config_path = Path("~/.config/scavenger/config.toml").expanduser()
        ai_config = load_ai_config(config_path)
        if not ai_config.enabled:
            self.notify("AI not configured — add [ai] section to config.toml", severity="error")
            return
        if not ai_config.anthropic_api_key:
            self.notify("anthropic_api_key not set in [ai] config", severity="error")
            return

        system = """You are an expert in online marketplace searching (eBay, Facebook Marketplace, Craigslist). Generate search parameters that will actually find relevant listings.

CRITICAL — how the scoring engine works:
- The keywords array is a list of REQUIRED match groups. EVERY top-level entry must match somewhere in the listing title or description, or the listing scores 0 and is dropped.
- A plain string like "sony" matches as a case-insensitive substring. "vintage computer" requires those EXACT words adjacent — "Vintage Apple Computer" would NOT match because "Apple" is between them.
- An OR group like ["ae-1", "ae1", "AE1"] matches if ANY variant appears. Use these for alternate spellings, abbreviations, and synonyms.
- Because all groups are AND'd, fewer top-level entries = more results. Be conservative. 2-3 keyword groups is ideal. More than 4 will likely match nothing.

Respond ONLY with valid JSON matching this schema:
{
  "keywords": ["string or [\"variant1\", \"variant2\"] for OR groups"],
  "negative_keywords": ["words that indicate irrelevant listings"],
  "price_min": number or null,
  "price_max": number or null,
  "escalation_keywords": ["words that suggest a listing deserves closer expert analysis"]
}

Keyword strategy:
- Use SHORT, SINGLE-WORD keywords as top-level entries whenever possible. "sony" not "sony camera". "vintage" not "vintage item".
- Use OR groups liberally for synonyms: ["vintage", "retro", "classic", "antique"] as ONE group, not four separate required keywords.
- Think about how real sellers actually write listing titles on eBay/Craigslist/Facebook. They don't use formal language.
- If the item has a specific model number, include common misspellings and abbreviations as OR variants.
- Do NOT require words like "rare" or "mint" as keywords — those belong in escalation_keywords.

Negative keywords:
- Terms that indicate junk, accessories, or unrelated items that happen to match the keywords.
- Be specific to the category. For cameras: "strap", "case only", "manual only". For computers: "keyboard only", "mouse", "cable".

Price range:
- Based on real market values for this category. null if too variable to estimate.

Escalation keywords:
- Terms that suggest the listing deserves a deeper look from a smarter AI model.
- Include: condition claims ("mint", "NOS", "sealed"), rarity claims ("rare", "prototype", "one of a kind"), and specific high-value model names/variants for this category.
- The escalation model will evaluate whether these claims are actually credible.

If the user has already entered values for some fields, improve and expand on them rather than starting over."""

        parts = [f"I'm looking for: {context['name']}"]
        if "keywords_so_far" in context:
            parts.append(f"I've started with these keywords: {context['keywords_so_far']}")
        if "negative_keywords_so_far" in context:
            parts.append(f"Negative keywords so far: {context['negative_keywords_so_far']}")
        if "price_min" in context or "price_max" in context:
            price = f"${context.get('price_min', '?')} – ${context.get('price_max', '?')}"
            parts.append(f"Price range I had in mind: {price}")
        if "escalation_keywords_so_far" in context:
            parts.append(f"Escalation keywords so far: {context['escalation_keywords_so_far']}")
        if "sources" in context:
            parts.append(f"Searching on: {', '.join(context['sources'])}")
        user = "\n".join(parts)

        try:
            from litellm import acompletion
            os.environ.setdefault("ANTHROPIC_API_KEY", ai_config.anthropic_api_key)
            model = f"anthropic/{ai_config.escalation_model}"
            response = await acompletion(
                model=model,
                messages=[
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
                temperature=0.3,
                max_tokens=1024,
                timeout=30.0,
            )
            content = response.choices[0].message.content
            suggestions = _json.loads(_extract_json(content))
        except Exception as e:
            self.notify(f"AI suggest failed: {type(e).__name__}: {e}", severity="error")
            return

        kw = suggestions.get("keywords", [])
        if kw:
            self.query_one("#input-keywords", Input).value = _kw_to_str(kw)
        neg = suggestions.get("negative_keywords", [])
        if neg:
            self.query_one("#input-negatives", Input).value = ", ".join(neg)
        if suggestions.get("price_min") is not None:
            self.query_one("#input-price-min", Input).value = str(int(suggestions["price_min"]))
        if suggestions.get("price_max") is not None:
            self.query_one("#input-price-max", Input).value = str(int(suggestions["price_max"]))
        esc = suggestions.get("escalation_keywords", [])
        if esc:
            self.query_one("#input-escalation", Input).value = ", ".join(esc)
        self.notify("Fields populated from AI suggestions — review and adjust")

    def _submit(self) -> None:
        name = self.query_one("#input-name", Input).value.strip()
        if not name:
            self.notify("Name is required", severity="error")
            return

        raw_kw = self.query_one("#input-keywords", Input).value.strip()
        if not raw_kw:
            self.notify("At least one keyword is required", severity="error")
            return

        keywords: list[str | list[str]] = []
        for part in raw_kw.split(","):
            part = part.strip()
            if not part:
                continue
            if "|" in part:
                keywords.append([v.strip() for v in part.split("|") if v.strip()])
            else:
                keywords.append(part)

        negatives = [
            n.strip() for n in self.query_one("#input-negatives", Input).value.split(",")
            if n.strip()
        ]
        sources = []
        if self.query_one("#src-ebay", Checkbox).value:
            sources.append("ebay")
        if self.query_one("#src-facebook", Checkbox).value:
            sources.append("facebook")
        if self.query_one("#src-craigslist", Checkbox).value:
            sources.append("craigslist")
        if not sources:
            self.notify("Select at least one source", severity="error")
            return

        price_min = None
        price_max = None
        raw_min = self.query_one("#input-price-min", Input).value.strip()
        raw_max = self.query_one("#input-price-max", Input).value.strip()
        if raw_min:
            try:
                price_min = float(raw_min)
            except ValueError:
                self.notify("Invalid minimum price", severity="error")
                return
        if raw_max:
            try:
                price_max = float(raw_max)
            except ValueError:
                self.notify("Invalid maximum price", severity="error")
                return

        poll_interval = int(self.query_one("#select-poll-interval", Select).value)
        priority = str(self.query_one("#select-priority", Select).value)
        tags = [t.strip() for t in self.query_one("#input-tags", Input).value.split(",") if t.strip()]
        escalation = [e.strip() for e in self.query_one("#input-escalation", Input).value.split(",") if e.strip()]
        radius = None
        raw_radius = self.query_one("#input-radius", Input).value.strip()
        if raw_radius:
            try:
                radius = int(raw_radius)
            except ValueError:
                self.notify("Invalid radius", severity="error")
                return

        # Keep original ID when editing, generate from name when creating
        if self._editing:
            profile_id = self._editing.id
            action = "update"
        else:
            profile_id = re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")
            action = "create"

        result: dict = {
            "_action": action,
            "id": profile_id,
            "name": name,
            "keywords": keywords,
            "sources": sources,
            "negative_keywords": negatives,
            "price_min": price_min,
            "price_max": price_max,
            "poll_interval_sec": poll_interval,
            "alert_priority": priority,
            "tags": tags,
            "escalation_keywords": escalation,
            "location_radius_mi": radius,
        }
        self.dismiss(result)
