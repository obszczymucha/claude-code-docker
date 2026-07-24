#!/usr/bin/env python3
"""Host-side bridge for the claude-code container. Zero dependencies.

Two jobs:
 1. MCP server (streamable HTTP, JSON responses) exposing netsync, served on
    127.0.0.1:8737 and on the docker bridge IP so the container can reach it
    at http://host.docker.internal:8737/mcp.
 2. IDE proxy: watches ~/.claude/ide/*.lock (filename = port of a Neovim
    websocket server bound to host loopback) and forwards
    <bridge-ip>:<port> -> 127.0.0.1:<port>, since claudecode.nvim only binds
    127.0.0.1 and the container is on a bridge network.

Only 127.0.0.1 and the docker bridge IP are bound; nothing is exposed to the
LAN. Listeners use IP_FREEBIND so the daemon may start before docker does.
"""

import functools
import json
import os
import re
import shutil
import socket
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

BRIDGE_IP = os.environ.get("CLAUDE_BRIDGE_IP", "172.17.0.1")  # docker0
MCP_PORT = 8737
IDE_LOCK_DIR = Path.home() / ".claude" / "ide"
LOCK_SCAN_INTERVAL = 2.0
COMMAND_TIMEOUT_SECONDS = 300
DIRECTORY_NAME = re.compile(r"^[A-Za-z0-9._ -]+$")
IP_FREEBIND = getattr(socket, "IP_FREEBIND", 15)  # constant added in py3.12

print = functools.partial(print, flush=True)  # immediate logs under systemd

TOOLS = [
    {
        "name": "netsync_copy_directory",
        "description": (
            "Sync a World of Warcraft addon directory by running "
            "`netsync copy-directory <directory>` on the host. Use after "
            "editing addon files, e.g. directory=AutoInvite."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "directory": {
                    "type": "string",
                    "description": "Addon directory name, e.g. AutoInvite",
                }
            },
            "required": ["directory"],
        },
    }
]


def freebind_listener(host, port):
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.setsockopt(socket.IPPROTO_IP, IP_FREEBIND, 1)
    sock.bind((host, port))
    sock.listen(16)
    return sock


# --- MCP -------------------------------------------------------------------

def run_netsync(arguments):
    directory = str(arguments.get("directory", ""))
    if not DIRECTORY_NAME.match(directory):
        raise ValueError(f"invalid directory name: {directory!r}")
    netsync = shutil.which("netsync")
    if netsync is None:
        raise RuntimeError("netsync not found on PATH of the MCP server process")
    proc = subprocess.run(
        [netsync, "copy-directory", directory],
        capture_output=True,
        text=True,
        timeout=COMMAND_TIMEOUT_SECONDS,
    )
    output = (proc.stdout + proc.stderr).strip() or "(no output)"
    if proc.returncode != 0:
        output = f"netsync exited with code {proc.returncode}\n{output}"
    return {
        "content": [{"type": "text", "text": output}],
        "isError": proc.returncode != 0,
    }


def handle_request(message):
    method = message.get("method")
    params = message.get("params") or {}
    if method == "initialize":
        return {
            "protocolVersion": params.get("protocolVersion", "2025-06-18"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "netsync", "version": "1.0.0"},
        }
    if method == "ping":
        return {}
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        name = params.get("name")
        if name != "netsync_copy_directory":
            raise ValueError(f"unknown tool: {name}")
        try:
            return run_netsync(params.get("arguments") or {})
        except subprocess.TimeoutExpired:
            return {
                "content": [{"type": "text", "text": "netsync timed out"}],
                "isError": True,
            }
    raise LookupError(f"method not found: {method}")


class McpHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _send_json(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _send_empty(self, status):
        self.send_response(status)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        # No server-initiated stream; clients must use POST.
        self._send_empty(405)

    def do_DELETE(self):
        # Stateless server: nothing to tear down on session close.
        self._send_empty(200)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        try:
            message = json.loads(self.rfile.read(length))
        except (json.JSONDecodeError, UnicodeDecodeError):
            self._send_json(400, {
                "jsonrpc": "2.0",
                "id": None,
                "error": {"code": -32700, "message": "parse error"},
            })
            return

        if "id" not in message:  # notification
            self._send_empty(202)
            return

        try:
            result = handle_request(message)
            response = {"jsonrpc": "2.0", "id": message["id"], "result": result}
        except LookupError as exc:
            response = {
                "jsonrpc": "2.0",
                "id": message["id"],
                "error": {"code": -32601, "message": str(exc)},
            }
        except Exception as exc:
            response = {
                "jsonrpc": "2.0",
                "id": message["id"],
                "error": {"code": -32603, "message": str(exc)},
            }
        self._send_json(200, response)

    def log_message(self, format, *args):
        pass


class BoundHTTPServer(ThreadingHTTPServer):
    daemon_threads = True

    def server_bind(self):
        self.socket.setsockopt(socket.IPPROTO_IP, IP_FREEBIND, 1)
        super().server_bind()


def start_mcp_servers():
    for host in ("127.0.0.1", BRIDGE_IP):
        server = BoundHTTPServer((host, MCP_PORT), McpHandler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        print(f"MCP server listening on http://{host}:{MCP_PORT}/mcp")


# --- IDE proxy -------------------------------------------------------------

def pump(src, dst):
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
    except OSError:
        pass
    finally:
        for s in (src, dst):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass


def serve_forward(listener, port):
    while True:
        try:
            client, _ = listener.accept()
        except OSError:
            return  # listener closed
        try:
            upstream = socket.create_connection(("127.0.0.1", port), timeout=5)
        except OSError:
            client.close()
            continue
        threading.Thread(target=pump, args=(client, upstream), daemon=True).start()
        threading.Thread(target=pump, args=(upstream, client), daemon=True).start()


def watch_ide_locks():
    forwards = {}  # port -> listener socket
    while True:
        ports = set()
        try:
            for lock in IDE_LOCK_DIR.glob("*.lock"):
                if lock.stem.isdigit():
                    ports.add(int(lock.stem))
        except OSError:
            pass
        for port in ports - forwards.keys():
            try:
                listener = freebind_listener(BRIDGE_IP, port)
            except OSError as exc:
                print(f"IDE proxy: cannot listen on {BRIDGE_IP}:{port}: {exc}")
                continue
            forwards[port] = listener
            threading.Thread(
                target=serve_forward, args=(listener, port), daemon=True
            ).start()
            print(f"IDE proxy: {BRIDGE_IP}:{port} -> 127.0.0.1:{port}")
        for port in forwards.keys() - ports:
            forwards.pop(port).close()
            print(f"IDE proxy: stopped forwarding port {port}")
        time.sleep(LOCK_SCAN_INTERVAL)


if __name__ == "__main__":
    start_mcp_servers()
    watch_ide_locks()
