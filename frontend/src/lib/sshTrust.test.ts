import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('$lib/stores/uiNotifications.svelte', () => ({ confirmModal: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import { confirmModal } from '$lib/stores/uiNotifications.svelte';
import { parseHostKeyProblem, withHostTrust } from './sshTrust';

const mockInvoke = invoke as unknown as Mock;
const mockConfirm = confirmModal as unknown as Mock;

const UNKNOWN = 'Tunnel error: SSH_HOST_UNKNOWN [bastion.example|22|ed25519|SHA256:AbC123+/xyzxyzxyzxyz]';
const CHANGED = UNKNOWN.replace('UNKNOWN', 'CHANGED');

beforeEach(() => {
  mockInvoke.mockReset();
  mockConfirm.mockReset();
});

describe('SSH host trust', () => {
  it('reads the marker out of a wrapped error message', () => {
    expect(parseHostKeyProblem(`postgres connection failed: ${UNKNOWN}`)).toEqual({
      kind: 'unknown',
      host: 'bastion.example',
      port: 22,
      keyType: 'ed25519',
      fingerprint: 'SHA256:AbC123+/xyzxyzxyzxyz',
    });
    expect(parseHostKeyProblem('connection refused')).toBeNull();
  });

  it('trusts and retries only after the user agrees', async () => {
    mockConfirm.mockResolvedValue(true);
    const action = vi.fn().mockRejectedValueOnce(UNKNOWN).mockResolvedValue('ok');
    await expect(withHostTrust(action)).resolves.toBe('ok');
    expect(mockInvoke).toHaveBeenCalledWith('ssh_trust_host', expect.objectContaining({ host: 'bastion.example', port: 22 }));
    expect(action).toHaveBeenCalledTimes(2);
  });

  it('refusing leaves the host untrusted and surfaces the error', async () => {
    mockConfirm.mockResolvedValue(false);
    const action = vi.fn().mockRejectedValue(UNKNOWN);
    await expect(withHostTrust(action)).rejects.toBe(UNKNOWN);
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(action).toHaveBeenCalledTimes(1);
  });

  it('a changed key is never trusted in one step', async () => {
    mockConfirm.mockResolvedValueOnce(true).mockResolvedValueOnce(false);
    const action = vi.fn().mockRejectedValueOnce(CHANGED).mockRejectedValue(UNKNOWN);
    await expect(withHostTrust(action)).rejects.toBe(UNKNOWN);
    expect(mockInvoke.mock.calls.map((c) => c[0])).toEqual(['ssh_forget_host']);
  });

  it('also handles failures reported inside a result', async () => {
    mockConfirm.mockResolvedValue(true);
    const action = vi
      .fn()
      .mockResolvedValueOnce({ success: false, message: UNKNOWN })
      .mockResolvedValue({ success: true, message: 'ok' });
    type Report = { success: boolean; message: string };
    await expect(withHostTrust<Report>(action, (r) => r.message)).resolves.toEqual({ success: true, message: 'ok' });
  });
});
