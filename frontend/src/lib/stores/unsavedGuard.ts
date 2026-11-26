// One place to ask "is it OK to throw away what is not saved yet?".
//
// The screen that owns unsaved edits registers a check while it is mounted. Anything that would
// destroy that screen's state (leaving the page, closing a connection, switching database or
// schema, closing the window) calls `confirmLeave()` first and stops when it returns false.

type Check = () => Promise<boolean>;

const checks = new Set<Check>();
let pending: Promise<boolean> | null = null;

/** Registers a check; returns the function that removes it again. */
export function registerUnsavedGuard(check: Check): () => void {
  checks.add(check);
  return () => {
    checks.delete(check);
  };
}

/** Resolves true when it is safe to continue. Overlapping calls share one question. */
export function confirmLeave(): Promise<boolean> {
  if (pending) return pending;
  const run = (async () => {
    for (const check of checks) {
      if (!(await check())) return false;
    }
    return true;
  })().finally(() => {
    pending = null;
  });
  pending = run;
  return run;
}
