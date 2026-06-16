import type { Command, NodeId } from "@adriane/graph-core";
import type { LLMGateway } from "../../llm-gateway/src/interfaces.js";
import type { NodeHandler } from "../../graph-runtime/src/interfaces.js";

type ReflectionNodeOptions = {
  llm: LLMGateway;
  previousNodeId: NodeId;
  maxReflections?: number;
};

const REFLECTION_COUNT_KEY = "__reflectionCount";

export const createReflectionNode = (options: ReflectionNodeOptions): NodeHandler => {
  const maxReflections = options.maxReflections ?? 2;
  return async (input) => {
    const channels = input as Record<string, unknown>;
    const count = typeof channels[REFLECTION_COUNT_KEY] === "number" ? (channels[REFLECTION_COUNT_KEY] as number) : 0;
    const completion = await options.llm.complete({
      provider: "openai",
      model: "reflection-node",
      messages: [
        {
          role: "user",
          content: `Critique output: ${JSON.stringify(input)}`
        }
      ]
    });
    const critique = completion.content.toLowerCase();
    if (count < maxReflections && (critique.includes("problem") || critique.includes("retry"))) {
      const cmd: Command = {
        goto: options.previousNodeId,
        update: { [REFLECTION_COUNT_KEY]: count + 1 } as never
      };
      return cmd;
    }
    return {
      ...channels,
      confidence: Math.min(1, (typeof channels.confidence === "number" ? (channels.confidence as number) : 0.5) + 0.1),
      [REFLECTION_COUNT_KEY]: count
    };
  };
};
