import { describe, expect, test } from "bun:test";
import {
  ChaffnetApiError,
  ChaffnetClient,
  ChaffnetProtocolError,
  ChaffnetTimeoutError,
  type CheckRequest,
} from "../src/index.ts";

const request: CheckRequest = {
  text: "Specific human note",
  context: "comment",
  author_email: "writer@example.com",
};

describe("ChaffnetClient", () => {
  test("checks one item with bearer authentication", async () => {
    const calls: Array<{ input: string; init?: RequestInit }> = [];
    const fetch = async (input: string | URL | Request, init?: RequestInit) => {
      calls.push({ input: String(input), init });
      return Response.json({ spam: 0.1, slop: 0.2, reasons: ["known_sender"] });
    };
    const client = new ChaffnetClient({
      baseUrl: "https://api.example.test/root/",
      apiKey: "secret",
      fetch,
    });

    const result = await client.check(request);

    expect(result).toEqual({ spam: 0.1, slop: 0.2, reasons: ["known_sender"] });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.input).toBe("https://api.example.test/root/v1/check");
    expect(calls[0]?.init?.method).toBe("POST");
    expect(new Headers(calls[0]?.init?.headers).get("authorization")).toBe(
      "Bearer secret",
    );
    expect(JSON.parse(String(calls[0]?.init?.body))).toEqual(request);
  });

  test("checks a batch and preserves result order", async () => {
    const fetch = async (_input: string | URL | Request, init?: RequestInit) => {
      expect(JSON.parse(String(init?.body))).toEqual({ items: [request, request] });
      return Response.json({
        results: [
          { spam: 0.1, slop: 0.2, reasons: [] },
          { spam: 0.8, slop: 0.3, reasons: ["high_link_ratio"] },
        ],
      });
    };
    const client = new ChaffnetClient({ baseUrl: "http://localhost:8080", fetch });

    const results = await client.checkBatch([request, request]);

    expect(results[1]?.spam).toBe(0.8);
  });

  test("throws a typed API error with a parsed response body", async () => {
    const fetch = async () =>
      Response.json(
        { title: "Too many requests", status: 429 },
        { status: 429, headers: { "retry-after": "10" } },
      );
    const client = new ChaffnetClient({ baseUrl: "https://api.example.test", fetch });

    try {
      await client.check(request);
      throw new Error("expected request to fail");
    } catch (error) {
      expect(error).toBeInstanceOf(ChaffnetApiError);
      const apiError = error as ChaffnetApiError;
      expect(apiError.status).toBe(429);
      expect(apiError.body).toEqual({ title: "Too many requests", status: 429 });
      expect(apiError.headers.get("retry-after")).toBe("10");
    }
  });

  test("rejects malformed successful responses", async () => {
    const fetch = async () => Response.json({ spam: "high", slop: 0.2, reasons: [] });
    const client = new ChaffnetClient({ baseUrl: "https://api.example.test", fetch });

    await expect(client.check(request)).rejects.toBeInstanceOf(ChaffnetProtocolError);
  });

  test("aborts requests that exceed the configured timeout", async () => {
    const fetch = (_input: string | URL | Request, init?: RequestInit) =>
      new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener(
          "abort",
          () => reject(init.signal?.reason),
          { once: true },
        );
      });
    const client = new ChaffnetClient({
      baseUrl: "https://api.example.test",
      fetch,
      timeoutMs: 5,
    });

    await expect(client.check(request)).rejects.toBeInstanceOf(ChaffnetTimeoutError);
  });

  test("honors a caller-provided abort signal", async () => {
    const fetch = (_input: string | URL | Request, init?: RequestInit) =>
      new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener(
          "abort",
          () => reject(init.signal?.reason),
          { once: true },
        );
      });
    const client = new ChaffnetClient({ baseUrl: "https://api.example.test", fetch });
    const controller = new AbortController();
    const reason = new Error("caller canceled");

    const pending = client.check(request, { signal: controller.signal });
    controller.abort(reason);

    await expect(pending).rejects.toBe(reason);
  });

  test("rejects oversized batches before sending a request", async () => {
    let called = false;
    const fetch = async () => {
      called = true;
      return Response.json({ results: [] });
    };
    const client = new ChaffnetClient({ baseUrl: "https://api.example.test", fetch });

    await expect(
      client.checkBatch(Array.from({ length: 1_001 }, () => request)),
    ).rejects.toThrow("batch exceeds 1000 items");
    expect(called).toBe(false);
  });

  test("submits hosted feedback", async () => {
    const calls: Array<{ input: string; init?: RequestInit }> = [];
    const fetch = async (input: string | URL | Request, init?: RequestInit) => {
      calls.push({ input: String(input), init });
      return Response.json({ accepted: true });
    };
    const client = new ChaffnetClient({
      baseUrl: "https://api.example.test",
      apiKey: "secret",
      fetch,
    });

    const response = await client.submitFeedback(request, "spam");

    expect(response).toEqual({ accepted: true });
    expect(calls[0]?.input).toBe("https://api.example.test/v1/feedback");
    expect(JSON.parse(String(calls[0]?.init?.body))).toEqual({
      content: request,
      verdict: "spam",
    });
  });
});

describe("configuration", () => {
  test("requires an HTTP base URL", () => {
    expect(() => new ChaffnetClient({ baseUrl: "file:///tmp/chaffnet" })).toThrow(
      "baseUrl must use http or https",
    );
  });

  test("requires a positive timeout", () => {
    expect(
      () => new ChaffnetClient({ baseUrl: "https://api.example.test", timeoutMs: 0 }),
    ).toThrow("timeoutMs must be a positive finite number");
  });
});
