/**
 * Resource-search seam types (ADR 0011). A `SearchDocument` is what a producer (graphs, KB,
 * agent-types) pushes into the index; a `SearchHit` is what a query returns. These are the
 * wire-neutral shapes the engine owns — the control plane maps them to/from contracts DTOs.
 */

/** The kinds of resource the unified search indexes. */
export type SearchResourceType = "graph" | "agent" | "kb";

/** One indexable resource. `tenantId === null` marks a global resource (catalog graph, agent
 * type) visible to every tenant. `namespace` is carried for KB docs (used in the href / display). */
export interface SearchDocument {
  type: SearchResourceType;
  /** Unique within its `type` (e.g. a graph id, a `namespace:path` for KB). */
  id: string;
  tenantId: string | null;
  namespace?: string;
  /** Short, high-weight label shown as the result title. */
  title: string;
  /** Full searchable body (description, content, labels…). */
  text: string;
  /** Studio route to open the resource. */
  href: string;
  /** ISO-8601 last-updated, for tie-breaking / display. */
  updatedAt?: string;
}

/** One ranked result. */
export interface SearchHit {
  type: SearchResourceType;
  id: string;
  tenantId: string | null;
  namespace?: string;
  title: string;
  /** A short excerpt around the match (or the title when the body did not match). */
  snippet: string;
  href: string;
  /** Higher is more relevant. Backend-specific scale; only the ordering is meaningful. */
  score: number;
}

/** Query scoping. `tenantId` is mandatory — a search is always tenant-scoped (global docs are
 * additionally included). `types` restricts to a subset; omitted means all. */
export interface SearchQueryOptions {
  tenantId: string;
  types?: SearchResourceType[];
  limit?: number;
}

/**
 * Pluggable search backend. The engine ships {@link InMemorySearchProvider}; the control plane
 * provides an Elasticsearch implementation behind the same interface (ADR 0011).
 */
export interface SearchProvider {
  /** Idempotent one-time setup (create the ES index/mapping). No-op for in-memory. */
  ensureReady(): Promise<void>;
  /** Upsert documents (keyed by `type` + `id`). */
  index(documents: SearchDocument[]): Promise<void>;
  /** Remove one document by `type` + `id`. Missing ids are ignored. */
  remove(type: SearchResourceType, id: string): Promise<void>;
  /** Tenant-scoped ranked search. Returns at most `opts.limit` hits (default 10). */
  search(query: string, opts: SearchQueryOptions): Promise<SearchHit[]>;
}

/** Default result cap when a query omits `limit`. */
export const DEFAULT_SEARCH_LIMIT = 10;
