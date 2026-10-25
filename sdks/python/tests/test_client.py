"""Tests for the fail-closed behaviour of the Bulkhead Python SDK."""

import os
import unittest
from types import SimpleNamespace

from bulkhead_py import GuardrailsClient, guard, set_default_client, BlockedAction


class FakeClient:
    """Stand-in gateway for unit tests: programmable verdicts."""

    def __init__(self, verdicts):
        self.verdicts = list(verdicts)
        self.calls = []

    def check_action(self, tool, params, resource=None):
        self.calls.append((tool, params, resource))
        return self.verdicts.pop(0)


class TestFailClosed(unittest.TestCase):
    def test_raises_without_endpoint(self) -> None:
        os.environ.pop("BULKHEAD_ENDPOINT", None)
        os.environ.pop("BULKHEAD_TOKEN", None)
        with self.assertRaises(RuntimeError):
            GuardrailsClient("agent-1", endpoint=None, token="t")

    def test_raises_without_token(self) -> None:
        with self.assertRaises(RuntimeError):
            GuardrailsClient("agent-1", endpoint="http://127.0.0.1:7373", token=None)


class TestGuard(unittest.TestCase):
    def test_allowed_action_executes(self) -> None:
        fake = FakeClient(
            [SimpleNamespace(allowed=True, reason="", rule_id=None)]
        )
        set_default_client(fake)

        @guard("http.get")
        def fetch(url: str) -> str:
            return f"got {url}"

        self.assertEqual(fetch(url="https://example.com"), "got https://example.com")
        self.assertEqual(fake.calls[0][0], "http.get")
        self.assertEqual(fake.calls[0][1], {"url": "https://example.com"})

    def test_denied_action_raises_and_does_not_execute(self) -> None:
        fake = FakeClient(
            [
                SimpleNamespace(
                    allowed=False,
                    reason="denied by rule 'deny-prod-writes'",
                    rule_id="deny-prod-writes",
                )
            ]
        )
        set_default_client(fake)
        executed = []

        @guard("db.query", resource="prod.db")
        def query(sql: str) -> None:
            executed.append(sql)

        with self.assertRaises(BlockedAction) as ctx:
            query(sql="DELETE FROM users")
        self.assertIn("deny-prod-writes", str(ctx.exception))
        self.assertEqual(fake.calls[0][2], "prod.db")
        self.assertEqual(executed, [])

    def test_guard_without_client_fails_closed(self) -> None:
        set_default_client(None)

        @guard("http.get")
        def fetch(url: str) -> str:
            return url

        with self.assertRaises(RuntimeError):
            fetch(url="https://example.com")


if __name__ == "__main__":
    unittest.main()
