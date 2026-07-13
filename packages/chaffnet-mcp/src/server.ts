import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import {
  ChaffnetApiError,
  ChaffnetProtocolError,
  ChaffnetTimeoutError,
  type ChaffnetClient,
  type CheckRequest,
} from "chaffnet";
import { z } from "zod";

const MAX_CONTENT_BYTES = 65_536;

type Classifier = Pick<ChaffnetClient, "check">;

const inputSchema = {
  text: z
    .string()
    .min(1)
    .max(MAX_CONTENT_BYTES)
    .describe("Content to classify, up to 65536 UTF-8 bytes"),
  context: z
    .enum(["comment", "review", "forum", "other"])
    .optional()
    .describe("Where the content will be published"),
  author_ip: z.string().max(45).optional().describe("Author IPv4 or IPv6 address"),
  author_email: z.string().max(320).optional().describe("Author email address"),
  author_name: z.string().max(256).optional().describe("Author display name"),
};

const outputSchema = {
  spam: z.number().min(0).max(1),
  slop: z.number().min(0).max(1),
  reasons: z.array(z.string().max(128)).max(64),
};

function toolError(message: string) {
  return {
    content: [{ type: "text" as const, text: message }],
    isError: true as const,
  };
}

function publicError(error: unknown): string {
  if (error instanceof ChaffnetTimeoutError) {
    return "Chaffnet request timed out.";
  }
  if (error instanceof ChaffnetProtocolError) {
    return "Chaffnet returned an invalid response.";
  }
  if (error instanceof ChaffnetApiError) {
    if (error.status === 401 || error.status === 403) {
      return "Chaffnet rejected the configured API key.";
    }
    if (error.status === 429) {
      return "Chaffnet rate limit exceeded. Retry later.";
    }
    if (error.status >= 500) {
      return "Chaffnet service is unavailable.";
    }
    return "Chaffnet rejected the classification request.";
  }
  return "Chaffnet request failed.";
}

export function createChaffnetMcpServer(classifier: Classifier): McpServer {
  const server = new McpServer({ name: "chaffnet", version: "0.1.0" });

  server.registerTool(
    "check_content",
    {
      title: "Check content",
      description: "Score content for spam and low-quality AI-generated writing.",
      inputSchema,
      outputSchema,
      annotations: {
        readOnlyHint: true,
        destructiveHint: false,
        idempotentHint: true,
        openWorldHint: true,
      },
    },
    async (input) => {
      if (new TextEncoder().encode(input.text).byteLength > MAX_CONTENT_BYTES) {
        return toolError("Content exceeds 65536 UTF-8 bytes.");
      }

      const request: CheckRequest = {
        text: input.text,
        ...(input.context === undefined ? {} : { context: input.context }),
        ...(input.author_ip === undefined ? {} : { author_ip: input.author_ip }),
        ...(input.author_email === undefined
          ? {}
          : { author_email: input.author_email }),
        ...(input.author_name === undefined ? {} : { author_name: input.author_name }),
      };

      try {
        const result = await classifier.check(request);
        const output = {
          spam: result.spam,
          slop: result.slop,
          reasons: [...result.reasons],
        };
        return {
          content: [{ type: "text", text: JSON.stringify(output) }],
          structuredContent: output,
        };
      } catch (error) {
        return toolError(publicError(error));
      }
    },
  );

  return server;
}
