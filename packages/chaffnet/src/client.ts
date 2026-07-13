import {
  ChaffnetApiError,
  ChaffnetProtocolError,
  ChaffnetTimeoutError,
} from "./errors.js";
import type {
  ChaffnetClientOptions,
  CheckRequest,
  CheckResponse,
  Fetch,
  FeedbackResponse,
  FeedbackVerdict,
  RequestOptions,
} from "./types.js";

const DEFAULT_TIMEOUT_MS = 10_000;
const MAX_BATCH_ITEMS = 1_000;

function normalizeBaseUrl(value: string | URL): URL {
  const url = new URL(value);
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new TypeError("baseUrl must use http or https");
  }
  if (url.username || url.password) {
    throw new TypeError("baseUrl must not contain credentials");
  }
  url.search = "";
  url.hash = "";
  if (!url.pathname.endsWith("/")) {
    url.pathname += "/";
  }
  return url;
}

function isProbability(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isFinite(value) &&
    value >= 0 &&
    value <= 1
  );
}

function parseCheckResponse(value: unknown): CheckResponse {
  if (
    typeof value !== "object" ||
    value === null ||
    !("spam" in value) ||
    !isProbability(value.spam) ||
    !("slop" in value) ||
    !isProbability(value.slop) ||
    !("reasons" in value) ||
    !Array.isArray(value.reasons) ||
    !value.reasons.every((reason) => typeof reason === "string")
  ) {
    throw new ChaffnetProtocolError("chaffnet returned an invalid check response");
  }
  return {
    spam: value.spam,
    slop: value.slop,
    reasons: [...value.reasons],
  };
}

function parseFeedbackResponse(value: unknown): FeedbackResponse {
  if (
    typeof value !== "object" ||
    value === null ||
    !("accepted" in value) ||
    value.accepted !== true
  ) {
    throw new ChaffnetProtocolError("chaffnet returned an invalid feedback response");
  }
  return { accepted: true };
}

async function readErrorBody(response: Response): Promise<unknown> {
  const text = await response.text();
  if (text.length === 0) {
    return undefined;
  }
  if (response.headers.get("content-type")?.includes("json")) {
    try {
      return JSON.parse(text) as unknown;
    } catch {
      return text;
    }
  }
  return text;
}

async function readJson(response: Response): Promise<unknown> {
  try {
    return (await response.json()) as unknown;
  } catch (error) {
    throw new ChaffnetProtocolError("chaffnet returned invalid JSON", {
      cause: error,
    });
  }
}

export class ChaffnetClient {
  readonly #baseUrl: URL;
  readonly #apiKey: string | undefined;
  readonly #timeoutMs: number;
  readonly #fetch: Fetch;
  readonly #headers: Headers;

  constructor(options: ChaffnetClientOptions) {
    this.#baseUrl = normalizeBaseUrl(options.baseUrl);
    this.#timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    if (!Number.isFinite(this.#timeoutMs) || this.#timeoutMs <= 0) {
      throw new TypeError("timeoutMs must be a positive finite number");
    }
    if (options.apiKey !== undefined && options.apiKey.trim().length === 0) {
      throw new TypeError("apiKey must not be empty");
    }
    this.#apiKey = options.apiKey;
    this.#fetch = options.fetch ?? globalThis.fetch.bind(globalThis);
    this.#headers = new Headers(options.headers);
  }

  async check(
    request: CheckRequest,
    options: RequestOptions = {},
  ): Promise<CheckResponse> {
    const value = await this.#post("v1/check", request, options.signal);
    return parseCheckResponse(value);
  }

  async checkBatch(
    items: readonly CheckRequest[],
    options: RequestOptions = {},
  ): Promise<CheckResponse[]> {
    if (items.length > MAX_BATCH_ITEMS) {
      throw new RangeError(`batch exceeds ${MAX_BATCH_ITEMS} items`);
    }
    const value = await this.#post("v1/check/batch", { items }, options.signal);
    if (
      typeof value !== "object" ||
      value === null ||
      !("results" in value) ||
      !Array.isArray(value.results) ||
      value.results.length !== items.length
    ) {
      throw new ChaffnetProtocolError("chaffnet returned an invalid batch response");
    }
    return value.results.map(parseCheckResponse);
  }

  async submitFeedback(
    content: CheckRequest,
    verdict: FeedbackVerdict,
    options: RequestOptions = {},
  ): Promise<FeedbackResponse> {
    const value = await this.#post(
      "v1/feedback",
      { content, verdict },
      options.signal,
    );
    return parseFeedbackResponse(value);
  }

  async #post(path: string, body: unknown, signal?: AbortSignal): Promise<unknown> {
    const controller = new AbortController();
    let timedOut = false;
    const abortFromCaller = () => controller.abort(signal?.reason);
    if (signal?.aborted) {
      abortFromCaller();
    } else {
      signal?.addEventListener("abort", abortFromCaller, { once: true });
    }
    const timeout = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, this.#timeoutMs);

    const headers = new Headers(this.#headers);
    headers.set("accept", "application/json");
    headers.set("content-type", "application/json");
    if (this.#apiKey !== undefined) {
      headers.set("authorization", `Bearer ${this.#apiKey}`);
    }

    try {
      const response = await this.#fetch(new URL(path, this.#baseUrl), {
        method: "POST",
        headers,
        body: JSON.stringify(body),
        signal: controller.signal,
      });
      if (!response.ok) {
        throw new ChaffnetApiError(
          response.status,
          await readErrorBody(response),
          response.headers,
        );
      }
      return await readJson(response);
    } catch (error) {
      if (timedOut) {
        throw new ChaffnetTimeoutError(this.#timeoutMs, { cause: error });
      }
      throw error;
    } finally {
      clearTimeout(timeout);
      signal?.removeEventListener("abort", abortFromCaller);
    }
  }
}
