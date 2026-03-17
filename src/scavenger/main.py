import click
import sys
from pathlib import Path
from scavenger.config import load_config, ConfigError

DEFAULT_CONFIG = Path("~/.config/scavenger/config.toml").expanduser()


@click.command()
@click.option("--config", "config_path", default=str(DEFAULT_CONFIG), type=click.Path())
def cli(config_path: str) -> None:
    """SCAVENGER — Continuous web intelligence terminal."""
    try:
        config = load_config(Path(config_path))
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    from scavenger.tui.app import ScavengerApp
    ScavengerApp(config=config, config_path=Path(config_path)).run()
