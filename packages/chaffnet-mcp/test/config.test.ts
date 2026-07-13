import { describe, expect, test } from "bun:test";
import { loadConfig } from "../src/config.ts";

describe("loadConfig", () => {
  test("loads required and optional values", () => {
    expect(
      loadConfig({
        CHAFFNET_BASE_URL: "https://api.example.test/root",
        CHAFFNET_API_KEY: "secret",
        CHAFFNET_TIMEOUT_MS: "2500",
      }),
    ).toEqual({
      baseUrl: "https://api.example.test/root",
      apiKey: "secret",
      timeoutMs: 2500,
    });
  });

  test("requires a base URL", () => {
    expect(() => loadConfig({})).toThrow("CHAFFNET_BASE_URL is required");
  });

  test.each(["0", "-1", "1.5", "nan"])(
    "rejects invalid timeout %s",
    (timeout) => {
      expect(() =>
        loadConfig({
          CHAFFNET_BASE_URL: "http://localhost:8080",
          CHAFFNET_TIMEOUT_MS: timeout,
        }),
      ).toThrow("CHAFFNET_TIMEOUT_MS must be a positive integer");
    },
  );
});
