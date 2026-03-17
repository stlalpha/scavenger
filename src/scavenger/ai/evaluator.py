import asyncio
import json as _json
import logging
import os
import re
from dataclasses import dataclass

from litellm import acompletion

from scavenger.ai.models import AIConfig, AIEvaluation
from scavenger.ai.prompts import build_prompt, build_batch_prompt, build_escalation_prompt
from scavenger.models import Profile, Listing

logger = logging.getLogger(__name__)

# Suppress litellm's noisy default logging
logging.getLogger("LiteLLM").setLevel(logging.WARNING)
logging.getLogger("httpx").setLevel(logging.WARNING)

BATCH_SIZE = 5  # smaller batches = fewer Ollama timeouts
ESCALATION_DELAY = 1.0  # seconds between Anthropic calls to avoid rate limits
MAX_ESCALATIONS_PER_BATCH = 5  # cap frontier calls per poll cycle


def _extract_json(text: str) -> str:
    """Extract the first JSON object from model output."""
    text = text.strip()
    if text.startswith("```"):
        nl = text.find("\n")
        if nl != -1:
            text = text[nl + 1:]
    if text.rstrip().endswith("```"):
        text = text[: text.rfind("```")]
    text = text.strip()
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


def _match_escalation_keywords(keywords: list[str], title: str, description: str) -> list[str]:
    """Return which escalation keywords appear in the listing text."""
    text = f"{title} {description}".lower()
    return [kw for kw in keywords if re.search(re.escape(kw.lower()), text)]


@dataclass
class _EvalJob:
    profile: Profile
    listings: list[Listing]
    future: asyncio.Future


class NoopEvaluator:
    """Passthrough evaluator used when AI is disabled."""

    async def evaluate(self, profile: Profile, listing: Listing) -> AIEvaluation:
        return AIEvaluation.passthrough()

    async def evaluate_batch(
        self, profile: Profile, listings: list[Listing]
    ) -> dict[str, AIEvaluation]:
        return {l.id: AIEvaluation.passthrough() for l in listings}

    async def start(self) -> None:
        pass

    async def stop(self) -> None:
        pass


