import { describe, expect, it } from "vitest";

import { parseTokenFromHash } from "./client.ts";

describe("parseTokenFromHash", () => {
  it("reads the token", () => {
    expect(parseTokenFromHash("#token=abc123")).toBe("abc123");
  });
  it("works among other params", () => {
    expect(parseTokenFromHash("#a=1&token=x%2By")).toBe("x+y");
  });
  it("returns null without a token", () => {
    expect(parseTokenFromHash("")).toBeNull();
    expect(parseTokenFromHash("#token=")).toBeNull();
    expect(parseTokenFromHash("#other=1")).toBeNull();
  });
});
