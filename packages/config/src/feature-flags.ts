export const FEATURE_FLAGS = ["streaming", "subgraphs", "multi-agent", "eval", "fleet"] as const;
export type FeatureFlag = (typeof FEATURE_FLAGS)[number];

const toEnvKey = (flag: FeatureFlag): string =>
  `FEATURE_${flag.replace(/-/g, "_").toUpperCase()}`;

const isTruthy = (value: string | undefined): boolean => {
  if (value === undefined) {
    return false;
  }
  return value.trim().toLowerCase() === "true";
};

export const isEnabled = (flag: FeatureFlag, source: NodeJS.ProcessEnv = process.env): boolean =>
  isTruthy(source[toEnvKey(flag)]);

export const getAllFlags = (source: NodeJS.ProcessEnv = process.env): Record<FeatureFlag, boolean> => ({
  streaming: isEnabled("streaming", source),
  subgraphs: isEnabled("subgraphs", source),
  "multi-agent": isEnabled("multi-agent", source),
  eval: isEnabled("eval", source),
  fleet: isEnabled("fleet", source)
});
