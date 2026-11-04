/** Presentation helpers for connection-test results (Rust `ConnectErrorKind` → UI text). */

export type ConnectErrorKind =
  | 'refused'
  | 'timeout'
  | 'auth_failed'
  | 'tls_failed'
  | 'tunnel_failed'
  | 'dns_failed'
  | 'other';

export interface ConnectResultLike {
  success: boolean;
  message: string;
  server_version?: string | null;
  latency_ms?: number;
  error_kind?: ConnectErrorKind | null;
}

export interface ConnectResultView {
  success: boolean;
  message: string;
  hint: string | null;
  latency_ms?: number;
}

const HINTS: Record<ConnectErrorKind, string> = {
  refused: 'Nothing is listening on that host/port. Check the port and that the server is running.',
  timeout: 'No response in time. Check firewall/port, VPN, or whether an SSH tunnel is required.',
  auth_failed: 'The server rejected the credentials. Check user, password and database.',
  tls_failed: "TLS/SSL negotiation failed. Check the server's SSL settings.",
  tunnel_failed: 'The SSH tunnel could not be established. Test the SSH settings first.',
  dns_failed: 'The host name could not be resolved. Check the host spelling and DNS.',
  other: 'Connection failed. See the message for details.',
};

export function connectHint(kind?: ConnectErrorKind | null): string | null {
  return kind ? (HINTS[kind] ?? null) : null;
}

/** The server version is appended only on success, and never as the placeholder "Unknown". */
export function describeConnectResult(res: ConnectResultLike): ConnectResultView {
  const version = res.server_version?.trim();
  const showVersion = res.success && !!version && version.toLowerCase() !== 'unknown';
  return {
    success: res.success,
    message: showVersion ? `${res.message} (${version})` : res.message,
    hint: res.success ? null : connectHint(res.error_kind),
    latency_ms: res.latency_ms,
  };
}
