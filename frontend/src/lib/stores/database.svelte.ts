import { invoke } from '@tauri-apps/api/core';
import type { ConnectErrorKind } from '$lib/connectErrors';
import type { ChangeSet } from '$lib/grid/staging.svelte';

export type DriverKind = 'sqlite' | 'postgres' | 'mysql' | 'mariadb' | 'duckdb' | 'redis';
export type Environment = 'development' | 'staging' | 'production';

export interface SshTunnelConfig {
  enabled: boolean;
  host: string;
  port: number;
  user: string;
  auth_type: 'password' | 'key' | 'agent';
  key_path?: string | null;
  key_passphrase?: string | null;
  password?: string | null;
}

export interface ConnectionConfig {
  id: string;
  name: string;
  driver: DriverKind;
  environment: Environment;
  host: string;
  port: number;
  database: string;
  user: string;
  password?: string | null;
  read_only: boolean;
  ssh_tunnel?: SshTunnelConfig | null;
  group_name?: string | null;
  workspace_id?: string | null;
  /** Connect timeout in seconds; null/undefined = backend default (10s). */
  connect_timeout_secs?: number | null;
}

export interface WorkspaceRecord {
  id: string;
  name: string;
  color: string;
  description?: string | null;
  created_at: number;
}

export interface PersonalizedGroupRecord {
  id: string;
  workspace_id: string;
  name: string;
  color: string;
  description?: string | null;
  created_at: number;
}

export interface TableColumnInfo {
  name: string;
  data_type: string;
  is_pk: boolean;
  not_null: boolean;
}

export interface TableInfo {
  name: string;
  table_type: string;
  columns: TableColumnInfo[];
  row_count: number;
}

export interface GuardVerdict {
  risk: 'safe' | 'warning' | 'destructive' | 'blocked';
  reason?: string | null;
}

export interface QueryResult {
  columns: string[];
  rows: any[][];
  rows_affected: number;
  duration_ms: number;
  error?: string | null;
  guard_verdict?: GuardVerdict | null;
  /** True when more rows existed than the backend's 10,000-row cap returned. */
  truncated?: boolean;
}

export interface ConnectionTestResult {
  success: boolean;
  server_version: string;
  latency_ms: number;
  message: string;
  /** Set by the backend on failure. */
  error_kind?: ConnectErrorKind | null;
}

export type FilterOp =
  | 'eq' | 'ne' | 'lt' | 'lte' | 'gt' | 'gte'
  | 'like' | 'not_like' | 'contains' | 'starts_with'
  | 'in' | 'not_in' | 'is_null' | 'is_not_null' | 'between';

export interface BrowseFilter {
  column: string;
  op: FilterOp;
  value?: string | null;
  values?: string[];
}

export interface BrowseOrderBy {
  column: string;
  descending?: boolean;
}

/** Describes one page of a table; the backend builds and runs the SQL. */
export interface BrowseRequest {
  table: string;
  schema?: string | null;
  limit?: number;
  offset?: number;
  order_by?: BrowseOrderBy[];
  filters?: BrowseFilter[];
  filter_mode?: 'and' | 'or';
  /** Quick search: rows where any of `search_columns` contains this text. */
  search?: string | null;
  search_columns?: string[];
  /** Also run an exact COUNT(*) with the same filters. */
  with_total?: boolean;
}

export interface BrowseResult {
  result: QueryResult;
  total: number | null;
  /** The SELECT that produced `result`. */
  sql: string;
}

export interface ColumnDetail {
  name: string;
  data_type: string;
  nullable: boolean;
  default: string | null;
  is_pk: boolean;
  is_auto_increment: boolean;
  comment: string | null;
  ordinal: number;
}

export interface ForeignKeyInfo {
  name: string | null;
  columns: string[];
  ref_table: string;
  ref_columns: string[];
  on_update: string | null;
  on_delete: string | null;
}

export interface IndexDetail {
  name: string;
  columns: string[];
  unique: boolean;
  primary: boolean;
  method: string | null;
}

export interface TableStructure {
  table: string;
  schema: string | null;
  columns: ColumnDetail[];
  foreign_keys: ForeignKeyInfo[];
  indexes: IndexDetail[];
  ddl: string | null;
}

export interface ApplyResult {
  statements: number;
  rows_affected: number;
  sql: string[];
}

export interface SessionInfo {
  session_id: string;
  connection_id: string;
  name: string;
  driver: DriverKind;
  tunneled: boolean;
  open_ms: number;
}

export interface SshTestResult {
  success: boolean;
  message: string;
  latency_ms: number;
}

export const DEFAULT_SANDBOX_CONNECTION: ConnectionConfig = {
  id: 'conn-sandbox',
  name: 'Local Sandbox DB',
  driver: 'sqlite',
  environment: 'development',
  host: 'localhost',
  port: 0,
  database: 'sandbox.db',
  user: 'admin',
  password: null,
  read_only: false,
  ssh_tunnel: null,
  group_name: null,
  connect_timeout_secs: null,
};

