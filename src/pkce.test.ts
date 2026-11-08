/** @format */

import { describe, expect, it } from "vitest";
import { codeChallengeS256, createCodeVerifier } from "./pkce";

describe("pkce", () => {
  it("matches the RFC 7636 appendix B example", async () => {
    expect(
      await codeChallengeS256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
    ).toBe("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
  });

  it("creates distinct url-safe verifiers of 43 characters", () => {
    const a = createCodeVerifier();
    expect(a).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(createCodeVerifier()).not.toBe(a);
  });
});
