"""Client for the Bulkhead gateway check API (v1).

Fail-closed: when ``BULKHEAD_ENDPOINT`` or the agent token is missing, or the
gateway is unreachable, the client refuses to execute actions rather than
running them ungoverned.
"""

from __future__ import annotations

import json
import os
import urllib.request
from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True)
class Verdict:
    """One adjudication returned by the gateway."""

    allowed: bool
    reason: str
    rule_id: Optional[str] = None


class GuardrailsClient:
    """Fail-closed guardrails client.

    Args:
        agent_id: stable agent identifier registered with the gateway.
        endpoint: gateway base URL (defaults to ``$BULKHEAD_ENDPOINT``).
        token: short-lived agent token (defaults to ``$BULKHEAD_TOKEN``). Issue
            one with ``bulkhead-gateway --issue-token <agent>``.

    Raises:
        RuntimeError: when endpoint or token is missing, or the gateway is
            unreachable — the SDK fails CLOSED, never open.
    """

    def __init__(
        self,
        agent_id: str,
        endpoint: Optional[str] = None,
        token: Optional[str] = None,
    ) -> None:
        self.agent_id = agent_id
        self.endpoint = (endpoint or os.environ.get("BULKHEAD_ENDPOINT", "")).rstrip("/")
        self.token = token or os.environ.get("BULKHEAD_TOKEN", "")
        if not self.endpoint:
            raise RuntimeError(
                "BULKHEAD_ENDPOINT is not set: refusing to run agent actions "
                "ungoverned. See docs/demo.md for how to start the gateway."
            )
        if not self.token:
            raise RuntimeError(
                "BULKHEAD_TOKEN is not set: refusing to run agent actions without "
                "an identity. Issue one with `bulkhead-gateway --issue-token <agent>`."
            )

    def check_action(
        self, tool: str, params: dict, resource: Optional[str] = None
    ) -> Verdict:
        """Ask the gateway to adjudicate one action (``POST /v1/check``).

        Returns the verdict; raises on transport failure (fail closed).
        """
        payload = json.dumps(
            {
                "agent": self.agent_id,
                "token": self.token,
                "tool": tool,
                "params": json.dumps(params),
                "resource": resource,
            }
        ).encode("utf-8")
        request = urllib.request.Request(
            f"{self.endpoint}/v1/check",
            data=payload,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=5) as response:
                data = json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            raise RuntimeError(
                f"bulkhead gateway rejected the request: {exc.code} {exc.reason}"
            ) from exc
        except Exception as exc:  # any transport failure => fail closed
            raise RuntimeError(f"bulkhead gateway unreachable: {exc}") from exc

        verdict = data["verdict"]
        return Verdict(
            allowed=verdict["allow"],
            reason=verdict["reason"],
            rule_id=verdict.get("rule_id"),
        )
