import json as _json
import logging
import httpx
from scavenger.ai.models import AIConfig, AIEvaluation
from scavenger.ai.prompts import build_prompt, build_batch_prompt
from scavenger.models import Profile, Listing

logger = logging.getLogger(__name__)

BATCH_SIZE = 10


class NoopEvaluator:
    """Passthrough evaluator used when AI is disabled."""

    async def evaluate(self, profile: Profile, listing: Listing) -> AIEvaluation:
        return AIEvaluation.passthrough()

    async def evaluate_batch(
        self, profile: Profile, listings: list[Listing]
    ) -> dict[str, AIEvaluation]:
        return {l.id: AIEvaluation.passthrough() for l in listings}


class AIEvaluator:
    def __init__(self, config: AIConfig):
        self._config = config
        self._url = f"{config.litellm_base_url}/chat/completions"

    async def evaluate(self, profile: Profile, listing: Listing) -> AIEvaluation:
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
        """Evaluate multiple listings in a single LLM call. Returns {listing_id: AIEvaluation}."""
        if not listings:
            return {}
        results: dict[str, AIEvaluation] = {}
        for i in range(0, len(listings), BATCH_SIZE):
            chunk = listings[i : i + BATCH_SIZE]
            chunk_results = await self._call_batch(profile, chunk)
            results.update(chunk_results)
        return results

    async def _call_batch(
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
                        "temperature": 0.1,
                        "response_format": {"type": "json_object"},
                    },
                    headers={"Authorization": f"Bearer {self._config.api_key}"},
                )
                response.raise_for_status()
                content = response.json()["choices"][0]["message"]["content"]
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
                        "temperature": 0.1,
                        "response_format": {"type": "json_object"},
                    },
                    headers={"Authorization": f"Bearer {self._config.api_key}"},
                )
                response.raise_for_status()
                content = response.json()["choices"][0]["message"]["content"]
                return AIEvaluation.model_validate_json(content)
        except (httpx.HTTPError, httpx.TimeoutException) as e:
            logger.warning("AI evaluation HTTP error: %s", e)
            return AIEvaluation.passthrough()
        except Exception as e:
            logger.warning("AI evaluation failed: %s", e)
            return AIEvaluation.passthrough()
