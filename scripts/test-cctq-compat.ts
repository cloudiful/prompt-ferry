const baseUrl = (process.env.CCTQ_BASE_URL ?? process.env.OPENAI_BASE_URL ?? "").replace(/\/+$/, "");
const apiKey = process.env.CCTQ_API_KEY ?? process.env.OPENAI_API_KEY ?? "";
const model = process.env.CCTQ_MODEL ?? "gpt-5.4";
const timeoutMs = Number(process.env.CCTQ_TIMEOUT_MS ?? "120000");

if (!baseUrl) {
  console.error("Missing CCTQ_BASE_URL or OPENAI_BASE_URL");
  process.exit(1);
}

if (!apiKey) {
  console.error("Missing CCTQ_API_KEY or OPENAI_API_KEY");
  process.exit(1);
}

type TestResult = {
  name: string;
  ok: boolean;
  status: number;
  elapsedMs: number;
  contentType: string;
  text: string;
  bodyText: string;
};

type TestCase = {
  name: string;
  run: () => Promise<TestResult>;
};

const marker = `CCTQ_OK_${crypto.randomUUID().slice(0, 8)}`;

function normalizeBaseUrl(input: string): string {
  return input.endsWith("/v1") ? input : `${input}/v1`;
}

function extractText(payload: any): string {
  if (typeof payload?.output_text === "string" && payload.output_text.length > 0) {
    return payload.output_text;
  }

  if (Array.isArray(payload?.choices)) {
    return payload.choices
      .map((choice: any) => choice?.message?.content ?? "")
      .filter((value: string) => typeof value === "string")
      .join("");
  }

  if (Array.isArray(payload?.output)) {
    return payload.output
      .flatMap((item: any) => {
        if (Array.isArray(item?.content)) {
          return item.content;
        }
        return [item];
      })
      .filter((part: any) => part?.type === "output_text")
      .map((part: any) => part?.text ?? "")
      .join("");
  }

  return "";
}

function extractCallId(payload: any): string | null {
  if (!Array.isArray(payload?.output)) {
    return null;
  }

  for (const item of payload.output) {
    if (item?.type === "function_call" && typeof item.call_id === "string") {
      return item.call_id;
    }

    if (Array.isArray(item?.content)) {
      for (const part of item.content) {
        if (part?.type === "function_call" && typeof part.call_id === "string") {
          return part.call_id;
        }
      }
    }
  }

  return null;
}

async function postJson(name: string, path: string, body: Record<string, unknown>): Promise<TestResult> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const url = `${normalizeBaseUrl(baseUrl)}${path}`;
  const startedAt = Date.now();

  try {
    const response = await fetch(url, {
      method: "POST",
      headers: {
        authorization: `Bearer ${apiKey}`,
        "content-type": "application/json",
      },
      body: JSON.stringify(body),
      signal: controller.signal,
    });

    const elapsedMs = Date.now() - startedAt;
    const contentType = response.headers.get("content-type") ?? "";
    const bodyText = await response.text();
    let parsed: unknown = null;

    try {
      parsed = JSON.parse(bodyText);
    } catch {
      parsed = null;
    }

    const text = contentType.includes("application/json") && parsed ? extractText(parsed) : "";

    return {
      name,
      ok: response.ok,
      status: response.status,
      elapsedMs,
      contentType,
      text,
      bodyText,
    };
  } catch (error) {
    const elapsedMs = Date.now() - startedAt;
    return {
      name,
      ok: false,
      status: 0,
      elapsedMs,
      contentType: "",
      text: "",
      bodyText: error instanceof Error ? error.message : String(error),
    };
  } finally {
    clearTimeout(timer);
  }
}

function toolDefinition() {
  return {
    type: "function",
    name: "get_current_time",
    description: "Return the current time string",
    parameters: {
      type: "object",
      additionalProperties: false,
      properties: {},
    },
  };
}

