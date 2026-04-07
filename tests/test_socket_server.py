import asyncio
import json
import tempfile
import pytest
from pathlib import Path
from scavenger.daemon.socket_server import SocketServer


@pytest.fixture
async def server():
    # Use /tmp directly — macOS tmp_path is too long for AF_UNIX (104-byte limit)
    with tempfile.TemporaryDirectory(dir="/tmp") as td:
        sock_path = Path(td) / "s.sock"
        srv = SocketServer(sock_path)
        await srv.start()
        yield srv, sock_path
        await srv.stop()


async def send_command(sock_path: Path, command: dict) -> dict:
    reader, writer = await asyncio.open_unix_connection(str(sock_path))
    writer.write(json.dumps(command).encode() + b"\n")
    await writer.drain()
    response = await reader.readline()
    writer.close()
    await writer.wait_closed()
    return json.loads(response)


async def test_status_command(server):
    srv, sock_path = server
    resp = await send_command(sock_path, {"command": "status"})
    assert resp["status"] == "ok"


async def test_unknown_command_returns_error(server):
    srv, sock_path = server
    resp = await send_command(sock_path, {"command": "bogus"})
    assert resp["status"] == "error"


async def test_poll_command_triggers_handler(server):
    srv, sock_path = server
    called = []
    async def handler(profile_id: str): called.append(profile_id)
    srv.register_poll_handler(handler)
    await send_command(sock_path, {"command": "poll", "profile_id": "sony"})
    await asyncio.sleep(0.05)
    assert "sony" in called


async def test_oversized_input_rejected(server):
    srv, sock_path = server
    reader, writer = await asyncio.open_unix_connection(str(sock_path))
    writer.write(b"x" * 131072 + b"\n")
    try:
        await writer.drain()
    except (BrokenPipeError, ConnectionResetError):
        pass
    response = await reader.readline()
    writer.close()
    await writer.wait_closed()
    resp = json.loads(response)
    assert resp["status"] == "error"
