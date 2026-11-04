"""Realtime voice binding tests. No test calls a provider."""

from __future__ import annotations

import base64
import hashlib
import socket
import threading
import time
from collections.abc import Iterator

import pytest

import superglue

_WS_GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def _accept_one_websocket(listener: socket.socket) -> None:
    conn, _ = listener.accept()
    with conn:
        request = b""
        while b"\r\n\r\n" not in request:
            chunk = conn.recv(4096)
            if not chunk:
                return
            request += chunk
        key = next(
            line.split(b":", 1)[1].strip()
            for line in request.split(b"\r\n")
            if line.lower().startswith(b"sec-websocket-key")
        )
        accept = base64.b64encode(hashlib.sha1(key + _WS_GUID).digest())
        conn.sendall(
            b"HTTP/1.1 101 Switching Protocols\r\n"
            b"Upgrade: websocket\r\nConnection: Upgrade\r\n"
            b"Sec-WebSocket-Accept: " + accept + b"\r\n\r\n"
        )
        while conn.recv(4096):
            pass


@pytest.fixture
def local_ws_url() -> Iterator[str]:
    listener = socket.create_server(("127.0.0.1", 0))
    threading.Thread(target=_accept_one_websocket, args=(listener,), daemon=True).start()
    yield f"ws://127.0.0.1:{listener.getsockname()[1]}"
    listener.close()


def _client() -> superglue.Client:
    return superglue.Client(api_key="sk-test", model="gpt-4o-mini")


def test_voice_rejects_unsupported_provider() -> None:
    with pytest.raises(ValueError, match="supported provider"):
        _client().voice_connect({"provider": "anthropic"}, on_event=lambda _: None)


def test_voice_openai_requires_realtime_model() -> None:
    with pytest.raises(ValueError, match="model"):
        _client().voice_connect({"provider": "openai"}, on_event=lambda _: None)


def test_voice_config_rejects_api_key_field() -> None:
    with pytest.raises(ValueError) as error:
        _client().voice_connect(
            {"provider": "openai", "model": "gpt-realtime", "api_key": "sk-leak"},
            on_event=lambda _: None,
        )
    assert "sk-leak" not in str(error.value)


def test_closed_session_rejects_commands(local_ws_url: str) -> None:
    events: list[dict] = []
    closed = threading.Event()

    def on_event(event: dict) -> None:
        events.append(event)
        if event["type"] == "closed":
            closed.set()

    session = _client().voice_connect(
        {"provider": "openai", "model": "gpt-realtime", "base_url": local_ws_url},
        on_event=on_event,
    )
    session.send_audio(b"\x00\x00")
    session.close()
    assert closed.wait(5), events

    deadline = time.monotonic() + 5
    while True:
        try:
            session.send_text("too late")
        except RuntimeError as error:
            assert "closed" in str(error)
            break
        assert time.monotonic() < deadline, "closed session kept accepting commands"
        time.sleep(0.01)
    assert events[0]["type"] == "session_created"
