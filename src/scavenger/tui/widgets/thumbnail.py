import logging
import time
from pathlib import Path, PurePosixPath
import httpx
from scavenger.dedup import content_hash

logger = logging.getLogger(__name__)

PLACEHOLDER = "□"
DEFAULT_CACHE_DIR = Path("~/.cache/scavenger/images").expanduser()
DEFAULT_MAX_AGE_DAYS = 30
_MEM_CACHE_SIZE = 128


def _ext_from_url(url: str) -> str:
    suffix = PurePosixPath(url.split("?")[0]).suffix
    return suffix if suffix else ".jpg"


class ThumbnailCache:
    """Downloads and caches listing images. Returns Path or PLACEHOLDER string."""

    def __init__(
        self,
        cache_dir: Path = DEFAULT_CACHE_DIR,
        max_age_days: int = DEFAULT_MAX_AGE_DAYS,
    ) -> None:
        self._cache_dir = cache_dir
        self._cache_dir.mkdir(parents=True, exist_ok=True)
        self._max_age_days = max_age_days
        self._client: httpx.AsyncClient | None = None
        self._resolved: dict[str, Path | str] = {}

    def _get_client(self) -> httpx.AsyncClient:
        if self._client is None or self._client.is_closed:
            self._client = httpx.AsyncClient(timeout=10.0)
        return self._client

    async def close(self) -> None:
        if self._client and not self._client.is_closed:
            await self._client.aclose()
            self._client = None

    def evict(self) -> int:
        """Remove cached images older than max_age_days. Returns count removed."""
        cutoff = time.time() - (self._max_age_days * 86400)
        removed = 0
        for f in self._cache_dir.iterdir():
            if f.is_file() and f.stat().st_mtime < cutoff:
                f.unlink()
                removed += 1
        if removed:
            logger.info("Evicted %d stale images from cache", removed)
        self._resolved.clear()
        return removed

    def _cache_path(self, url: str) -> Path:
        return self._cache_dir / f"{content_hash(url)}{_ext_from_url(url)}"

    async def get(self, url: str | None) -> Path | str:
        if not url:
            return PLACEHOLDER
        hit = self._resolved.get(url)
        if hit is not None:
            return hit
        cached = self._cache_path(url)
        if cached.exists():
            self._resolved[url] = cached
            return cached
        result = await self._download(url, cached)
        self._resolved[url] = result
        if len(self._resolved) > _MEM_CACHE_SIZE:
            # drop oldest quarter
            keys = list(self._resolved)[:_MEM_CACHE_SIZE // 4]
            for k in keys:
                del self._resolved[k]
        return result

    async def _download(self, url: str, dest: Path) -> Path | str:
        try:
            client = self._get_client()
            response = await client.get(url)
            response.raise_for_status()
            tmp = dest.with_suffix(".tmp")
            tmp.write_bytes(response.content)
            tmp.rename(dest)
            return dest
        except (httpx.HTTPError, httpx.TimeoutException, OSError) as e:
            logger.debug("Thumbnail download failed for %s: %s", url, e)
            return PLACEHOLDER
