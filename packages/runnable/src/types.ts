export type CallbackHandler = {
  onStart?: (input: unknown) => void | Promise<void>;
  onEnd?: (output: unknown) => void | Promise<void>;
  onError?: (error: unknown) => void | Promise<void>;
};

export type RetryPolicy = {
  maxAttempts: number;
  backoffMs?: number;
};

export type RunnableConfig = {
  tags?: string[];
  metadata?: Record<string, unknown>;
  recursionLimit?: number;
  callbacks?: CallbackHandler[];
};

export type RunnableInput<T> = T;
export type RunnableOutput<T> = T;
