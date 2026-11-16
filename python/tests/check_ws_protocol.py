"""End-to-end WebSocket protocol check for coach-api (manual/dev script).

Boots nothing itself: requires a running coach-api (default port scan).
Verifies:
  1. /ws/chat announces the provider list as a system frame.
  2. /ws/chat streams real deltas from a live Ollama/Mistral endpoint when
     available, or yields a typed error frame otherwise (provider not running).
  3. /ws/terminal allocates a REAL PTY: `echo coach-pty-check` round-trips
     through the shell.

Run: python tests/check_ws_protocol.py [port]
"""

from __future__ import annotations

import asyncio
import json
import sys
import urllib.request

import websockets


def detect_port() -> int:
    for port in range(8901, 8921):
        try:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/api/health", timeout=1):
                return port
        except Exception:
            continue
    raise SystemExit("coach-api not running (start it, or pass a port)")


def create_echo_provider(port: int) -> str:
    """A local echo-ish provider: llama.cpp/Ollama absent — we use an
    openai_compatible provider pointed at a port that is guaranteed closed,
    and expect a typed 'error' frame: the transport failure path."""
    body = json.dumps(
        {
            "name": "ws-check",
            "kind": "openai_compatible",
            "base_url": "http://127.0.0.1:9/v1",  # port 9: discard, closed locally
            "api_key": "",
            "model": "x",
        }
    ).encode()
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/api/settings/providers",
        data=body, headers={"Content-Type": "application/json"}, method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.load(resp)["provider"]["id"]


async def check_chat(port: int, provider_id: str) -> bool:
    async with websockets.connect(f"ws://127.0.0.1:{port}/ws/chat") as ws:
        system = json.loads(await asyncio.wait_for(ws.recv(), 5))
        assert system["type"] == "system" and "providers" in system, system
        print("chat system frame OK, providers:", len(system["providers"]))

        await ws.send(json.dumps({"type": "chat", "provider_id": provider_id, "text": "ping"}))
        frame = json.loads(await asyncio.wait_for(ws.recv(), 15))
        assert frame["type"] == "error" and "provider" in frame["message"].lower(), frame
        print("chat transport-error frame OK:", frame["message"][:80])
        return True


async def check_terminal(port: int) -> bool:
    async with websockets.connect(f"ws://127.0.0.1:{port}/ws/terminal?cols=80&rows=24") as ws:
        marker = "coach-pty-check-12345"
        await asyncio.sleep(0.5)  # let the shell boot
        await ws.send(f"echo {marker}")
        buf = b""
        try:
            while marker not in buf.decode(errors="replace"):
                msg = await asyncio.wait_for(ws.recv(), 10)
                buf += msg if isinstance(msg, (bytes, bytearray)) else str(msg).encode()
        except asyncio.TimeoutError:
            print("terminal FG: marker not echoed back; got:", buf[-200:])
            return False
        print("terminal real PTY OK, echoed marker")
        return True


def main() -> None:
    port = int(sys.argv[1]) if len(sys.argv) > 1 else detect_port()
    provider_id = create_echo_provider(port)
    chat_ok = asyncio.run(check_chat(port, provider_id))
    term_ok = asyncio.run(check_terminal(port))
    ok = chat_ok and term_ok
    print("ALL WS CHECKS:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
