import { z } from "zod";

/**
 * Authentication / principal DTOs shared across the API↔Studio boundary.
 *
 * These are plain wire shapes — they carry NO auth machinery (no hashing, no sessions,
 * no guards) and the engine never imports them for execution. A control plane
 * owns all of that; contracts only describes what crosses the wire.
 */

/**
 * Role a principal holds WITHIN a tenant (RBAC). Hierarchy: `owner ⊃ approver ⊃ viewer`.
 * It is TENANT-SCOPED and resolved SERVER-SIDE on every request — it is never read from a
 * JWT claim or a client-supplied header. The wire carries it only as an enrichment of the
 * principal for the current tenant (so the Studio can gate UI cosmetically); the server's
 * 403 remains the only authority.
 */
export const TenantRoleSchema = z.enum(["owner", "approver", "viewer"]);

/** The authenticated principal, as returned by `GET /auth/me`. Never includes secrets. */
export const PrincipalDtoSchema = z.object({
  id: z.string().min(1),
  email: z.string().email(),
  displayName: z.string().min(1).nullable(),
  /** The principal's role in the CURRENT tenant, resolved server-side (never from the JWT). */
  currentTenantRole: TenantRoleSchema
});

/** Login credentials posted to `POST /auth/login`. */
export const LoginDtoSchema = z.object({
  email: z.string().email(),
  password: z.string().min(1)
});

/** Accept an invitation: the signed invite token + the password the new member chooses. */
export const AcceptInviteDtoSchema = z.object({
  token: z.string().min(1),
  password: z.string().min(8)
});

/** Issued session: the bearer token plus its absolute expiry (ISO 8601). */
export const SessionDtoSchema = z.object({
  token: z.string().min(1),
  expiresAt: z.string().datetime()
});

export type TenantRole = z.infer<typeof TenantRoleSchema>;
export type PrincipalDto = z.infer<typeof PrincipalDtoSchema>;
export type LoginDto = z.infer<typeof LoginDtoSchema>;
export type AcceptInviteDto = z.infer<typeof AcceptInviteDtoSchema>;
export type SessionDto = z.infer<typeof SessionDtoSchema>;
