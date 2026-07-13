import { expect, test } from "bun:test";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { fileURLToPath } from "node:url";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import {
  getDefaultEnvironment,
  StdioClientTransport,
} from "@modelcontextprotocol/sdk/client/stdio.js";

test("serves check_content over stdio with the configured API", async () => {
  let authorization: string | undefined;
  let requestBody: unknown;
  const api = createServer((request, response) => {
    authorization = request.headers.authorization;
    const chunks: Uint8Array[] = [];
    request.on("data", (chunk: Uint8Array) => chunks.push(chunk));
    request.on("end", () => {
      requestBody = JSON.parse(Buffer.concat(chunks).toString("utf8")) as unknown;
      response.writeHead(200, { "content-type": "application/json" });
      response.end('{"spam":0.7,"slop":0.2,"reasons":["high_link_ratio"]}');
    });
  });
  await new Promise<void>((resolve, reject) => {
    api.once("error", reject);
    api.listen(0, "127.0.0.1", resolve);
  });

  const address = api.address() as AddressInfo;
  const transport = new StdioClientTransport({
    command: "node",
    args: [fileURLToPath(new URL("../dist/main.js", import.meta.url))],
    env: {
      ...getDefaultEnvironment(),
      CHAFFNET_BASE_URL: `http://127.0.0.1:${address.port}`,
      CHAFFNET_API_KEY: "stdio-test-key",
    },
    stderr: "pipe",
  });
  const client = new Client({ name: "stdio-test", version: "0.1.0" });

  try {
    await client.connect(transport);
    const result = await client.callTool({
      name: "check_content",
      arguments: { text: "review me", context: "review" },
    });

    expect(result.isError).not.toBe(true);
    expect(result.structuredContent).toEqual({
      spam: 0.7,
      slop: 0.2,
      reasons: ["high_link_ratio"],
    });
    expect(authorization).toBe("Bearer stdio-test-key");
    expect(requestBody).toEqual({ text: "review me", context: "review" });
  } finally {
    await client.close();
    await new Promise<void>((resolve, reject) => {
      api.close((error) => (error ? reject(error) : resolve()));
    });
  }
});