const READ_ONLY_SQL = /^\s*(select|show|describe|desc|explain|pragma|with)\b/i;

function errorResult(message: string): QueryResult {
  return { columns: [], rows: [], rows_affected: 0, duration_ms: 0, error: message };
}

function messageOf(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message);
  return String(e);
}

const SESSION_GONE = /is not open/i;

// Global reactive store for database operations.
//
// All database access goes through a backend *session* (tunnel + connection pool kept alive by
// Rust). The WebView never sends credentials with a query — only the opaque session id.
class DatabaseStore {
  connections = $state<ConnectionConfig[]>([DEFAULT_SANDBOX_CONNECTION]);
  activeConnection = $state<ConnectionConfig>(DEFAULT_SANDBOX_CONNECTION);
  selectedConnectionId = $state<string | null>(null);
  isConnected = $state<boolean>(false);
  tables = $state<TableInfo[]>([]);
  isLoading = $state(false);
  activeTable = $state<string | null>(null);
  /** Last schema-load failure, so pages can show it instead of an empty list. */
  loadError = $state<string | null>(null);
  sessionId = $state<string | null>(null);

  private sessionConnectionId: string | null = null;
  private opening: Promise<string> | null = null;
  private openingFor: string | null = null;
  /** Connection id whose schema is currently cached in `tables` (null = stale/none). */
  private schemaFor: string | null = null;
  private schemaLoad: Promise<void> | null = null;

  async loadConnections() {
    try {
      const conns = await invoke<ConnectionConfig[]>('list_connections');
      if (conns && conns.length > 0) {
        this.connections = conns;
        // Keep active or set to first
        if (!this.connections.some((c) => c.id === this.activeConnection.id)) {
          this.activeConnection = this.connections[0];
        }
        if (!this.selectedConnectionId || !this.connections.some((c) => c.id === this.selectedConnectionId)) {
          this.selectedConnectionId = this.activeConnection.id;
        }
      } else {
        this.connections = [DEFAULT_SANDBOX_CONNECTION];
        this.activeConnection = DEFAULT_SANDBOX_CONNECTION;
        this.selectedConnectionId = DEFAULT_SANDBOX_CONNECTION.id;
      }
    } catch {
      this.connections = [DEFAULT_SANDBOX_CONNECTION];
      this.selectedConnectionId = DEFAULT_SANDBOX_CONNECTION.id;
    }
  }

  async saveConnection(cfg: ConnectionConfig) {
    await invoke('save_connection', { config: cfg });
    // Saved settings may differ from the open session (host, tunnel, timeout…): drop it so the next
    // use reconnects with them.
    if (this.sessionConnectionId === cfg.id) await this.closeSession();
    await this.loadConnections();
    this.selectedConnectionId = cfg.id;
  }

  async deleteConnection(id: string) {
    await invoke('delete_connection', { id });
    if (this.sessionConnectionId === id) await this.closeSession();
    await this.loadConnections();
    if (this.selectedConnectionId === id) {
      this.selectedConnectionId = this.connections[0]?.id || null;
    }
  }

  async testConnection(cfg: ConnectionConfig): Promise<ConnectionTestResult> {
    return invoke<ConnectionTestResult>('test_database_connection', { config: cfg });
  }

  async testSshTunnel(config: SshTunnelConfig): Promise<SshTestResult> {
    return invoke<SshTestResult>('test_ssh_tunnel', { config });
  }

  // ---- Session lifecycle -------------------------------------------------------------------

  /**
   * Returns the open session for the active connection, opening one if needed. The in-flight open is
   * recorded synchronously, so concurrent callers (several pages loading at once) share one attempt.
   */
  ensureSession(): Promise<string> {
    const connId = this.activeConnection.id;
    if (this.sessionId && this.sessionConnectionId === connId) return Promise.resolve(this.sessionId);
    if (this.opening && this.openingFor === connId) return this.opening;

    const attempt: Promise<string> = (async () => {
      // A session that belongs to another connection is dropped first.
      await this.closeSession();
      const info = await invoke<SessionInfo>('session_open', { connectionId: connId });
      this.sessionId = info.session_id;
      this.sessionConnectionId = connId;
      return info.session_id;
    })().finally(() => {
      if (this.opening === attempt) {
        this.opening = null;
        this.openingFor = null;
      }
    });
    this.opening = attempt;
    this.openingFor = connId;
    return attempt;
  }

  async closeSession() {
    const id = this.sessionId;
    this.sessionId = null;
    this.sessionConnectionId = null;
    this.schemaFor = null;
    if (id) {
      try {
        await invoke('session_close', { sessionId: id });
      } catch {
        // The backend may already have dropped it; nothing to clean up.
      }
    }
  }

  /** Round-trip time of the active session in ms, or null when there is none / it failed. */
  async ping(): Promise<number | null> {
    if (!this.sessionId) return null;
    try {
      return await invoke<number>('session_ping', { sessionId: this.sessionId });
    } catch {
      return null;
    }
  }

