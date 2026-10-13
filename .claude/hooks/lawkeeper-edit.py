#!/usr/bin/env python3
import json
import os
import subprocess
import sys

def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except json.JSONDecodeError as e:
        sys.stderr.write(f"lawkeeper hook: bad stdin json: {e}\n")
        return 1
    path = (payload.get("tool_input") or {}).get("file_path") or ""
    if not path.endswith(".rs"):
        return 0
    if "/src/" not in path:
        return 0
    root = os.environ.get("CLAUDE_PROJECT_DIR", "")
    if root and not os.path.realpath(path).startswith(os.path.realpath(root) + os.sep):
        return 0
    try:
        proc = subprocess.run(["lawkeeper", path], capture_output=True, text=True, timeout=30)
    except FileNotFoundError:
        sys.stderr.write("lawkeeper hook: `lawkeeper` not in PATH\n")
        return 1
    except subprocess.TimeoutExpired:
        sys.stderr.write("lawkeeper hook: timed out\n")
        return 1
    if proc.returncode == 0:
        return 0
    sys.stderr.write(proc.stderr or proc.stdout)
    return 2 if proc.returncode == 2 else 1

if __name__ == "__main__":
    sys.exit(main())
