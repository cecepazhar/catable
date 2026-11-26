import { invoke } from '@tauri-apps/api/core';
import type { ConnectErrorKind } from '$lib/connectErrors';
import type { ChangeSet } from '$lib/grid/staging.svelte';
import { workspace } from '$lib/stores/workspace.svelte';
import { withHostTrust } from '$lib/sshTrust';

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
  /** PostgreSQL schema; absent for single-namespace engines. */
  schema?: string | null;
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
  /** Tables of the active schema (what every page lists). */
  tables = $state<TableInfo[]>([]);
  /** Every table the connection returned, across schemas. */
  allTables = $state<TableInfo[]>([]);
  /** Schemas found (PostgreSQL); empty for single-namespace engines. */
  schemas = $state<string[]>([]);
  activeSchema = $state<string | null>(null);
  isLoading = $state(false);
  activeTable = $state<string | null>(null);
  /** Last schema-load failure, so pages can show it instead of an empty list. */
  loadError = $state<string | null>(null);
  sessionId = $state<string | null>(null);

  private sessionConnectionId: string | null = null;
  /** Live backend sessions, one per open connection (TablePlus-style multi-connection). */
  private sessions = new Map<string, string>();
  /** Catalog cache per open connection, so switching between them is instant. */
  private tableCache = new Map<string, { all: TableInfo[]; schema: string | null }>();
  /** Open connections, in rail order. */
  openConnections = $state<ConnectionConfig[]>([]);
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
    if (this.sessionConnectionId === cfg.id) {
      await this.closeSession();
    } else {
      const sid = this.sessions.get(cfg.id) ?? null;
      this.sessions.delete(cfg.id);
      this.tableCache.delete(cfg.id); // edited settings may point somewhere else
      void this.closeSessionId(sid);
    }
    this.openConnections = this.openConnections.map((c) => (c.id === cfg.id ? cfg : c));
    if (this.activeConnection.id === cfg.id) this.activeConnection = cfg;
    await this.loadConnections();
    this.selectedConnectionId = cfg.id;
  }

  async deleteConnection(id: string) {
    await invoke('delete_connection', { id });
    if (this.openConnections.some((c) => c.id === id)) this.closeConnection(id);
    else if (this.sessionConnectionId === id) await this.closeSession();
    await this.loadConnections();
    if (this.selectedConnectionId === id) {
      this.selectedConnectionId = this.connections[0]?.id || null;
    }
  }

  async testConnection(cfg: ConnectionConfig): Promise<ConnectionTestResult> {
    return withHostTrust(() => invoke<ConnectionTestResult>('test_database_connection', { config: cfg }), (r) => r.message);
  }

  async testSshTunnel(config: SshTunnelConfig): Promise<SshTestResult> {
    return withHostTrust(() => invoke<SshTestResult>('test_ssh_tunnel', { config }), (r) => r.message);
  }

  // ---- Session lifecycle -------------------------------------------------------------------

  /**
   * Returns the open session for the active connection, opening one if needed. The in-flight open is
   * recorded synchronously, so concurrent callers (several pages loading at once) share one attempt.
   */
  ensureSession(): Promise<string> {
    const connId = this.activeConnection.id;
    if (this.sessionId && this.sessionConnectionId === connId) return Promise.resolve(this.sessionId);
    const known = this.sessions.get(connId);
    if (known) {
      this.sessionId = known;
      this.sessionConnectionId = connId;
      return Promise.resolve(known);
    }
    if (this.opening && this.openingFor === connId) return this.opening;

    const attempt: Promise<string> = (async () => {
      // Sessions of other open connections stay alive; only this connection gets a new one.
      // An unknown SSH server is only used after the user checked its fingerprint.
      const info = await withHostTrust(() => invoke<SessionInfo>('session_open', { connectionId: connId }));
      this.sessions.set(connId, info.session_id);
      if (this.activeConnection.id === connId) {
        this.sessionId = info.session_id;
        this.sessionConnectionId = connId;
      }
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

  /** Closes the active connection's backend session (it is reopened on next use). */
  async closeSession() {
    const id = this.sessionId;
    const connId = this.sessionConnectionId;
    this.sessionId = null;
    this.sessionConnectionId = null;
    this.schemaFor = null;
    if (connId) this.sessions.delete(connId);
    await this.closeSessionId(id);
  }

  private async closeSessionId(id: string | null) {
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

  /** Remembers the current connection's tabs and schema before another one takes over. */
  private parkActive() {
    const id = this.activeConnection.id;
    if (!this.isConnected || !this.openConnections.some((c) => c.id === id)) return;
    workspace.park(id);
    const cached = this.tableCache.get(id);
    if (cached) cached.schema = this.activeSchema;
  }

  async connect(conn: ConnectionConfig) {
    this.parkActive();
    // "Connect" always starts fresh so edited settings take effect.
    const old = this.sessions.get(conn.id) ?? null;
    this.sessions.delete(conn.id);
    this.tableCache.delete(conn.id);
    workspace.forget(conn.id);
    void this.closeSessionId(old);

    this.activeConnection = conn;
    this.selectedConnectionId = conn.id;
    this.sessionId = null;
    this.sessionConnectionId = null;
    this.schemaFor = null;
    workspace.reset();
    this.activeTable = null;
    this.activeSchema = null;
    try {
      await this.ensureSession();
      this.isConnected = true;
      const i = this.openConnections.findIndex((c) => c.id === conn.id);
      this.openConnections = i < 0 ? [...this.openConnections, conn] : this.openConnections.map((c) => (c.id === conn.id ? conn : c));
      await this.loadTables(true);
    } catch (e) {
      this.openConnections = this.openConnections.filter((c) => c.id !== conn.id);
      this.isConnected = false;
      this.loadError = messageOf(e);
      throw e;
    }
  }

  /** "Connect" from a list: an already-open connection is shown again (tabs kept), else opened. */
  async open(conn: ConnectionConfig) {
    if (this.openConnections.some((c) => c.id === conn.id)) await this.switchConnection(conn.id);
    else await this.connect(conn);
  }

  /** Closes every open connection (saved connections are untouched). */
  closeAll() {
    const ids = [...this.sessions.values()];
    if (this.sessionId && !ids.includes(this.sessionId)) ids.push(this.sessionId);
    for (const c of this.openConnections) workspace.forget(c.id);
    this.sessions.clear();
    this.tableCache.clear();
    this.openConnections = [];
    this.sessionId = null;
    this.sessionConnectionId = null;
    this.schemaFor = null;
    this.isConnected = false;
    workspace.reset();
    for (const sid of ids) void this.closeSessionId(sid);
  }

  /** Switches to another open connection without reconnecting (rail click, Ctrl+1…9). */
  async switchConnection(id: string) {
    if (this.isConnected && id === this.activeConnection.id) return;
    const conn = this.openConnections.find((c) => c.id === id);
    if (!conn) return;
    this.parkActive();
    this.activeConnection = conn;
    this.selectedConnectionId = conn.id;
    const sid = this.sessions.get(id) ?? null;
    this.sessionId = sid;
    this.sessionConnectionId = sid ? id : null;
    this.isConnected = true;
    const cached = this.tableCache.get(id);
    if (cached) {
      this.activeSchema = cached.schema;
      this.applyTables(cached.all);
      this.schemaFor = id;
    } else {
      this.activeSchema = null;
      this.applyTables([]);
      this.schemaFor = null;
    }
    workspace.unpark(id);
    this.activeTable = workspace.active;
    if (!cached) await this.loadTables(true);
  }

  /** Closes the active connection; another open one (if any) takes over. */
  disconnect() {
    const id = this.activeConnection.id;
    const sid = this.sessions.get(id) ?? this.sessionId;
    this.sessions.delete(id);
    this.tableCache.delete(id);
    workspace.forget(id);
    this.openConnections = this.openConnections.filter((c) => c.id !== id);
    this.sessionId = null;
    this.sessionConnectionId = null;
    this.schemaFor = null;
    void this.closeSessionId(sid);
    const next = this.openConnections[this.openConnections.length - 1];
    if (next) {
      this.isConnected = false; // so switchConnection does not park the closed one
      void this.switchConnection(next.id);
    } else {
      this.isConnected = false;
      workspace.reset();
    }
  }

  /** Closes one open connection from the rail. */
  closeConnection(id: string) {
    if (id === this.activeConnection.id) {
      this.disconnect();
      return;
    }
    const sid = this.sessions.get(id) ?? null;
    this.sessions.delete(id);
    this.tableCache.delete(id);
    workspace.forget(id);
    this.openConnections = this.openConnections.filter((c) => c.id !== id);
    void this.closeSessionId(sid);
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
        if (this.activeConnection.id !== connId) return; // switched away meanwhile
        this.applyTables(tbls || []);
        this.schemaFor = connId;
        this.tableCache.set(connId, { all: this.allTables, schema: this.activeSchema });
        if (this.tables.length > 0 && !this.tables.some((t) => t.name === this.activeTable)) {
          this.activeTable = this.tables[0].name;
        }
      } catch (e) {
        this.applyTables([]);
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

  /** Splits the catalog by schema and keeps (or picks) the active one. */
  private applyTables(all: TableInfo[]) {
    this.allTables = all;
    const schemas = [...new Set(all.map((t) => t.schema).filter((s): s is string => !!s))].sort();
    this.schemas = schemas;
    if (schemas.length === 0) {
      this.activeSchema = null;
    } else if (!this.activeSchema || !schemas.includes(this.activeSchema)) {
      this.activeSchema = schemas.includes('public') ? 'public' : schemas[0];
    }
    this.tables = this.activeSchema ? all.filter((t) => t.schema === this.activeSchema) : all;
  }

  /** Switches the schema shown in the sidebar; open tabs belong to the old one and close. */
  setSchema(schema: string) {
    if (schema === this.activeSchema || !this.schemas.includes(schema)) return;
    this.activeSchema = schema;
    const cached = this.tableCache.get(this.activeConnection.id);
    if (cached) cached.schema = schema;
    this.tables = this.allTables.filter((t) => t.schema === schema);
    this.activeTable = null;
    workspace.reset();
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
      return invoke<BrowseResult>('session_browse', { sessionId: sid, request: this.withSchema(request) });
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
    return invoke<TableStructure>('session_describe_table', { sessionId: sid, table, schema: schema ?? this.activeSchema });
  }

  /** The exact SQL a staged change set will run (shown before commit). Touches nothing. */
  async previewChanges(changes: ChangeSet): Promise<string[]> {
    const sid = await this.ensureSession();
    return invoke<string[]>('session_preview_changes', { sessionId: sid, changes: { ...changes, schema: changes.schema ?? this.activeSchema } });
  }

  /** Applies a staged change set in ONE backend transaction; rejects (and saves nothing) on failure. */
  async applyChanges(changes: ChangeSet): Promise<ApplyResult> {
    const sid = await this.ensureSession();
    const result = await invoke<ApplyResult>('session_apply_changes', {
      sessionId: sid,
      changes: { ...changes, schema: changes.schema ?? this.activeSchema },
    });
    this.invalidateSchema(); // row counts changed
    return result;
  }

  /** The dialect-correct `SELECT` for a browse request, built by the backend (never by the UI). */
  async buildSelect(request: BrowseRequest): Promise<string> {
    const sid = await this.ensureSession();
    return invoke<string>('session_build_select', { sessionId: sid, request: this.withSchema(request) });
  }

  // ---- Databases on the active connection's server ------------------------------------------

  /** True when the active connection's server holds several databases (PostgreSQL / MySQL). */
  get supportsDatabases(): boolean {
    return ['postgres', 'mysql', 'mariadb'].includes(this.activeConnection.driver);
  }

  /** Databases on the active connection's server (credentials stay in Rust). */
  async listDatabases(): Promise<string[]> {
    return invoke<string[]>('saved_list_databases', { connectionId: this.activeConnection.id });
  }

  /** Creates a database on the active connection's server; returns the final (normalised) name. */
  async createDatabase(name: string): Promise<string> {
    return invoke<string>('saved_create_database', { connectionId: this.activeConnection.id, name });
  }

  /** Points the active connection at another database (saved) and reconnects. */
  async switchDatabase(database: string) {
    if (database === this.activeConnection.database) return;
    const next: ConnectionConfig = { ...this.activeConnection, database };
    await invoke('save_connection', { config: next });
    await this.loadConnections();
    await this.connect(next);
  }

  /** Browse requests target the active schema unless they name one. */
  private withSchema(request: BrowseRequest): BrowseRequest {
    return request.schema || !this.activeSchema ? request : { ...request, schema: this.activeSchema };
  }

  async evaluateSafety(sql: string): Promise<GuardVerdict> {
    // Safety evaluation needs the policy fields only — never send secrets over IPC for it.
    return invoke<GuardVerdict>('evaluate_query_safety', {
      sql,
      config: { ...this.activeConnection, password: null, ssh_tunnel: null },
    });
  }

  setActiveConnection(conn: ConnectionConfig) {
    if (conn.id !== this.activeConnection.id && !this.openConnections.some((c) => c.id === this.activeConnection.id)) {
      void this.closeSession();
    }
    this.activeConnection = conn;
    void this.loadTables(true);
  }
}

export const dbStore = new DatabaseStore();
