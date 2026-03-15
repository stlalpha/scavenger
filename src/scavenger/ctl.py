import asyncio
import json
import socket
import sys
from pathlib import Path

import click

from scavenger.config import load_config, ConfigError

DEFAULT_CONFIG = Path("~/.config/scavenger/config.toml").expanduser()


def _send(socket_path: Path, command: dict) -> dict:
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(str(socket_path))
        sock.sendall(json.dumps(command).encode() + b"\n")
        data = b""
        while not data.endswith(b"\n"):
            chunk = sock.recv(4096)
            if not chunk:
                break
            data += chunk
        sock.close()
        return json.loads(data)
    except FileNotFoundError:
        return {"status": "error", "message": "Daemon not running"}
    except ConnectionRefusedError:
        return {"status": "error", "message": "Daemon not running"}


@click.group()
@click.option("--config", "config_path", default=str(DEFAULT_CONFIG), type=click.Path())
@click.pass_context
def cli(ctx, config_path):
    """scavenger-ctl — control the SCAVENGER daemon."""
    ctx.ensure_object(dict)
    ctx.obj["config_path"] = Path(config_path)


@cli.command("list-profiles")
@click.pass_context
def list_profiles(ctx):
    """List all configured interest profiles."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    for p in config.profiles:
        state = "enabled" if p.enabled else "disabled"
        click.echo(f"  [{state}] {p.name} ({p.id})")
        click.echo(f"           sources: {', '.join(p.sources)}  priority: {p.alert_priority}")


@cli.command()
@click.pass_context
def status(ctx):
    """Show daemon status."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    resp = _send(config.socket_path, {"command": "status"})
    click.echo("running" if resp["status"] == "ok" else f"error: {resp.get('message')}")


@cli.command()
@click.argument("profile_name")
@click.pass_context
def poll(ctx, profile_name):
    """Trigger immediate poll for a profile."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    matching = [p for p in config.profiles if p.name == profile_name or p.id == profile_name]
    if not matching:
        click.echo(f"Error: profile not found: {profile_name}", err=True)
        sys.exit(1)
    resp = _send(config.socket_path, {"command": "poll", "profile_id": matching[0].id})
    click.echo("ok" if resp["status"] == "ok" else f"error: {resp.get('message')}")


@cli.command()
@click.pass_context
def stop(ctx):
    """Stop the daemon."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    resp = _send(config.socket_path, {"command": "shutdown"})
    click.echo("ok" if resp["status"] == "ok" else f"error: {resp.get('message')}")


@cli.command()
@click.pass_context
def start(ctx):
    """Start the daemon."""
    try:
        config = load_config(ctx.obj["config_path"])
    except ConfigError as e:
        click.echo(f"Error: {e}", err=True)
        sys.exit(1)
    from scavenger.daemon.main import Daemon
    click.echo("Starting SCAVENGER daemon...")
    asyncio.run(Daemon(config).run())
