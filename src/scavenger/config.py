import tomllib
from pathlib import Path
from pydantic import BaseModel, ValidationError
from scavenger.models import Profile
from scavenger.ai.models import AIConfig

try:
    import tomli_w
except ImportError:
    tomli_w = None  # type: ignore[assignment]


class GlobalConfig(BaseModel):
    db_path: str = "~/.local/share/scavenger/scavenger.db"
    image_cache_path: str = "~/.cache/scavenger/images"
    log_level: str = "INFO"
    socket_path: str = "~/.run/scavenger/daemon.sock"
    home_zip: str | None = None
    tui_refresh_sec: float = 2.0


class AppConfig(BaseModel):
    global_config: GlobalConfig = GlobalConfig()
    profiles: list[Profile]

    @property
    def log_level(self) -> str:
        return self.global_config.log_level

    @property
    def db_path(self) -> Path:
        return Path(self.global_config.db_path).expanduser()

    @property
    def socket_path(self) -> Path:
        return Path(self.global_config.socket_path).expanduser()


class ConfigError(Exception):
    pass


def load_config(path: Path) -> AppConfig:
    try:
        raw = path.read_text()
    except FileNotFoundError:
        raise ConfigError(f"Config file not found: {path}")
    except OSError as e:
        raise ConfigError(f"Cannot read config: {e}")

    try:
        data = tomllib.loads(raw)
    except tomllib.TOMLDecodeError as e:
        raise ConfigError(f"Invalid TOML: {e}")

    try:
        global_data = data.get("global", {})
        profiles_data = data.get("profiles", [])
        global_config = GlobalConfig(**global_data)
        profiles = [Profile(**p) for p in profiles_data]
        return AppConfig(global_config=global_config, profiles=profiles)
    except (ValidationError, TypeError) as e:
        raise ConfigError(f"Invalid config: {e}")


def load_ai_config(path: Path) -> AIConfig:
    """Load AI config from the main config TOML's [ai] section.

    Returns disabled config if file missing or no [ai] section.
    """
    try:
        raw = path.read_text()
    except FileNotFoundError:
        return AIConfig()  # disabled by default
    except OSError as e:
        raise ConfigError(f"Cannot read AI config: {e}")
    try:
        data = tomllib.loads(raw)
    except tomllib.TOMLDecodeError as e:
        raise ConfigError(f"Invalid AI config TOML: {e}")
    ai_data = data.get("ai", {})
    if not ai_data:
        return AIConfig()  # no [ai] section = disabled
    try:
        return AIConfig(**ai_data)
    except Exception as e:
        raise ConfigError(f"Invalid AI config: {e}")


def _read_config_data(path: Path) -> dict:
    try:
        raw = path.read_text()
        return tomllib.loads(raw)
    except FileNotFoundError:
        return {}
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise ConfigError(f"Cannot read config: {e}")


def _profile_to_entry(profile: Profile) -> dict:
    entry: dict = {
        "id": profile.id,
        "name": profile.name,
        "keywords": profile.keywords,
        "sources": profile.sources,
        "enabled": profile.enabled,
    }
    if profile.negative_keywords:
        entry["negative_keywords"] = profile.negative_keywords
    if profile.price_min is not None:
        entry["price_min"] = profile.price_min
    if profile.price_max is not None:
        entry["price_max"] = profile.price_max
    if profile.poll_interval_sec != 3600:
        entry["poll_interval_sec"] = profile.poll_interval_sec
    if profile.alert_priority != "normal":
        entry["alert_priority"] = profile.alert_priority
    if profile.tags:
        entry["tags"] = profile.tags
    if profile.escalation_keywords:
        entry["escalation_keywords"] = profile.escalation_keywords
    if profile.location_radius_mi is not None:
        entry["location_radius_mi"] = profile.location_radius_mi
    return entry


def append_profile(path: Path, profile_data: dict) -> Profile:
    """Append a new profile to the config TOML and return the validated Profile."""
    if tomli_w is None:
        raise ConfigError("tomli_w not installed — run: uv sync --all-extras")

    data = _read_config_data(path)
    profile = Profile(**profile_data)

    existing = data.get("profiles", [])
    if any(p.get("id") == profile.id for p in existing):
        raise ConfigError(f"Profile ID already exists: {profile.id}")

    existing.append(_profile_to_entry(profile))
    data["profiles"] = existing
    path.write_text(tomli_w.dumps(data))
    return profile


def update_profile(path: Path, profile_data: dict) -> Profile:
    """Update an existing profile in the config TOML."""
    if tomli_w is None:
        raise ConfigError("tomli_w not installed — run: uv sync --all-extras")

    data = _read_config_data(path)
    profile = Profile(**profile_data)

    existing = data.get("profiles", [])
    idx = next((i for i, p in enumerate(existing) if p.get("id") == profile.id), None)
    if idx is None:
        raise ConfigError(f"Profile not found: {profile.id}")

    existing[idx] = _profile_to_entry(profile)
    data["profiles"] = existing
    path.write_text(tomli_w.dumps(data))
    return profile


def delete_profile(path: Path, profile_id: str) -> None:
    """Remove a profile from the config TOML."""
    if tomli_w is None:
        raise ConfigError("tomli_w not installed — run: uv sync --all-extras")

    data = _read_config_data(path)
    existing = data.get("profiles", [])
    filtered = [p for p in existing if p.get("id") != profile_id]
    if len(filtered) == len(existing):
        raise ConfigError(f"Profile not found: {profile_id}")
    data["profiles"] = filtered
    path.write_text(tomli_w.dumps(data))
