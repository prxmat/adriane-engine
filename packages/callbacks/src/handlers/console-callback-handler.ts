import type { CallbackHandler } from "../interfaces.js";
import type { CallbackEvent } from "../types.js";

const log = (event: CallbackEvent): void => {
  console.log(`[callback] ${event.type}`, event);
};

export class ConsoleCallbackHandler implements CallbackHandler {
  public onLLMStart = log;
  public onLLMToken = log;
  public onLLMEnd = log;
  public onLLMError = log;
  public onToolStart = log;
  public onToolEnd = log;
  public onToolError = log;
  public onNodeStart = log;
  public onNodeEnd = log;
  public onNodeError = log;
  public onChainStart = log;
  public onChainEnd = log;
  public onChainError = log;
  public onAgentAction = log;
  public onAgentFinish = log;
}
