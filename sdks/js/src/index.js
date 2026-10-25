/**
 * Bulkhead JavaScript SDK — adjudicate agent actions via the gateway (API v1).
 *
 * Fails CLOSED: no endpoint configured, or any transport failure, rejects
 * the call. A broken guardrail must never mean "no guardrail".
 */

let defaultClient = null;

/**
 * @param {object} options
 * @param {string} options.agentId - stable agent identifier
 * @param {string} [options.endpoint] - gateway base URL (defaults to $BULKHEAD_ENDPOINT)
 * @param {string} [options.token] - short-lived agent token (defaults to $BULKHEAD_TOKEN)
 */
export class GuardrailsClient {
  constructor({
    agentId,
    endpoint = process.env.BULKHEAD_ENDPOINT ?? "",
    token = process.env.BULKHEAD_TOKEN ?? "",
  } = {}) {
    if (!agentId) throw new Error("agentId is required");
    if (!endpoint) {
      throw new Error(
        "BULKHEAD_ENDPOINT is not set: refusing to run agent actions ungoverned.",
      );
    }
    if (!token) {
      throw new Error(
        "BULKHEAD_TOKEN is not set: refusing to run agent actions without an identity.",
      );
    }
    this.agentId = agentId;
    this.endpoint = endpoint.replace(/\/+$/, "");
    this.token = token;
  }

  /**
   * Adjudicate one action: POST {endpoint}/v1/check
   * @param {string} tool
   * @param {object} [params]
   * @param {string|null} [resource]
   * @returns {Promise<{allowed: boolean, reason: string, ruleId: string|null, remainingCents: number|null}>}
   */
  async checkAction(tool, params = {}, resource = null) {
    const res = await fetch(`${this.endpoint}/v1/check`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        agent: this.agentId,
        token: this.token,
        tool,
        params: JSON.stringify(params),
        resource,
      }),
    });
    if (!res.ok) throw new Error(`bulkhead gateway responded ${res.status}`);
    const data = await res.json();
    return {
      allowed: data.verdict.allow,
      reason: data.verdict.reason,
      ruleId: data.verdict.rule_id ?? null,
      remainingCents: data.remaining_cents ?? null,
    };
  }
}

/** Raised when the gateway denies an action. */
export class BlockedActionError extends Error {
  /**
   * @param {string} tool
   * @param {{reason: string, ruleId: string|null}} verdict
   */
  constructor(tool, verdict) {
    const rule = verdict.ruleId ? ` (rule ${verdict.ruleId})` : "";
    super(`bulkhead blocked '${tool}': ${verdict.reason}${rule}`);
    this.tool = tool;
    this.verdict = verdict;
  }
}

/** Register the client used by guards without an explicit client. */
export function setDefaultClient(client) {
  defaultClient = client;
}

/**
 * Wrap a tool function: adjudicate, then execute only if allowed.
 * @param {string} tool
 * @param {string|null} [resource]
 * @param {GuardrailsClient|null} [client]
 */
export function guardTool(tool, resource = null, client = null) {
  return (fn) =>
    async (...args) => {
      const active = client ?? defaultClient;
      if (!active) {
        throw new Error(
          "no GuardrailsClient configured: call setDefaultClient(...) first",
        );
      }
      const params = args.length === 1 ? args[0] : args;
      const verdict = await active.checkAction(tool, params, resource);
      if (!verdict.allowed) {
        throw new BlockedActionError(tool, verdict);
      }
      return fn(...args);
    };
}

export default GuardrailsClient;
