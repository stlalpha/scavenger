import logging
import httpx
from scavenger.ai.models import AIConfig, AIEvaluation
from scavenger.ai.prompts import build_prompt
from scavenger.models import Profile, Listing

logger = logging.getLogger(__name__)


class NoopEvaluator:
    """Passthrough evaluator used when AI is disabled."""

    async def evaluate(self, profile: Profile, listing: Listing) -> AIEvaluation:
        return AIEvaluation.passthrough()


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
            evaluation = await self._call_model(
                profile, listing,
                model=self._config.escalation_model,
                timeout=self._config.escalation_timeout_sec,
            )
        return evaluation

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