const tests: TestCase[] = [
  {
    name: "chat-basic",
    run: () =>
      postJson("chat-basic", "/chat/completions", {
        model,
        messages: [
          {
            role: "user",
            content: `Return exactly this ASCII text, no quotes, no extra text: ${marker}_CHAT`,
          },
        ],
        temperature: 0,
        stream: false,
      }),
  },
  {
    name: "responses-basic",
    run: () =>
      postJson("responses-basic", "/responses", {
        model,
        stream: false,
        text: { verbosity: "low" },
        input: [
          {
            role: "user",
            type: "message",
            content: [
              {
                type: "input_text",
                text: `Return exactly this ASCII text, no quotes, no extra text: ${marker}_RESPONSES`,
              },
            ],
          },
        ],
      }),
  },
  {
    name: "responses-tools",
    run: () =>
      postJson("responses-tools", "/responses", {
        model,
        stream: false,
        text: { verbosity: "low" },
        tools: [toolDefinition()],
        tool_choice: {
          type: "function",
          name: "get_current_time",
        },
        input: [
          {
            role: "user",
            type: "message",
            content: [
              {
                type: "input_text",
                text: `Call get_current_time once, then return exactly this ASCII text: ${marker}_TOOLS`,
              },
            ],
          },
        ],
      }),
  },
  {
    name: "responses-tools-inline-output",
    run: () =>
      postJson("responses-tools-inline-output", "/responses", {
        model,
        stream: false,
        text: { verbosity: "low" },
        input: [
          {
            role: "user",
            type: "message",
            content: [
              {
                type: "input_text",
                text: `After using the tool result, return exactly this ASCII text: ${marker}_INLINE`,
              },
            ],
          },
          {
            type: "function_call",
            call_id: "call_inline_1",
            name: "get_current_time",
            arguments: "{}",
          },
          {
            type: "function_call_output",
            call_id: "call_inline_1",
            output: "2026-06-13T16:00:00+08:00",
          },
        ],
      }),
  },
  {
    name: "responses-tools-previous-response",
    run: async () => {
      const turn1 = await postJson("responses-tools-previous-response-turn1", "/responses", {
        model,
        stream: false,
        text: { verbosity: "low" },
        tools: [toolDefinition()],
        tool_choice: {
          type: "function",
          name: "get_current_time",
        },
        input: [
          {
            role: "user",
            type: "message",
            content: [
              {
                type: "input_text",
                text: `Call get_current_time once and wait for my tool output. Marker: ${marker}_PREV`,
              },
            ],
          },
        ],
      });

      if (!turn1.ok) {
        return {
          ...turn1,
          name: "responses-tools-previous-response",
        };
      }

      let parsed: any = null;
      try {
        parsed = JSON.parse(turn1.bodyText);
      } catch {
        return {
          ...turn1,
          name: "responses-tools-previous-response",
          ok: false,
          bodyText: `turn1 invalid JSON\n${turn1.bodyText}`,
        };
      }

      const responseId = typeof parsed?.id === "string" ? parsed.id : null;
      const callId = extractCallId(parsed);

      if (!responseId || !callId) {
        return {
          ...turn1,
          name: "responses-tools-previous-response",
          ok: false,
          bodyText: `turn1 missing response id or call id\n${turn1.bodyText}`,
        };
      }

      return postJson("responses-tools-previous-response", "/responses", {
        model,
        stream: false,
        text: { verbosity: "low" },
        previous_response_id: responseId,
        input: [
          {
            type: "function_call_output",
            call_id: callId,
            output: "2026-06-13T16:00:00+08:00",
          },
        ],
      });
    },
  },
  {
    name: "responses-stream",
    run: () =>
      postJson("responses-stream", "/responses", {
        model,
        stream: true,
        text: { verbosity: "low" },
        input: [
          {
            role: "user",
            type: "message",
            content: [
              {
                type: "input_text",
                text: `Return exactly this ASCII text, no quotes, no extra text: ${marker}_STREAM`,
              },
            ],
          },
        ],
      }),
  },
];

for (const test of tests) {
  console.log(`\n=== ${test.name} ===`);
  const result = await test.run();
  console.log(`status: ${result.status}`);
  console.log(`ok: ${result.ok}`);
  console.log(`elapsed_ms: ${result.elapsedMs}`);
  if (result.contentType) {
    console.log(`content_type: ${result.contentType}`);
  }
  if (result.text) {
    console.log(`text: ${result.text}`);
  }
  console.log("body:");
  console.log(result.bodyText);
}
