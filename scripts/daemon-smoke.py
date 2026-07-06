#!/usr/bin/env python3
"""Mission ports P2b/P3a: boot lecternd and drive a REAL session loop over its
native transport — unix socket on Linux/macOS, 127.0.0.1+token on Windows.
Run by CI on all three OSes; asserts ping → models → run(mock) → sessions →
history. Exit 0 = the daemon stack genuinely works on this OS."""
import json, os, socket, subprocess, sys, tempfile, time, pathlib

# Windows runners default to cp1252 — the checkmarks crashed the FIRST run
# while the transport itself was passing. UTF-8 or bust.
sys.stdout.reconfigure(encoding="utf-8", errors="replace")

REPO = pathlib.Path(__file__).resolve().parent.parent
DAEMON = REPO / "target" / "debug" / ("lecternd.exe" if os.name == "nt" else "lecternd")
HOME = os.environ.get("USERPROFILE" if os.name == "nt" else "HOME", ".")

def connect():
    if os.name == "nt":
        meta = pathlib.Path(HOME) / ".lectern"
        port = int((meta / "lecternd.port").read_text().strip())
        token = (meta / "lecternd.token").read_text().strip()
        s = socket.create_connection(("127.0.0.1", port), timeout=10)
        return s, token
    sock = (os.environ.get("XDG_RUNTIME_DIR") or "/tmp") + "/lectern/lecternd.sock"
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(10)
    s.connect(sock)
    return s, None

def rpc(method, params, stream=False):
    s, token = connect()
    req = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    if token:
        req["token"] = token
    s.sendall((json.dumps(req) + "\n").encode())
    buf, result, events = b"", None, []
    deadline = time.time() + 120
    while result is None and time.time() < deadline:
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            if not line.strip():
                continue
            m = json.loads(line)
            if m.get("method") == "event":
                events.append(m["params"].get("type"))
            elif "result" in m:
                result = m["result"]
    s.close()
    return (result, events) if stream else result

def main():
    assert DAEMON.exists(), f"build lecternd first: {DAEMON}"
    proc = subprocess.Popen([str(DAEMON)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    try:
        for _ in range(50):
            time.sleep(0.2)
            try:
                if rpc("ping", {}) == "pong":
                    break
            except OSError:
                continue
        else:
            sys.exit("daemon never answered ping")
        print("✓ ping")

        models = rpc("models", {})
        assert isinstance(models, dict) and "claude" in models, f"models: {models}"
        print(f"✓ models (claude={len(models['claude'])}, opencode={len(models['opencode'])})")

        ws = tempfile.mkdtemp(prefix="lectern-smoke-")
        (pathlib.Path(ws) / "readme.md").write_text("smoke\n")
        result, events = rpc("run", {"path": ws, "prompt": "daemon smoke", "backend": "mock"}, stream=True)
        assert result and result.get("changes") == 1, f"run: {result}"
        assert "message" in events and "done" in events, f"events: {events}"
        print(f"✓ run (events: {' → '.join(events[:6])}…)")

        sessions = rpc("sessions", {"path": ws, "limit": 5})
        assert isinstance(sessions, list) and sessions, f"sessions: {sessions}"
        print(f"✓ sessions ({len(sessions)})")

        history = rpc("history", {"session_id": sessions[0]["id"]})
        assert isinstance(history, list) and len(history) >= 5, f"history: {len(history) if isinstance(history, list) else history}"
        print(f"✓ history ({len(history)} events)")
        print("DAEMON SMOKE: PASS")
    finally:
        proc.kill()

if __name__ == "__main__":
    main()
