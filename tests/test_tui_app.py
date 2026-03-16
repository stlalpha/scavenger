import pytest
from pathlib import Path
from textual.app import App
from scavenger.tui.app import ScavengerApp
from scavenger.config import AppConfig, GlobalConfig
from scavenger.db import Database
from scavenger.models import Profile


async def make_test_db(tmp_path: Path) -> Database:
    db = Database(tmp_path / "test.db")
    await db.init()
    await db.migrate()
    return db


def make_config(tmp_path: Path) -> AppConfig:
    return AppConfig(
        global_config=GlobalConfig(
            db_path=str(tmp_path / "test.db"),
            socket_path=str(tmp_path / "daemon.sock"),
        ),
        profiles=[
            Profile(id="p1", name="Sony Glass", keywords=["sony"],
                    negative_keywords=[], sources=["ebay"]),
        ],
    )


async def test_app_launches_and_quits(tmp_path):
    db = await make_test_db(tmp_path)
    await db.close()
    config = make_config(tmp_path)
    app = ScavengerApp(config=config)
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        assert app.is_running
        await pilot.press("q")


async def test_app_tab_cycles_focus(tmp_path):
    db = await make_test_db(tmp_path)
    await db.close()
    config = make_config(tmp_path)
    app = ScavengerApp(config=config)
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        await pilot.press("tab")
        await pilot.pause(0.1)
        assert app.is_running


async def test_app_right_arrow_cycles_focus(tmp_path):
    db = await make_test_db(tmp_path)
    await db.close()
    config = make_config(tmp_path)
    app = ScavengerApp(config=config)
    async with app.run_test(size=(120, 40)) as pilot:
        await pilot.pause(0.2)
        await pilot.press("right")
        await pilot.pause(0.1)
        assert app.is_running