  async connect(conn: ConnectionConfig) {
    this.activeConnection = conn;
    this.selectedConnectionId = conn.id;
    // "Connect" always starts fresh so edited settings take effect.
    await this.closeSession();
    try {
      await this.ensureSession();
      this.isConnected = true;
      await this.loadTables(true);
    } catch (e) {
      this.isConnected = false;
      this.loadError = messageOf(e);
      throw e;
    }
  }

  disconnect() {
    this.isConnected = false;
    void this.closeSession();
  }

  // ---- Schema ------------------------------------------------------------------------------

  /**
   * Loads tables + columns. The result is cached per connection: pages call this on mount, but the
   * backend is only hit again after `force`, a connection change, or a statement that changed data.
   */
  async loadTables(force = false): Promise<void> {
    const connId = this.activeConnection.id;
    if (!force && this.schemaFor === connId) return;
    if (this.schemaLoad) return this.schemaLoad;

    this.isLoading = true;
    this.loadError = null;
    const run = (async () => {
      try {
        const sid = await this.ensureSession();
        const tbls = await invoke<TableInfo[]>('session_list_tables', { sessionId: sid });
        this.tables = tbls || [];
        this.schemaFor = connId;
        if (this.tables.length > 0 && !this.tables.some((t) => t.name === this.activeTable)) {
          this.activeTable = this.tables[0].name;
        }
      } catch (e) {
        this.tables = [];
        this.schemaFor = null;
        this.loadError = messageOf(e);
      } finally {
        this.isLoading = false;
        this.schemaLoad = null;
      }
    })();
    this.schemaLoad = run;
    return run;
  }

  /** Marks the cached schema/row counts stale; the next `loadTables()` refetches. */
  invalidateSchema() {
    this.schemaFor = null;
  }

  // ---- Queries -----------------------------------------------------------------------------

  async executeQuery(sql: string): Promise<QueryResult> {
    const run = async (): Promise<QueryResult> => {
      const sid = await this.ensureSession();
      return invoke<QueryResult>('session_execute', { sessionId: sid, sql });
    };

    let res: QueryResult;
    try {
      res = await run();
    } catch (first) {
      // The backend lost the session (e.g. it restarted): reconnect once and retry.
      if (SESSION_GONE.test(messageOf(first))) {
        await this.closeSession();
        try {
          res = await run();
        } catch (second) {
          return errorResult(messageOf(second));
        }
      } else {
        return errorResult(messageOf(first));
      }
    }

    // Anything that is not clearly a read may have changed data or structure: cached row counts and
    // the table list are no longer trustworthy.
    if (!res.error && !READ_ONLY_SQL.test(sql)) this.invalidateSchema();
    return res;
  }

  /** One page of a table with server-side paging, sorting and filtering. Never throws. */
  async browseTable(request: BrowseRequest): Promise<BrowseResult> {
    const run = async () => {
      const sid = await this.ensureSession();
      return invoke<BrowseResult>('session_browse', { sessionId: sid, request });
    };
    try {
      try {
        return await run();
      } catch (first) {
        if (!SESSION_GONE.test(messageOf(first))) throw first;
        await this.closeSession();
        return await run();
      }
    } catch (e) {
      return { result: errorResult(messageOf(e)), total: null, sql: '' };
    }
  }

  /** Columns, keys, foreign keys, indexes and DDL of a table, read from the database's catalog. */
  async describeTable(table: string, schema: string | null = null): Promise<TableStructure> {
    const sid = await this.ensureSession();
    return invoke<TableStructure>('session_describe_table', { sessionId: sid, table, schema });
  }

  /** The exact SQL a staged change set will run (shown before commit). Touches nothing. */
  async previewChanges(changes: ChangeSet): Promise<string[]> {
    const sid = await this.ensureSession();
    return invoke<string[]>('session_preview_changes', { sessionId: sid, changes });
  }

  /** Applies a staged change set in ONE backend transaction; rejects (and saves nothing) on failure. */
  async applyChanges(changes: ChangeSet): Promise<ApplyResult> {
    const sid = await this.ensureSession();
    const result = await invoke<ApplyResult>('session_apply_changes', { sessionId: sid, changes });
    this.invalidateSchema(); // row counts changed
    return result;
  }

  /** The dialect-correct `SELECT` for a browse request, built by the backend (never by the UI). */
  async buildSelect(request: BrowseRequest): Promise<string> {
    const sid = await this.ensureSession();
    return invoke<string>('session_build_select', { sessionId: sid, request });
  }

  async evaluateSafety(sql: string): Promise<GuardVerdict> {
    // Safety evaluation needs the policy fields only — never send secrets over IPC for it.
    return invoke<GuardVerdict>('evaluate_query_safety', {
      sql,
      config: { ...this.activeConnection, password: null, ssh_tunnel: null },
    });
  }

  setActiveConnection(conn: ConnectionConfig) {
    if (conn.id !== this.activeConnection.id) {
      void this.closeSession();
    }
    this.activeConnection = conn;
    void this.loadTables(true);
  }
}

export const dbStore = new DatabaseStore();
