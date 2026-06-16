import type { Agent, AgentRegistry } from "./interfaces.js";
import type { AgentId } from "./types.js";

export class InMemoryAgentRegistry implements AgentRegistry {
  private readonly agents = new Map<AgentId, Agent>();

  public register(agent: Agent): void {
    this.agents.set(agent.id, agent);
  }

  public resolve(id: AgentId): Agent | undefined {
    return this.agents.get(id);
  }

  public list(): Agent[] {
    return [...this.agents.values()];
  }
}
