# JSON-RPC Server Mode

Start the CLI as a JSON-RPC server using stdin/stdout for communication:

```console
cos-cli serve
```

The server reads JSON-RPC requests from stdin and writes responses to stdout. It also publishes notifications for state changes.

## Methods

### `info`
Returns current information about apps, workspaces, outputs, and seats.

```console
echo '{"jsonrpc": "2.0", "method": "info", "id": 1}' | cos-cli serve
```

No parameters.

---

### `move`
Move an application to a specific workspace.

```json
{
  "jsonrpc": "2.0",
  "method": "move",
  "params": {
    "app_id": "firefox",
    "workspace": 1
  },
  "id": 2
}
```

**Parameters:**

- `app_id` (string, optional) — Application ID (partial match, case-insensitive)
- `index` (number, optional) — Application index from `info`
- `workspace` (number, required) — Target workspace index
- `workspace_group` (number, optional) — Workspace group index
- `output_index` (number, optional) — Output index
- `wait` (number, optional) — Seconds to wait for the app to appear

---

### `activate`
Activate an application.

```json
{
  "jsonrpc": "2.0",
  "method": "activate",
  "params": {
    "index": 0
  },
  "id": 3
}
```

**Parameters:**

- `index` (number, required) — Application index from `info`
- `seat` (number, optional) — Seat index

---

### `state`
Set the state of an application.

```json
{
  "jsonrpc": "2.0",
  "method": "state",
  "params": {
    "index": 0,
    "maximize": true
  },
  "id": 4
}
```

**Parameters:**

- `app_id` (string, optional) — Application ID (partial match, case-insensitive)
- `index` (number, optional) — Application index from `info`
- `wait` (number, optional) — Seconds to wait for the app to appear
- `maximize` / `unmaximize` (bool, optional) — Maximize state
- `minimize` / `unminimize` (bool, optional) — Minimize state
- `fullscreen` / `unfullscreen` (bool, optional) — Fullscreen state
- `sticky` / `unsticky` (bool, optional) — Sticky state

---

### `ws_activate`
Activate a workspace.

```json
{
  "jsonrpc": "2.0",
  "method": "ws_activate",
  "params": {
    "workspace": 1
  },
  "id": 5
}
```

**Parameters:**

- `workspace` (number, required) — Workspace index to activate
- `workspace_group` (number, optional) — Workspace group index

---

### `close`
Close an application window.

```json
{
  "jsonrpc": "2.0",
  "method": "close",
  "params": {
    "app_id": "firefox"
  },
  "id": 6
}
```

**Parameters:**

- `app_id` (string, optional) — Application ID (partial match, case-insensitive)
- `index` (number, optional) — Application index from `info`

## Notifications

The server publishes `state_change` notifications when workspace or window state changes:

```json
{
  "jsonrpc": "2.0",
  "method": "state_change",
  "params": {
    "state": {...}
  }
}
```

## Example: Pin apps to fixed workspaces (Python)

The following script runs `cos-cli serve` as a daemon and automatically moves known applications to their assigned workspaces as they appear:

```python
#!/usr/bin/env python3
"""Pin apps to fixed workspaces using cos-cli serve."""
import json
import subprocess
import threading

# Mapping of app_id patterns -> workspace index
RULES = {
    "telegram": 0,
    "firefox":  1,
    "wezterm":  2,
    "kodi":     3,
}

ID_COUNTER = 0

def next_id():
    global ID_COUNTER
    ID_COUNTER += 1
    return ID_COUNTER

def reader_thread(proc):
    """Read JSON-RPC notifications/responses from the server's stdout."""
    pinned = set()  # track already-moved apps so we only act on first appearance

    for raw in proc.stdout:
        raw = raw.strip()
        if not raw:
            continue
        try:
            msg = json.loads(raw)
        except json.JSONDecodeError:
            continue

        # Only handle state_change notifications (responses have an "id" field)
        if msg.get("method") != "state_change":
            continue

        apps = msg.get("params", {}).get("state", {}).get("apps", [])
        for app in apps:
            app_id = (app.get("app_id") or "").lower()

            # Skip apps that have already been pinned
            if app_id in pinned:
                continue

            for pattern, workspace in RULES.items():
                if pattern in app_id:
                    pinned.add(app_id)
                    request = {
                        "jsonrpc": "2.0",
                        "method": "move",
                        "params": {"app_id": app["app_id"], "workspace": workspace},
                        "id": next_id(),
                    }
                    proc.stdin.write(json.dumps(request) + "\n")
                    proc.stdin.flush()
                    print(f"Moved {app['app_id']} to workspace {workspace}", flush=True)
                    break

def main():
    proc = subprocess.Popen(
        ["cos-cli", "serve"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )

    t = threading.Thread(target=reader_thread, args=(proc,), daemon=True)
    t.start()

    print(f"Pinning apps: {RULES}", flush=True)
    t.join()

if __name__ == "__main__":
    main()
```
