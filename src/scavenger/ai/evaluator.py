import asyncio
import json as _json
import logging
from dataclasses import dataclass
import httpx
from scavenger.ai.models import AIConfig, AIEvaluation
from scavenger.ai.prompts import build_prompt, build_batch_prompt
from scavenger.models import Profile, Listing

logger = logging.getLogger(__name__)

BATCH_SIZE = 10


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
        """Single-listing evaluation — bypasses queue, used for escalation calls."""
        evaluation = await self._call_model(
            profile, listing,
            model=self._config.filter_model,
            timeout=self._config.filter_timeout_sec,
        )
        if (
            evaluation.escalate
            and self._config.escalation_enabled
            and listing.relevance_score >= self._config.escalation_min_keyword_score
        ):
            escalation = await self._call_model(
                profile, listing,
                model=self._config.escalation_model,
                timeout=self._config.escalation_timeout_sec,
            )
            evaluation = AIEvaluation(
                relevant=evaluation.relevant,
                reason=evaluation.reason,
                notable=evaluation.notable or escalation.notable,
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
        return all_results

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
