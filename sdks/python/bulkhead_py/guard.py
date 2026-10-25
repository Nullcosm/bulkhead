"""Guard decorator: adjudicate an agent tool call before executing it.

```python
from bulkhead_py import GuardrailsClient, guard, set_default_client

set_default_client(GuardrailsClient("agent-1"))

@guard("db.query", resource="prod.db")
def query(sql: str) -> str:
    ...  # only runs if the gateway allows it

query("DELETE FROM users")  # raises BlockedAction if denied
```
"""

from __future__ import annotations

import functools
from typing import Any, Callable, Optional

from .client import GuardrailsClient, Verdict


class BlockedAction(RuntimeError):
    """Raised when the gateway denies an action (fail-closed execution)."""

    def __init__(self, tool: str, verdict: Verdict) -> None:
        self.tool = tool
        self.verdict = verdict
        rule = f" (rule {verdict.rule_id})" if verdict.rule_id else ""
        super().__init__(f"bulkhead blocked '{tool}': {verdict.reason}{rule}")


_default_client: Optional[GuardrailsClient] = None


def set_default_client(client: GuardrailsClient) -> None:
    """Register the client used by undecorated-with-client guards."""
    global _default_client
    _default_client = client


def guard(
    tool: str,
    resource: Optional[str] = None,
    client: Optional[GuardrailsClient] = None,
) -> Callable[[Callable], Callable]:
    """Decorate a tool function: check, then execute only if allowed.

    The function's keyword arguments become the action params. A denial
    raises :class:`BlockedAction` — the agent framework's error path handles
    it, and the attempt is on the gateway's audit trail either way.
    """

    def decorator(fn: Callable) -> Callable:
        @functools.wraps(fn)
        def wrapper(*args: Any, **kwargs: Any) -> Any:
            active = client or _default_client
            if active is None:
                raise RuntimeError(
                    "no GuardrailsClient configured: call "
                    "bulkhead_py.guard.set_default_client(...) first"
                )
            verdict = active.check_action(tool, dict(kwargs), resource)
            if not verdict.allowed:
                raise BlockedAction(tool, verdict)
            return fn(*args, **kwargs)

        return wrapper

    return decorator
