import asyncio
import json
import logging
from pathlib import Path
from typing import Callable, Awaitable

logger = logging.getLogger(__name__)
PollHandler = Callable[[str], Awaitable[None]]


class SocketServer:
    def __init__(self, socket_path: Path):
        self._path = socket_path
        self._server: asyncio.Server | None = None
        self._status_handler: Callable[[], dict] | None = None
        self._poll_handler: PollHandler | None = None
        self._shutdown_handler: Callable[[], None] | None = None

    def register_status_handler(self, handler: Callable[[], dict]) -> None:
        self._status_handler = handler

    def register_poll_handler(self, handler: PollHandler) -> None:
        self._poll_handler = handler

    def register_shutdown_handler(self, handler: Callable[[], None]) -> None:
        self._shutdown_handler = handler

    async def start(self) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        if self._path.exists():
            self._path.unlink()
        self._server = await asyncio.start_unix_server(self._handle, path=str(self._path))

    async def stop(self) -> None:
        if self._server:
            self._server.close()
            await self._server.wait_closed()
        if self._path.exists():
            self._path.unlink()

    async def _handle(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        try:
            line = await reader.readline()
            if not line:
                return
            try:
                request = json.loads(line)
            except json.JSONDecodeError:
                response = {"status": "error", "message": "invalid JSON"}
            else:
                response = await self._dispatch(request)
            writer.write(json.dumps(response).encode() + b"\n")
            await writer.drain()
        except Exception as e:
            logger.exception("Socket error: %s", e)
        finally:
            writer.close()
            await writer.wait_closed()

    async def _dispatch(self, request: dict) -> dict:
        cmd = request.get("command")
        if cmd == "status":
            data = self._status_handler() if self._status_handler else {"state": "running"}
            return {"status": "ok", "data": data}
        elif cmd == "poll":
            profile_id = request.get("profile_id")
            if not profile_id:
                return {"status": "error", "message": "profile_id required"}
            if self._poll_handler:
                asyncio.create_task(self._poll_handler(profile_id))
            return {"status": "ok", "data": {"profile_id": profile_id}}
        elif cmd == "shutdown":
            if self._shutdown_handler:
                self._shutdown_handler()
            return {"status": "ok", "data": {"message": "shutting down"}}
        return {"status": "error", "message": f"unknown command: {cmd}"}
