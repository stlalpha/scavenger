import tomllib
from pathlib import Path
from pydantic import BaseModel, ValidationError
from scavenger.models import Profile
from scavenger.ai.models import AIConfig


class GlobalConfig(BaseModel):
    db_path: str = "~/.local/share/scavenger/scavenger.db"
    image_cache_path: str = "~/.cache/scavenger/images"
    log_level: str = "INFO"
    socket_path: str = "~/.run/scavenger/daemon.sock"
    home_zip: str | None = None


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
    """Load AI config from TOML. Returns disabled config if file missing."""
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
    try:
        return AIConfig(**data.get("ai", {}))
    except Exception as e:
        raise ConfigError(f"Invalid AI config: {e}")
