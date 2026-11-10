#!/usr/bin/env python3
"""Forward an existing status line and retain only quota metadata for Peck."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument("--client", choices=("claude-code", "antigravity"), required=True)
parser.add_argument("--data-dir", default=str(Path.home() / "Library/Application Support/Peck"))
parser.add_argument("--forward-original")
args = parser.parse_args()
raw = sys.stdin.read()

try:
    data = json.loads(raw)
    fields = {}
    if args.client == "claude-code":
        limits = data.get("rate_limits") or {}
        fields["rate_limits"] = {
            name: {key: value[key] for key in ("used_percentage", "resets_at") if isinstance(value.get(key), (int, float))}
            for name, value in limits.items()
            if name in ("five_hour", "seven_day") and isinstance(value, dict)
        }
    else:
        fields["quota"] = {
            name: {key: value[key] for key in ("remaining_fraction", "remaining_amount", "reset_time", "disabled") if key in value and isinstance(value[key], (str, int, float, bool))}
            for name, value in (data.get("quota") or {}).items()
            if isinstance(value, dict)
        }
    account = data.get("accountId") or data.get("email")
    if isinstance(account, str):
        fields["accountId"] = account
    # Only executable paths and parent IDs; never argv, environment or session text.
    rows = subprocess.check_output(["/bin/ps", "-axo", "pid=,ppid=,comm="], text=True)
    procs = {}
    for row in rows.splitlines():
        parts = row.strip().split(None, 2)
        if len(parts) == 3:
            procs[int(parts[0])] = (int(parts[1]), parts[2])
    source_pid = os.getppid()
    cursor = source_pid
    while cursor in procs:
        parent, command = procs[cursor]
        executable = Path(command).name.lower()
        if (args.client == "claude-code" and (executable == "claude" or "/.local/share/claude/versions/" in command.lower()) and "claude.app/" not in command.lower()) or (args.client == "antigravity" and executable == "antigravity" and ".app/" not in command.lower()):
            source_pid = cursor
            break
        if parent == cursor:
            break
        cursor = parent
    output = Path(args.data_dir) / "usage-events"
    output.mkdir(parents=True, exist_ok=True)
    target = output / (args.client + ".json")
    temporary = output / (args.client + "." + str(os.getpid()) + ".tmp")
    temporary.write_text(json.dumps({"schemaVersion": 1, "client": args.client, "sourcePid": source_pid, "capturedAt": int(time.time() * 1000), "fields": fields}))
    os.chmod(temporary, 0o600)
    os.replace(temporary, target)
except (ValueError, OSError, TypeError):
    # A collector failure must not break the user's original status line.
    pass

if args.forward_original:
    try:
        result = subprocess.run(args.forward_original, shell=True, input=raw, text=True, timeout=5)
        sys.exit(result.returncode)
    except (OSError, subprocess.TimeoutExpired):
        sys.exit(1)
else:
    print("Peck · " + args.client + " 额度快照")
