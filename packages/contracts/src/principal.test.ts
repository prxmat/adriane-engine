import { describe, expect, it } from "vitest";

import {
  LoginDtoSchema,
  PrincipalDtoSchema,
  SessionDtoSchema,
  TenantRoleSchema
} from "./principal.js";

describe("principal contracts", () => {
  it("accepts a valid principal", () => {
    const parsed = PrincipalDtoSchema.parse({
      id: "user-1",
      email: "a@b.com",
      displayName: "Alice",
      currentTenantRole: "owner"
    });
    expect(parsed.email).toBe("a@b.com");
    expect(parsed.currentTenantRole).toBe("owner");
  });

  it("allows a null displayName", () => {
    const parsed = PrincipalDtoSchema.parse({
      id: "user-1",
      email: "a@b.com",
      displayName: null,
      currentTenantRole: "viewer"
    });
    expect(parsed.displayName).toBeNull();
  });

  it("requires a current tenant role (resolved server-side) on the principal", () => {
    expect(() =>
      PrincipalDtoSchema.parse({ id: "user-1", email: "a@b.com", displayName: "Alice" })
    ).toThrow();
  });

  it("constrains the tenant role to the RBAC enum", () => {
    expect(TenantRoleSchema.parse("approver")).toBe("approver");
    expect(() => TenantRoleSchema.parse("superuser")).toThrow();
  });

  it("rejects a malformed email on login", () => {
    expect(() => LoginDtoSchema.parse({ email: "nope", password: "x" })).toThrow();
  });

  it("requires an ISO expiry on a session", () => {
    expect(() => SessionDtoSchema.parse({ token: "t", expiresAt: "not-a-date" })).toThrow();
    const ok = SessionDtoSchema.parse({ token: "t", expiresAt: new Date().toISOString() });
    expect(ok.token).toBe("t");
  });
});
