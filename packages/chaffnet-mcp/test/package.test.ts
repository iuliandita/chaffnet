import { expect, test } from "bun:test";

interface PackageManifest {
  version: string;
  dependencies?: Record<string, string>;
}

test("pins chaffnet dependency to the SDK release version", async () => {
  const sdk = (await Bun.file(
    new URL("../../chaffnet/package.json", import.meta.url),
  ).json()) as PackageManifest;
  const mcp = (await Bun.file(
    new URL("../package.json", import.meta.url),
  ).json()) as PackageManifest;

  expect(mcp.dependencies?.chaffnet).toBe(sdk.version);
});
