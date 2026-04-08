from datetime import datetime
from typing import Literal
from pydantic import BaseModel, field_validator


class Listing(BaseModel):
    id: str
    profile_id: str
    source_id: str
    title: str
    description: str = ""
    price: float | None = None
    currency: str = "USD"
    condition: str | None = None
    url: str
    image_urls: list[str] = []
    location: str | None = None
    first_seen: datetime
    last_seen: datetime
    relevance_score: float = 0.0
    status: Literal["new", "seen", "saved", "dismissed", "snoozed"] = "new"
    ai_evaluation: str | None = None  # JSON-serialized AIEvaluation, set by daemon


class Profile(BaseModel):
    id: str
    name: str
    keywords: list[str | list[str]]
    negative_keywords: list[str] = []
    sources: list[str]
    price_min: float | None = None
    price_max: float | None = None
    poll_interval_sec: int = 3600
    alert_priority: Literal["high", "normal", "low"] = "normal"
    enabled: bool = True
    tags: list[str] = []
    escalation_keywords: list[str] = []
    location_radius_mi: int | None = None

    @field_validator("poll_interval_sec")
    @classmethod
    def poll_interval_must_be_positive(cls, v: int) -> int:
        if v < 30:
            raise ValueError("poll_interval_sec must be >= 30")
        return v
