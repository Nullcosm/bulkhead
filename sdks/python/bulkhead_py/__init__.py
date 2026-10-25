"""Bulkhead Python SDK — wrap agent tools so every call is adjudicated by the gateway."""

from .client import GuardrailsClient, Verdict
from .guard import BlockedAction, guard, set_default_client

__all__ = [
    "GuardrailsClient",
    "Verdict",
    "guard",
    "set_default_client",
    "BlockedAction",
]
__version__ = "0.1.0"
