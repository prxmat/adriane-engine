/**
 * `@adriane/graph-sdk` — the front door to the Adriane framework.
 *
 * Build, compile and run stateful, resumable agent graphs without touching the
 * lower-level engine primitives. Everything you need for the common case is
 * re-exported here.
 *
 * ```ts
 * import { createGraph } from "@adriane/graph-sdk";
 *
 * const app = createGraph({ name: "greeter" })
 *   .node("hello", async (_input, state) => ({ greeting: `Hello, ${state.channels.name}!` }))
 *   .compile();
 *
 * const result = await app.run({ name: "Ada" });
 * console.log(result.channels.greeting); // "Hello, Ada!"
 * ```
 */

export { createGraph, GraphBuilder } from "./builder.js";
export type { CreateGraphOptions, ChannelInput, NodeInput } from "./builder.js";

export { CompiledGraph } from "./compiled-graph.js";
export type { CompiledGraphParts, RunOptions, ApproveAndResumeOptions } from "./compiled-graph.js";

export { exampleGraphs } from "./example-graphs.js";
export type { ExampleGraph } from "./example-graphs.js";

// The complete Doc-QA reference pipeline (input → output), runnable as a CompiledGraph
// or persistable as a carrier-bearing GraphDefinition for the catalog run path.
export {
  buildDocQaReference,
  docQaReferenceDefinition,
  DEFAULT_REFERENCE_CORPUS
} from "./reference-graph.js";
export type { DocQaReferenceOptions } from "./reference-graph.js";

// Pure compute components (run natively on Rust, faithful TS fallback) and the
// prebuilt, tier-tagged micro-agent graphs.
export { components } from "./components.js";
export type {
  ComponentDescriptor,
  IntegrationComponentHandler,
  ComponentKind,
  RustComponentConfig,
  PromptBuilderParams,
  JsonValidatorParams,
  OutputParserParams,
  RouterParams,
  RouterRule,
  RetrieverParams,
  RetrieverDoc,
  RerankerParams,
  TextCleanerParams,
  DocumentSplitterParams,
  HtmlToTextParams,
  CsvParserParams,
  DocumentJoinerParams,
  DeduplicatorParams,
  TruncatorParams,
  RegexExtractorParams,
  AnswerBuilderParams,
  FieldMapperParams,
  LexicalDoc,
  Bm25RetrieverParams,
  KeywordRetrieverParams,
  SentenceWindowSplitterParams,
  LanguageDetectorParams,
  PredicateOp,
  MetadataFilterParams,
  ListJoinerParams,
  MergeRankerParams,
  EvaluatorParams,
  ChatMessageSpec,
  ChatMessageBuilderParams,
  ConditionalRouterBranch,
  ConditionalRouterParams,
  DocumentWriterParams,
  HttpFetchParams,
  HttpFetchResult,
  HttpFetchImpl,
  HttpFetchRequestInit,
  HttpFetchResponseLike,
  WebSearchParams,
  WebSearchResult,
  WebSearchOutcome,
  WebSearchImpl,
  WebSearchTransport
} from "./components.js";

export { prebuilt } from "./prebuilt-agents.js";
export type { PrebuiltOptions, RagAnswererOptions } from "./prebuilt-agents.js";

// Real embeddings + a vector store + a semantic (vector-store) retrieval connector.
// Exported SDK helpers — NOT catalog component kinds — all injectable for offline tests.
export { createEmbeddings, MissingEmbeddingsKeyError, EmbeddingsResponseError } from "./embeddings.js";
export type {
  Embeddings,
  EmbeddingsTransport,
  EmbeddingsRequestBody,
  CreateEmbeddingsOptions
} from "./embeddings.js";

export { createVectorStore, cosineSimilarity } from "./vector-store.js";
export type {
  VectorStore,
  VectorStoreItem,
  VectorStoreMatch,
  CreateVectorStoreOptions
} from "./vector-store.js";

export { semanticRetriever } from "./semantic-retriever.js";
export type { SemanticRetrieverParams, SemanticRetrieverDoc } from "./semantic-retriever.js";

// Static catalog metadata backing the API's catalog endpoint (components / prebuilt
// agents / capability tiers).
export { componentCatalog, prebuiltCatalog, tierCatalog } from "./catalog.js";
export type {
  ComponentCatalogEntry,
  ComponentCategory,
  ComponentParamMeta,
  PrebuiltAgentCatalogEntry,
  ModelTierInfo
} from "./catalog.js";

