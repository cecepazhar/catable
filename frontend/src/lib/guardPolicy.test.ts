import { describe, it, expect } from 'vitest';
import { needsConfirmation, type Environment, type Risk } from './guardPolicy';

describe('needsConfirmation', () => {
  const envs: Environment[] = ['development', 'staging', 'production'];

  it('always confirms destructive queries, including on Development', () => {
    for (const env of envs) expect(needsConfirmation('destructive', env)).toBe(true);
  });

  it('confirms warnings outside Development only', () => {
    expect(needsConfirmation('warning', 'development')).toBe(false);
    expect(needsConfirmation('warning', 'staging')).toBe(true);
    expect(needsConfirmation('warning', 'production')).toBe(true);
  });

  it('never prompts for safe queries, and blocked ones are refused before any prompt', () => {
    for (const env of envs) {
      expect(needsConfirmation('safe', env)).toBe(false);
      expect(needsConfirmation('blocked' as Risk, env)).toBe(false);
    }
  });
});
