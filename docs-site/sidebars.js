// @ts-check

// A MANUAL sidebar (LangChain-style information architecture): top-level sections
// Get started · Build · Tutorials · Govern · Monitor · Deploy · Reference, with Build
// split into sub-sections. Files stay where they are on disk — this file controls the
// navigation grouping only, so doc URLs and cross-links are unchanged. (Test and No-code
// agents are intentionally deferred until they have real content.)
/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  docs: [
    {
      type: "category",
      label: "Get started",
      collapsed: false,
      items: [
        "introduction/why-adriane",
        "introduction/comparison",
        "getting-started/quickstart",
        "getting-started/installation",
        "getting-started/your-first-run"
      ]
    },
    {
      type: "category",
      label: "Build",
      collapsed: false,
      items: [
        {
          type: "category",
          label: "Concepts",
          items: [
            "core-concepts/graphs-nodes-edges-state",
            "core-concepts/channels-and-reducers",
            "core-concepts/execution-contract",
            "core-concepts/resumability-and-approvals",
            "core-concepts/runtime-and-engine",
            "architecture/overview",
            "architecture/napi-bridge"
          ]
        },
        {
          type: "category",
          label: "Graphs",
          items: [
            "building/action-nodes-and-routing",
            "building/streaming-and-events",
            "building/subgraphs",
            "building/dynamic-message-send",
            "building/durable-timers-and-signals"
          ]
        },
        {
          type: "category",
          label: "Agents",
          items: [
            "building/agent-nodes-and-react",
            "building/tools-and-tool-nodes",
            "building/multi-agent-orchestration"
          ]
        },
        {
          type: "category",
          label: "Deep agents",
          items: [
            "advanced-agents/middleware-and-profiles",
            "advanced-agents/governed-filesystem",
            "advanced-agents/deep-agents"
          ]
        },
        {
          type: "category",
          label: "Integrations",
          items: [
            "building/llm-gateway",
            "building/providers",
            {
              type: "category",
              label: "Models",
              items: [
                "integrations/models/overview",
                "integrations/models/anthropic",
                "integrations/models/google",
                "integrations/models/openai",
                "integrations/models/azure",
                "integrations/models/mistral",
                "integrations/models/openrouter",
                "integrations/models/groq",
                "integrations/models/huggingface",
                "integrations/models/ollama",
                "integrations/models/nvidia",
                "integrations/models/aws-bedrock"
              ]
            },
            { type: "category", label: "Middleware", items: ["integrations/middleware/overview"] },
            { type: "category", label: "Backends", items: ["integrations/backends/overview"] },
            { type: "category", label: "Checkpointers", items: ["integrations/checkpointers/overview"] },
            { type: "category", label: "Retrievers", items: ["integrations/retrievers/overview"] },
            { type: "category", label: "Text splitters", items: ["integrations/text-splitters/overview"] },
            { type: "category", label: "Vector stores", items: ["integrations/vector-stores/overview"] },
            { type: "category", label: "Sandboxes", items: ["integrations/sandboxes/overview"] }
          ]
        },
        {
          type: "category",
          label: "Adriane Lang (DSL)",
          items: [
            "dsl/graph-yaml-syntax",
            "dsl/prompt-agent-chain-syntax",
            "dsl/compiler-pipeline"
          ]
        },
        {
          type: "category",
          label: "Components",
          items: ["building/components-reference"]
        },
        {
          type: "category",
          label: "Knowledge",
          items: ["knowledge/knowledge-base-and-graph", "knowledge/open-knowledge-format"]
        },
        {
          type: "category",
          label: "MCP",
          items: ["building/mcp-server"]
        },
        {
          type: "category",
          label: "SDK parity",
          items: [
            "sdk-parity/one-engine-two-languages",
            "sdk-parity/typescript-sdk",
            "sdk-parity/python-sdk"
          ]
        }
      ]
    },
    {
      type: "category",
      label: "Cookbook",
      items: [
        "recipes/governed-refund-agent",
        "recipes/idea-to-ship-pipeline",
        "recipes/rag-question-answerer",
        "recipes/resume-across-processes",
        "recipes/stream-to-dashboard",
        "recipes/yaml-and-builder"
      ]
    },
    {
      type: "category",
      label: "Govern",
      items: [
        "governance/governance-model",
        "governance/approval-gates",
        "governance/tool-approval-and-attestation",
        "governance/pii-redaction"
      ]
    },
    {
      type: "category",
      label: "Monitor",
      items: ["governance/observable-runs"]
    },
    {
      type: "category",
      label: "Deploy",
      items: [
        "production/deployment",
        "production/best-practices",
        "production/troubleshooting",
        "cli/commands"
      ]
    },
    {
      type: "category",
      label: "API Reference",
      items: [
        "reference/builder-api",
        "reference/component-catalog",
        "reference/events-and-streams",
        "reference/errors",
        "glossary",
        "roadmap"
      ]
    }
  ]
};

module.exports = sidebars;
