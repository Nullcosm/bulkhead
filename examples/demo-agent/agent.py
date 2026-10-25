#!/usr/bin/env python3
"""Bulkhead demo agent — the three-moment story from docs/demo.md.

    allowed:  read a public URL            -> runs
    blocked:  write to prod DB             -> denied by policy (with rule id)
    allowed:  write to staging DB          -> passes the condition
    killed:   read after budget exhausts   -> kill switch

With BULKHEAD_ENDPOINT + BULKHEAD_TOKEN set, verdicts come from the live gateway and
execution happens only after adjudication. Without them, the script refuses
to run tools (fail closed) and prints how to start the real thing.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path
from types import SimpleNamespace

# Repo-local run: make the Python SDK importable without pip install.
_SDK_DIR = Path(__file__).resolve().parents[2] / "sdks" / "python"
if str(_SDK_DIR) not in sys.path:
    sys.path.insert(0, str(_SDK_DIR))


def main() -> None:
    endpoint = os.environ.get("BULKHEAD_ENDPOINT")
    token = os.environ.get("BULKHEAD_TOKEN")
    if endpoint and token:
        from bulkhead_py import GuardrailsClient, set_default_client

        set_default_client(GuardrailsClient("demo-agent", endpoint=endpoint, token=token))
        print("[bulkhead] adjudicating against the live gateway.\n")
        run_live()
    else:
        print(
            "[bulkhead] BULKHEAD_ENDPOINT / BULKHEAD_TOKEN not set — refusing to run tools "
            "ungoverned (fail closed).\n"
            "       Start the gateway, then:\n"
            "         export BULKHEAD_ENDPOINT=http://127.0.0.1:7373\n"
            "         export BULKHEAD_TOKEN=$(bulkhead-gateway --issue-token demo-agent)\n"
        )
        run_simulated()


def run_live() -> None:
    from bulkhead_py import BlockedAction, guard

    @guard("http.get")
    def read_url(url: str) -> str:
        return f"[executed] GET {url}"

    @guard("db.query", resource="prod.db")
    def prod_write(sql: str) -> str:
        return f"[executed] {sql}"

    @guard("db.query", resource="staging.db")
    def staging_write(sql: str) -> str:
        return f"[executed] {sql}"

    steps = [
        ("read a public URL", lambda: read_url(url="https://example.com")),
        ("write to prod DB", lambda: prod_write(sql="DELETE FROM users")),
        ("write to staging DB", lambda: staging_write(sql="INSERT INTO t VALUES (1)")),
        ("more reads until budget dies", lambda: read_url(url="https://api.example.com/a")),
        ("one read too many", lambda: read_url(url="https://api.example.com/b")),
    ]
    for story, step in steps:
        try:
            result = step()
            print(f"  {story:<30} -> ALLOWED {result}")
        except BlockedAction as blocked:
            print(f"  {story:<30} -> BLOCKED: {blocked}")


def run_simulated() -> None:
    def check(tool: str, resource: str | None) -> SimpleNamespace:
        if tool == "db.query" and resource == "prod.db":
            return SimpleNamespace(
                allowed=False,
                reason="denied by rule 'deny-prod-writes'",
                rule_id="deny-prod-writes",
            )
        if resource and resource.endswith("/b"):
            return SimpleNamespace(allowed=False, reason="budget exhausted (kill switch)", rule_id=None)
        return SimpleNamespace(allowed=True, reason="", rule_id=None)

    steps = [
        ("read a public URL", "http.get", "https://example.com"),
        ("write to prod DB", "db.query", "prod.db"),
        ("write to staging DB", "db.query", "staging.db"),
        ("more reads until budget dies", "http.get", "https://api.example.com/a"),
        ("one read too many", "http.get", "https://api.example.com/b"),
    ]
    print("[bulkhead] simulated story (no gateway configured):\n")
    for story, tool, resource in steps:
        verdict = check(tool, resource)
        if verdict.allowed:
            print(f"  {story:<30} -> ALLOWED")
        else:
            suffix = f" (rule {verdict.rule_id})" if verdict.rule_id else ""
            print(f"  {story:<30} -> BLOCKED: {verdict.reason}{suffix}")


if __name__ == "__main__":
    main()
