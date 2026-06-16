/**
 * Cycle-safe structural equality for channel values.
 *
 * Replaces `JSON.stringify(a) === JSON.stringify(b)`, which crashes on circular
 * references and gives false negatives when object keys are in a different order.
 * Compares primitives with `Object.is` (so `NaN` equals `NaN`), `Date` by instant,
 * arrays element-wise, and plain objects by their own enumerable keys — guarding
 * against cycles and shared references with a pair map.
 */
export const structuralEqual = (left: unknown, right: unknown): boolean =>
  deepEqual(left, right, new WeakMap());

const deepEqual = (a: unknown, b: unknown, pairs: WeakMap<object, WeakSet<object>>): boolean => {
  if (Object.is(a, b)) {
    return true;
  }
  if (typeof a !== typeof b || a === null || b === null || typeof a !== "object") {
    // Different primitives (Object.is already ruled out equality) or type mismatch.
    return false;
  }

  if (a instanceof Date || b instanceof Date) {
    return a instanceof Date && b instanceof Date && a.getTime() === b.getTime();
  }

  const aObj = a as object;
  const bObj = b as object;

  // If we're already comparing this exact pair (a cycle, or a shared reference),
  // treat it as equal — the surrounding comparison decides the real outcome.
  const seenForA = pairs.get(aObj);
  if (seenForA?.has(bObj)) {
    return true;
  }
  if (seenForA) {
    seenForA.add(bObj);
  } else {
    pairs.set(aObj, new WeakSet([bObj]));
  }

  const aIsArray = Array.isArray(a);
  const bIsArray = Array.isArray(b);
  if (aIsArray !== bIsArray) {
    return false;
  }

  if (aIsArray && bIsArray) {
    if (a.length !== b.length) {
      return false;
    }
    for (let i = 0; i < a.length; i += 1) {
      if (!deepEqual(a[i], b[i], pairs)) {
        return false;
      }
    }
    return true;
  }

  const aRecord = a as Record<string, unknown>;
  const bRecord = b as Record<string, unknown>;
  const aKeys = Object.keys(aRecord);
  const bKeys = Object.keys(bRecord);
  if (aKeys.length !== bKeys.length) {
    return false;
  }
  for (const key of aKeys) {
    if (!Object.prototype.hasOwnProperty.call(bRecord, key)) {
      return false;
    }
    if (!deepEqual(aRecord[key], bRecord[key], pairs)) {
      return false;
    }
  }
  return true;
};