class AIEvaluator:
    def __init__(self, config: AIConfig):
        self._config = config
        # Set API key for litellm's Anthropic calls
        if config.anthropic_api_key:
            os.environ.setdefault("ANTHROPIC_API_KEY", config.anthropic_api_key)
        # Ollama model prefix for litellm
        self._filter_model = f"ollama/{config.filter_model}"
        self._filter_base = config.litellm_base_url.removesuffix("/v1").removesuffix("/")
        # Anthropic model for escalation
        self._escalation_model = f"anthropic/{config.escalation_model}"
        self._queue: asyncio.Queue[_EvalJob | None] = asyncio.Queue()
        self._worker_task: asyncio.Task | None = None

    async def start(self) -> None:
        self._worker_task = asyncio.create_task(self._worker())

    async def stop(self) -> None:
        await self._queue.put(None)
        if self._worker_task:
            await self._worker_task

    async def _worker(self) -> None:
        while True:
            job = await self._queue.get()
            if job is None:
                break
            try:
                results = await self._process_batch(job.profile, job.listings)
                job.future.set_result(results)
            except Exception as e:
                if not job.future.done():
                    job.future.set_exception(e)
            finally:
                self._queue.task_done()

    async def _call_filter(self, system: str, user: str) -> str:
        """Call the local Ollama model via litellm."""
        response = await acompletion(
            model=self._filter_model,
            messages=[
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            temperature=0.1,
            timeout=self._config.filter_timeout_sec,
            api_base=self._filter_base,
            response_format={"type": "json_object"},
        )
        return response.choices[0].message.content

    async def _call_frontier(self, system: str, user: str) -> str:
        """Call the Anthropic frontier model via litellm."""
        response = await acompletion(
            model=self._escalation_model,
            messages=[
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            temperature=0.1,
            max_tokens=1024,
            timeout=self._config.escalation_timeout_sec,
        )
        return _extract_json(response.choices[0].message.content)

    async def evaluate(self, profile: Profile, listing: Listing) -> AIEvaluation:
        """Single-listing evaluation with optional keyword-triggered escalation."""
        system, user = build_prompt(profile, listing)
        try:
            content = await self._call_filter(system, user)
            evaluation = AIEvaluation.model_validate_json(content)
        except Exception as e:
            logger.warning("Filter eval failed: %s", e)
            return AIEvaluation.passthrough()

        if not evaluation.relevant:
            return evaluation

        should_escalate = evaluation.escalate
        matched: list[str] = []
        if self._config.escalation_enabled and profile.escalation_keywords:
            matched = _match_escalation_keywords(
                profile.escalation_keywords, listing.title, listing.description
            )
            if matched:
                should_escalate = True
        if should_escalate and self._config.escalation_enabled:
            escalation = await self._escalate(profile, listing, matched or ["(model-triggered)"])
            evaluation = AIEvaluation(
                relevant=evaluation.relevant,
                reason=escalation.reason or evaluation.reason,
                notable=escalation.notable or evaluation.notable,
                escalate=escalation.escalate,
            )
        return evaluation

    async def evaluate_batch(
        self, profile: Profile, listings: list[Listing]
    ) -> dict[str, AIEvaluation]:
        if not listings:
            return {}
        loop = asyncio.get_running_loop()
        all_results: dict[str, AIEvaluation] = {}
        futures: list[asyncio.Future] = []
        for i in range(0, len(listings), BATCH_SIZE):
            chunk = listings[i : i + BATCH_SIZE]
            future = loop.create_future()
            futures.append(future)
            await self._queue.put(_EvalJob(profile=profile, listings=chunk, future=future))
        for future in futures:
            chunk_results = await future
            all_results.update(chunk_results)

        # Escalate listings that contain escalation keywords (rate-limited)
        if self._config.escalation_enabled and profile.escalation_keywords:
            escalation_count = 0
            for listing in listings:
                if escalation_count >= MAX_ESCALATIONS_PER_BATCH:
                    logger.info("Escalation cap reached (%d), deferring rest", MAX_ESCALATIONS_PER_BATCH)
                    break
                ev = all_results.get(listing.id)
                if not ev or not ev.relevant:
                    continue
                matched = _match_escalation_keywords(
                    profile.escalation_keywords, listing.title, listing.description
                )
                if matched:
                    logger.info(
                        "Escalating %s — matched: %s",
                        listing.id[:12], ", ".join(matched),
                    )
                    if escalation_count > 0:
                        await asyncio.sleep(ESCALATION_DELAY)
                    escalation = await self._escalate(profile, listing, matched)
                    all_results[listing.id] = AIEvaluation(
                        relevant=ev.relevant,
                        reason=escalation.reason or ev.reason,
                        notable=escalation.notable or ev.notable,
                        escalate=escalation.escalate,
                    )
                    escalation_count += 1

        return all_results

    async def _escalate(
        self, profile: Profile, listing: Listing, triggered_keywords: list[str]
    ) -> AIEvaluation:
        system, user = build_escalation_prompt(profile, listing, triggered_keywords)
        try:
            content = await self._call_frontier(system, user)
            return AIEvaluation.model_validate_json(content)
        except Exception as e:
            logger.warning("Escalation failed for %s: %s", listing.id[:12], e)
            return AIEvaluation.passthrough()

    async def _process_batch(
        self, profile: Profile, listings: list[Listing]
    ) -> dict[str, AIEvaluation]:
        system, user = build_batch_prompt(profile, listings)
        try:
            content = await self._call_filter(system, user)
            parsed = _json.loads(content)
            if isinstance(parsed, dict):
                parsed = parsed.get("results", parsed.get("evaluations", []))
            if not isinstance(parsed, list):
                logger.warning("AI batch: expected list, got %s", type(parsed).__name__)
                return {l.id: AIEvaluation.passthrough() for l in listings}
            results: dict[str, AIEvaluation] = {}
            for item in parsed:
                try:
                    listing_id = item.pop("id", None)
                    if listing_id:
                        results[listing_id] = AIEvaluation(**item)
                except Exception as e:
                    logger.debug("AI batch: skipping malformed item: %s", e)
            for listing in listings:
                if listing.id not in results:
                    results[listing.id] = AIEvaluation.passthrough()
            return results
        except Exception as e:
            logger.warning("AI batch eval failed (%s): %s", type(e).__name__, e)
            return {l.id: AIEvaluation.passthrough() for l in listings}
