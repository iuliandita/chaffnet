import type { ChaffnetClientOptions } from "chaffnet";

type Environment = Readonly<Record<string, string | undefined>>;

export function loadConfig(env: Environment = process.env): ChaffnetClientOptions {
  const baseUrl = env.CHAFFNET_BASE_URL?.trim();
  if (!baseUrl) {
    throw new Error("CHAFFNET_BASE_URL is required");
  }

  const timeoutValue = env.CHAFFNET_TIMEOUT_MS?.trim();
  let timeoutMs: number | undefined;
  if (timeoutValue) {
    timeoutMs = Number(timeoutValue);
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
      throw new Error("CHAFFNET_TIMEOUT_MS must be a positive integer");
    }
  }

  const apiKey = env.CHAFFNET_API_KEY?.trim();
  return {
    baseUrl,
    ...(apiKey ? { apiKey } : {}),
    ...(timeoutMs === undefined ? {} : { timeoutMs }),
  };
}
