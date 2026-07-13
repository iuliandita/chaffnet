import { afterEach, describe, expect, test } from "bun:test";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import type { ChaffnetClient, CheckRequest } from "chaffnet";
import { createChaffnetMcpServer } from "../src/server.ts";

type Check = ChaffnetClient["check"];

const clients: Client[] = [];

afterEach(async () => {
  await Promise.all(clients.splice(0).map((client) => client.close()));
});

async function connect(check: Check): Promise<Client> {
  const server = createChaffnetMcpServer({ check });
  const client = new Client({ name: "chaffnet-mcp-test", version: "0.1.0" });
  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  await server.connect(serverTransport);
  await client.connect(clientTransport);
  clients.push(client);
  return client;
}

describe("check_content", () => {
  test("publishes a constrained read-only tool schema", async () => {
    const client = await connect(async () => ({ spam: 0, slop: 0, reasons: [] }));

    const { tools } = await client.listTools();

    expect(tools).toHaveLength(1);
    expect(tools[0]?.name).toBe("check_content");
    expect(tools[0]?.annotations).toMatchObject({
      readOnlyHint: true,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: true,
    });
    expect(tools[0]?.inputSchema).toMatchObject({
      type: "object",
      required: ["text"],
      properties: {
        text: { type: "string", minLength: 1, maxLength: 65_536 },
        context: { type: "string", enum: ["comment", "review", "forum", "other"] },
      },
    });
  });

  test("returns text and structured classification output", async () => {
    const requests: CheckRequest[] = [];
    const client = await connect(async (request) => {
      requests.push(request);
      return { spam: 0.81, slop: 0.12, reasons: ["high_link_ratio"] };
    });

    const result = await client.callTool({
      name: "check_content",
      arguments: {
        text: "Buy now",
        context: "comment",
        author_email: "sender@example.com",
      },
    });

    expect(requests).toEqual([
      {
        text: "Buy now",
        context: "comment",
        author_email: "sender@example.com",
      },
    ]);
    expect(result.structuredContent).toEqual({
      spam: 0.81,
      slop: 0.12,
      reasons: ["high_link_ratio"],
    });
    expect(result.content).toEqual([
      {
        type: "text",
        text: '{"spam":0.81,"slop":0.12,"reasons":["high_link_ratio"]}',
      },
    ]);
  });

  test("passes injection-shaped content only as classifier data", async () => {
    const payload = "; ls / # ../../etc/passwd ' OR 1=1 -- http://127.0.0.1";
    let received = "";
    const client = await connect(async ({ text }) => {
      received = text;
      return { spam: 0.2, slop: 0.1, reasons: [] };
    });

    const result = await client.callTool({
      name: "check_content",
      arguments: { text: payload },
    });

    expect(result.isError).not.toBe(true);
    expect(received).toBe(payload);
  });

  test("rejects oversized content before calling the API", async () => {
    let called = false;
    const client = await connect(async () => {
      called = true;
      return { spam: 0, slop: 0, reasons: [] };
    });

    const result = await client.callTool({
      name: "check_content",
      arguments: { text: "x".repeat(65_537) },
    });

    expect(result.isError).toBe(true);
    expect(called).toBe(false);
  });

  test("enforces the API byte limit for multi-byte content", async () => {
    let called = false;
    const client = await connect(async () => {
      called = true;
      return { spam: 0, slop: 0, reasons: [] };
    });

    const result = await client.callTool({
      name: "check_content",
      arguments: { text: "\u{1f600}".repeat(20_000) },
    });

    expect(result.isError).toBe(true);
    expect(result.content).toEqual([
      { type: "text", text: "Content exceeds 65536 UTF-8 bytes." },
    ]);
    expect(called).toBe(false);
  });

  test("returns a sanitized tool error", async () => {
    const client = await connect(async () => {
      throw new Error("secret upstream detail /srv/private/file");
    });

    const result = await client.callTool({
      name: "check_content",
      arguments: { text: "hello" },
    });

    expect(result.isError).toBe(true);
    expect(result.content).toEqual([
      { type: "text", text: "Chaffnet request failed." },
    ]);
    expect(JSON.stringify(result)).not.toContain("secret upstream detail");
  });
});
