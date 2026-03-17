import asyncio
import json as _json
import logging
import re
from dataclasses import dataclass
import httpx
from scavenger.ai.models import AIConfig, AIEvaluation
from scavenger.ai.prompts import build_prompt, build_batch_prompt, build_escalation_prompt
from scavenger.models import Profile, Listing

logger = logging.getLogger(__name__)

BATCH_SIZE = 10
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


def _ollama_base(config_url: str) -> str:
    """Derive Ollama native API base from the configured URL.

    Strips /v1 suffix if present (OpenAI compat path) to get the root.
    """
    return config_url.removesuffix("/v1").removesuffix("/")


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
        self._url = f"{_ollama_base(config.litellm_base_url)}/api/chat"
        self._queue: asyncio.Queue[_EvalJob | None] = asyncio.Queue()
        self._worker_task: asyncio.Task | None = None

    async def start(self) -> None:
        """Start the background evaluation worker."""
        self._worker_task = asyncio.create_task(self._worker())

    async def stop(self) -> None:
        """Signal the worker to drain and stop."""
        await self._queue.put(None)
        if self._worker_task:
            await self._worker_task

    async def _worker(self) -> None:
        """Single worker that processes evaluation jobs sequentially."""
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

    async def evaluate(self, profile: Profile, listing: Listing) -> AIEvaluation:
        """Single-listing evaluation with optional keyword-triggered escalation."""
        evaluation = await self._call_model(
            profile, listing,
            model=self._config.filter_model,
            timeout=self._config.filter_timeout_sec,
        )
        if not evaluation.relevant:
            return evaluation
        # Escalate if cheap model says so OR if escalation keywords match
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
        """Queue listings for evaluation and await the result."""
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

        # Escalate listings that contain escalation keywords
        if self._config.escalation_enabled and profile.escalation_keywords:
            for listing in listings:
                ev = all_results.get(listing.id)
                if not ev or not ev.relevant:
                    continue
                matched = _match_escalation_keywords(
                    profile.escalation_keywords, listing.title, listing.description
                )
                if matched:
                    logger.info(
                        "Escalating %s — matched keywords: %s",
                        listing.id[:12], ", ".join(matched),
                    )
                    escalation = await self._escalate(profile, listing, matched)
                    all_results[listing.id] = AIEvaluation(
                        relevant=ev.relevant,
                        reason=escalation.reason or ev.reason,
                        notable=escalation.notable or ev.notable,
                        escalate=escalation.escalate,
                    )

        return all_results

    async def _escalate(
        self, profile: Profile, listing: Listing, triggered_keywords: list[str]
    ) -> AIEvaluation:
        """Send a listing to the frontier model for deeper evaluation."""
        system_prompt, user_prompt = build_escalation_prompt(
            profile, listing, triggered_keywords
        )
        try:
            content = await self._call_anthropic(system_prompt, user_prompt)
            return AIEvaluation.model_validate_json(content)
        except Exception as e:
            logger.warning("Escalation failed for %s: %s", listing.id[:12], e)
            return AIEvaluation.passthrough()

    async def _call_anthropic(
        self, system_prompt: str, user_prompt: str, temperature: float = 0.1
    ) -> str:
        """Call the Anthropic Messages API. Returns the text content."""
        api_key = self._config.anthropic_api_key
        if not api_key:
            raise ValueError("anthropic_api_key not set in [ai] config")
        async with httpx.AsyncClient(timeout=self._config.escalation_timeout_sec) as client:
            response = await client.post(
                "https://api.anthropic.com/v1/messages",
                headers={
                    "x-api-key": api_key,
                    "anthropic-version": "2023-06-01",
                    "content-type": "application/json",
                },
                json={
                    "model": self._config.escalation_model,
                    "max_tokens": 1024,
                    "system": system_prompt,
                    "messages": [{"role": "user", "content": user_prompt}],
                    "temperature": temperature,
                },
            )
            response.raise_for_status()
            data = response.json()
            return _extract_json(data["content"][0]["text"])

    async def _process_batch(
        self, profile: Profile, listings: list[Listing]
    ) -> dict[str, AIEvaluation]:
        system_prompt, user_prompt = build_batch_prompt(profile, listings)
        try:
            async with httpx.AsyncClient(timeout=self._config.filter_timeout_sec) as client:
                response = await client.post(
                    self._url,
                    json={
                        "model": self._config.filter_model,
                        "messages": [
                            {"role": "system", "content": system_prompt},
                            {"role": "user", "content": user_prompt},
                        ],
                        "stream": False,
                        "think": False,
                        "format": "json",
                        "options": {"temperature": 0.1},
                    },
                )
                response.raise_for_status()
                content = response.json()["message"]["content"]
                parsed = _json.loads(content)
                # Handle both {"results": [...]} and bare [...]
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
                # Fill in passthrough for any listings the model missed
                for listing in listings:
                    if listing.id not in results:
                        results[listing.id] = AIEvaluation.passthrough()
                return results
        except (httpx.HTTPError, httpx.TimeoutException) as e:
            logger.warning("AI batch evaluation HTTP error: %s", e)
            return {l.id: AIEvaluation.passthrough() for l in listings}
        except Exception as e:
            logger.warning("AI batch evaluation failed: %s", e)
            return {l.id: AIEvaluation.passthrough() for l in listings}

    async def _call_model(
        self, profile: Profile, listing: Listing, model: str, timeout: float
    ) -> AIEvaluation:
        system_prompt, user_prompt = build_prompt(profile, listing)
        try:
            async with httpx.AsyncClient(timeout=timeout) as client:
                response = await client.post(
                    self._url,
                    json={
                        "model": model,
                        "messages": [
                            {"role": "system", "content": system_prompt},
                            {"role": "user", "content": user_prompt},
                        ],
                        "stream": False,
                        "think": False,
                        "format": "json",
                        "options": {"temperature": 0.1},
                    },
                )
                response.raise_for_status()
                content = response.json()["message"]["content"]
                return AIEvaluation.model_validate_json(content)
        except (httpx.HTTPError, httpx.TimeoutException) as e:
            logger.warning("AI evaluation HTTP error: %s", e)
            return AIEvaluation.passthrough()
        except Exception as e:
            logger.warning("AI evaluation failed: %s", e)
            return AIEvaluation.passthrough()
