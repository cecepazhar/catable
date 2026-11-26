// The backend refuses SSH servers whose host key is not trusted and says why with a marker:
//   SSH_HOST_UNKNOWN [host|port|key-type|SHA256:…]
//   SSH_HOST_CHANGED [host|port|key-type|SHA256:…]
// This turns that into a question for the user and retries once they approve.

import { invoke } from '@tauri-apps/api/core';
import { confirmModal } from '$lib/stores/uiNotifications.svelte';

export interface HostKeyProblem {
  kind: 'unknown' | 'changed';
  host: string;
  port: number;
  keyType: string;
  fingerprint: string;
}

const MARKER = /SSH_HOST_(UNKNOWN|CHANGED) \[([^|\]]+)\|(\d+)\|([^|\]]*)\|(SHA256:[A-Za-z0-9+/]+)\]/;

export function parseHostKeyProblem(message: unknown): HostKeyProblem | null {
  const m = MARKER.exec(typeof message === 'string' ? message : String((message as Error)?.message ?? message ?? ''));
  if (!m) return null;
  return {
    kind: m[1] === 'UNKNOWN' ? 'unknown' : 'changed',
    host: m[2],
    port: Number(m[3]),
    keyType: m[4],
    fingerprint: m[5],
  };
}

let pending: Promise<boolean> | null = null;

/** Asks the user about one problem; true means "it is dealt with, try again". */
async function resolve(p: HostKeyProblem): Promise<boolean> {
  const server = `${p.host}:${p.port}`;
  if (p.kind === 'unknown') {
    const ok = await confirmModal(
      `CATable has not seen the SSH server ${server} before.\n\n${p.keyType} key fingerprint:\n${p.fingerprint}\n\n` +
        'Continue only if this matches the fingerprint your server administrator gave you. ' +
        'Your SSH password or key is not sent until you trust it.',
      'Trust this SSH server?',
      false,
      'Trust and connect',
      'Cancel'
    );
    if (!ok) return false;
    await invoke('ssh_trust_host', { host: p.host, port: p.port, keyType: p.keyType, fingerprint: p.fingerprint });
    return true;
  }
  const forget = await confirmModal(
    `The key of ${server} is NOT the one you trusted before. Someone may be intercepting this connection.\n\n` +
      `New ${p.keyType} fingerprint:\n${p.fingerprint}\n\n` +
      'Remove the old key only if you know the server was reinstalled or its key was rotated. ' +
      'You will then be asked to check the new fingerprint.',
    'SSH host key changed!',
    true,
    'Remove old key',
    'Keep blocking'
  );
  if (!forget) return false;
  await invoke('ssh_forget_host', { host: p.host, port: p.port });
  return true;
}

function ask(p: HostKeyProblem): Promise<boolean> {
  // Several requests can hit the same server at once: show one question.
  pending ??= resolve(p).finally(() => {
    pending = null;
  });
  return pending;
}

/**
 * Runs `action`; while it fails (thrown, or reported by `messageOf` in its result) because of an
 * untrusted host key that the user then resolves, runs it again. At most two questions in a row
 * (forget the old key, then trust the new one).
 */
export async function withHostTrust<T>(
  action: () => Promise<T>,
  messageOf: (result: T) => unknown = () => null
): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    let result: T;
    try {
      result = await action();
    } catch (e) {
      const problem = parseHostKeyProblem(e);
      if (!problem || attempt >= 2 || !(await ask(problem))) throw e;
      continue;
    }
    const problem = parseHostKeyProblem(messageOf(result));
    if (!problem || attempt >= 2 || !(await ask(problem))) return result;
  }
}
