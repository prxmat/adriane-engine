/**
 * Versioned prompt registry. Agents reference prompts by id (+ optional version)
 * instead of hardcoding them inline (rule 090). The `system` text is the immutable,
 * cacheable prefix — keep volatile/dynamic values out of it and pass those in the
 * per-call user message instead.
 */
export type PromptTemplate = {
  id: string;
  version: string;
  system: string;
  description?: string;
};

export interface PromptRegistry {
  register(template: PromptTemplate): void;
  get(id: string, version?: string): PromptTemplate;
  list(): PromptTemplate[];
}

export class PromptNotFoundError extends Error {
  public readonly promptId: string;
  public readonly version?: string;

  public constructor(promptId: string, version?: string) {
    super(
      version === undefined
        ? `No prompt registered for id '${promptId}'.`
        : `No prompt registered for id '${promptId}' at version '${version}'.`
    );
    this.name = "PromptNotFoundError";
    this.promptId = promptId;
    this.version = version;
  }
}

export class InMemoryPromptRegistry implements PromptRegistry {
  private readonly byId = new Map<string, Map<string, PromptTemplate>>();
  private readonly latest = new Map<string, string>();

  public register(template: PromptTemplate): void {
    const versions = this.byId.get(template.id) ?? new Map<string, PromptTemplate>();
    versions.set(template.version, template);
    this.byId.set(template.id, versions);
    // Most recently registered version is the resolved default for `get` with no version.
    this.latest.set(template.id, template.version);
  }

  public get(id: string, version?: string): PromptTemplate {
    const versions = this.byId.get(id);
    if (versions === undefined) {
      throw new PromptNotFoundError(id);
    }
    const resolvedVersion = version ?? this.latest.get(id);
    if (resolvedVersion === undefined) {
      throw new PromptNotFoundError(id);
    }
    const template = versions.get(resolvedVersion);
    if (template === undefined) {
      throw new PromptNotFoundError(id, version);
    }
    return template;
  }

  public list(): PromptTemplate[] {
    return [...this.byId.values()].flatMap((versions) => [...versions.values()]);
  }
}
