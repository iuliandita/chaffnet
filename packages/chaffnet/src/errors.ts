export class ChaffnetError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = new.target.name;
  }
}

export class ChaffnetApiError extends ChaffnetError {
  readonly status: number;
  readonly body: unknown;
  readonly headers: Headers;

  constructor(status: number, body: unknown, headers: Headers) {
    super(`chaffnet request failed with status ${status}`);
    this.status = status;
    this.body = body;
    this.headers = new Headers(headers);
  }
}

export class ChaffnetProtocolError extends ChaffnetError {}

export class ChaffnetTimeoutError extends ChaffnetError {
  readonly timeoutMs: number;

  constructor(timeoutMs: number, options?: ErrorOptions) {
    super(`chaffnet request timed out after ${timeoutMs}ms`, options);
    this.timeoutMs = timeoutMs;
  }
}