export { rustValidatorActive } from "./rust-validator.js";
export { rustEngineAvailable } from "./rust-engine.js";
// The approve/resume provenance wire shape `{ name, requestedBy, resolvedBy }` the Rust
// guard-rail validates — the control plane builds it from ApprovalEngine decisions.
export type { ApprovedToolWire } from "./rust-engine.js";

// Execute a *catalog* graph (a plain GraphDefinition whose nodes carry the shared
// `node.metadata.component` / `node.metadata.agent` carrier) on the Rust engine. This
// is the seam the control plane uses to run graphs authored in the Studio editor.
export {
  runCatalogGraph,
  resumeCatalogGraph,
  isCatalogGraph,
  readComponentCarrier,
  readAgentCarrier,
  RustEngineUnavailableError
} from "./run-catalog-graph.js";
export type {
  CatalogRunOutcome,
  RunCatalogGraphOptions,
  ComponentCarrier,
  AgentCarrier
} from "./run-catalog-graph.js";

export type {
  ChannelValues,
  EmptyChannels,
  TypedGraphState,
  TypedNodeHandler,
  TypedCondition,
  ChannelUpdate,
  InitialData
} from "./typed.js";

export {
  createAgentNodeHandler,
  createToolNodeHandler,
  streamAgentTokens,
  toRustAgentConfig,
  toAgentApprovalBinding,
  DEFAULT_AGENT_OUTPUT_CHANNEL,
  APPROVED_TOOLS_CHANNEL,
  APPROVAL_IDS_CHANNEL,
  AGENT_APPROVAL_INTERRUPT
} from "./agent-node.js";
export type {
  AgentApprovalBinding,
  AgentNodeConfig,
  AgentPromptSource,
  RustAgentConfig,
  RustToolBinding,
  StreamAgentConfig,
  ToolNodeConfig
} from "./agent-node.js";

export { AdrianeSdkError, GraphCompileError, DuplicateNodeError, MissingHandlerError } from "./errors.js";
export type { Result } from "./errors.js";

// Re-export the engine types most callers reach for, so a single import suffices.
export type {
  Command,
  GraphDefinition,
  GraphState,
  GraphStatus,
  NodeId,
  RunId,
  ChannelReducer
} from "@adriane/graph-core";
export type { Message, AIMessage, ToolCall, MessageId } from "@adriane/graph-core";
export type { ConditionFn, NodeHandler, RunEvent } from "@adriane/graph-runtime";
export type { StreamEvent, StreamMode } from "@adriane/graph-runtime";

// Advanced wiring for callers who want durable checkpoints / custom buses. The
// Postgres-backed adapters live in the PRIVATE `@adriane/db-adapters` package and are
// intentionally NOT re-exported here, so the public SDK bundle never embeds the DB
// schema. Bring your own `Checkpointer` (the interface is exported above) or import
// the Pg adapters from `@adriane/db-adapters` in private/control-plane code.
export { InMemoryCheckpointer, DynamicInterrupt } from "@adriane/graph-runtime";

// Building blocks for agent/tool nodes, re-exported so a single import suffices.
export {
  DefaultLLMGateway,
  MockLLMProviderAdapter,
  AnthropicProviderAdapter,
  OpenAICompatibleProviderAdapter,
  InMemoryPromptRegistry,
  ModelPolicy,
  MODEL_TIERS,
  DEFAULT_TIER_TABLE,
  DEFAULT_PREFERENCE
} from "@adriane/llm-gateway";
export type {
  LLMGateway,
  LLMProvider,
  LLMResponse,
  LLMStreamChunk,
  LLMToolCall,
  PromptRegistry,
  OpenAICompatibleAdapterOptions,
  OpenAICompatibleTransportPort,
  OpenAIChatRequestBody,
  OpenAIChatResponse,
  ModelTier,
  ModelChoice,
  TierModelTable,
  ResolveOverride
} from "@adriane/llm-gateway";
export { InMemoryToolRegistry } from "@adriane/agents-core";
export type { ToolRegistry, ToolDefinition, ToolId, AgentResult } from "@adriane/agents-core";
