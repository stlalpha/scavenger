import logging
from pathlib import Path, PurePosixPath
import httpx
from scavenger.dedup import content_hash

logger = logging.getLogger(__name__)

PLACEHOLDER = "□"
DEFAULT_CACHE_DIR = Path("~/.cache/scavenger/images").expanduser()


def _ext_from_url(url: str) -> str:
    suffix = PurePosixPath(url.split("?")[0]).suffix
    return suffix if suffix else ".jpg"


class ThumbnailCache:
    """Downloads and caches listing images. Returns Path or PLACEHOLDER string."""

    def __init__(self, cache_dir: Path = DEFAULT_CACHE_DIR) -> None:
        self._cache_dir = cache_dir
        self._cache_dir.mkdir(parents=True, exist_ok=True)

    def _cache_path(self, url: str) -> Path:
        return self._cache_dir / f"{content_hash(url)}{_ext_from_url(url)}"

    async def get(self, url: str | None) -> Path | str:
        if not url:
            return PLACEHOLDER
        cached = self._cache_path(url)
        if cached.exists():
            return cached
        return await self._download(url, cached)

    async def _download(self, url: str, dest: Path) -> Path | str:
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                response = await client.get(url)
                response.raise_for_status()
            tmp = dest.with_suffix(".tmp")
            tmp.write_bytes(response.content)
            tmp.rename(dest)
            return dest
        except (httpx.HTTPError, httpx.TimeoutException, OSError) as e:
            logger.debug("Thumbnail download failed for %s: %s", url, e)
            return PLACEHOLDER
