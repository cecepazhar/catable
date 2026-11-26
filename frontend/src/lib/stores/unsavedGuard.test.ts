import { describe, it, expect, vi } from 'vitest';
import { confirmLeave, registerUnsavedGuard } from './unsavedGuard';

describe('unsaved guard', () => {
  it('allows leaving when nothing is registered', async () => {
    await expect(confirmLeave()).resolves.toBe(true);
  });

  it('blocks when the owner says no, and stops asking once removed', async () => {
    const check = vi.fn(async () => false);
    const remove = registerUnsavedGuard(check);
    await expect(confirmLeave()).resolves.toBe(false);
    remove();
    await expect(confirmLeave()).resolves.toBe(true);
    expect(check).toHaveBeenCalledTimes(1);
  });

  it('asks only once when two actions race', async () => {
    const check = vi.fn(async () => true);
    const remove = registerUnsavedGuard(check);
    await Promise.all([confirmLeave(), confirmLeave()]);
    expect(check).toHaveBeenCalledTimes(1);
    remove();
  });
});
