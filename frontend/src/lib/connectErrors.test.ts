import { describe, it, expect } from 'vitest';
import { connectHint, describeConnectResult } from './connectErrors';

describe('describeConnectResult', () => {
  it('appends the server version on success', () => {
    const v = describeConnectResult({
      success: true,
      message: 'Successfully connected',
      server_version: 'MySQL v8.4.0',
      latency_ms: 12,
    });
    expect(v.message).toBe('Successfully connected (MySQL v8.4.0)');
    expect(v.hint).toBeNull();
    expect(v.latency_ms).toBe(12);
  });

  it('never appends "Unknown" to a failure message (regression: "timed out (Unknown)")', () => {
    const v = describeConnectResult({
      success: false,
      message: 'mysql connection timed out after 10s',
      server_version: 'Unknown',
      error_kind: 'timeout',
    });
    expect(v.message).toBe('mysql connection timed out after 10s');
    expect(v.message).not.toContain('Unknown');
    expect(v.hint).toBe(connectHint('timeout'));
  });

  it('gives no hint for a failure without a kind', () => {
    expect(describeConnectResult({ success: false, message: 'x' }).hint).toBeNull();
  });

  it('has a hint for every kind', () => {
    for (const k of ['refused', 'timeout', 'auth_failed', 'tls_failed', 'tunnel_failed', 'dns_failed', 'other'] as const) {
      expect(connectHint(k)).toBeTruthy();
    }
  });
});
