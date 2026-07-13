export type ContentContext = "comment" | "review" | "forum" | "other";

export interface CheckRequest {
  /** Content to classify. */
  text: string;
  /** Defaults to `other` on the server. */
  context?: ContentContext;
  author_ip?: string;
  author_email?: string;
  author_name?: string;
}

export interface CheckResponse {
  spam: number;
  slop: number;
  reasons: string[];
}

export type FeedbackVerdict = "ham" | "spam";

export interface FeedbackResponse {
  accepted: true;
}

export interface RequestOptions {
  signal?: AbortSignal;
}

export type Fetch = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Response>;

export interface ChaffnetClientOptions {
  /** Self-hosted or hosted API root, with an optional path prefix. */
  baseUrl: string | URL;
  /** Sent as an `Authorization: Bearer` credential when present. */
  apiKey?: string;
  /** Request timeout in milliseconds. Defaults to 10 seconds. */
  timeoutMs?: number;
  /** Custom fetch implementation for tests or non-standard runtimes. */
  fetch?: Fetch;
  /** Additional headers applied before protected SDK headers. */
  headers?: HeadersInit;
}
