# ADR 0010 — Admin-managed config & secrets (LLM providers, connector apps)

- Status: **Accepted** (control plane — private)
- Date: 2026-06-21

## Context

A self-hosting client has no access to `.env`. Tenant-level config — LLM provider keys,
connector OAuth app credentials, email/billing — is currently env-only, so a client can't set
it. It must become **admin-managed**: per-tenant, in the Studio, encrypted at rest. Infra
secrets (DB/Redis/JWT/the master encryption key/Clerk/Sentry) stay deploy-only.

## Decision

A unified, owner-only **Administration** section. Two new admin-managed secret stores; the rest
(members, PII/router policies, connectors connections, billing) already exist and are consolidated.

### Secret stores (per-tenant, AES-256-GCM at rest via `connectors.crypto`)
- `llm_provider_keys` — { tenantId, provider, encApiKey, baseUrl?, defaultModel?, tier? }.
- `connector_app_credentials` — { tenantId, provider, encClientId, encClientSecret } (the OAuth
  *app* creds; per-user connections stay in `connector_connections`).

The master `CONNECTOR_ENC_KEY` stays **infra** (deploy secret). Keys are **write-only** over the
API (never returned in plaintext; show last-4). Owner-only (`@Roles("owner")`).

### LLM key injection (the crux) — Option B
The Rust engine reads provider keys from `std::env::var` in `bridge.rs::build_gateway`;
`AgentSpec` carries no credentials. We add an optional `providerKeys: Map<string,string>` to
`AgentSpec`; `build_gateway` uses a spec key when present, falling back to env. `RunsService`
decrypts the tenant's keys and passes them per-run. Per-run scoped → no cross-tenant leak (vs
setting `process.env`, which is shared and unsafe). **Cost: a napi rebuild.** The TS gateway
path builds its adapters from the same keys.

### Connector OAuth app creds
`connectors.service` reads client id/secret from `connector_app_credentials` (tenant) first,
falling back to env — so a self-host client registers their own OAuth apps via a form.

### Admin UI
Studio `/admin` (owner-only): tabs Providers (LLM) · Connectors/Sources · Members · Plan &
Billing · Privacy (PII) · Routing · Settings. Reuses existing panels + the two new forms.

## Security
- Secrets encrypted at rest; the master key is infra. Write-only API (last-4 display); owner-only.
- Per-run key passing is in-process (JS→Rust napi), not over a network; scoped to one run.
- `.env` remains the **fallback** for every admin-managed value (dev + ops), so nothing breaks
  when a tenant hasn't set it.

## Reversibility
Each store falls back to env, so the feature is additive. Dropping it = stop reading the tables.
The injection seam is one optional spec field + one `build_gateway` branch.
