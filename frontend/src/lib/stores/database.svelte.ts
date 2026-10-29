import { invoke } from '@tauri-apps/api/core';

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
}

export interface ConnectionTestResult {
  success: boolean;
  server_version: string;
  latency_ms: number;
  message: string;
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
};

// Global reactive store for database operations
class DatabaseStore {
  connections = $state<ConnectionConfig[]>([DEFAULT_SANDBOX_CONNECTION]);
  activeConnection = $state<ConnectionConfig>(DEFAULT_SANDBOX_CONNECTION);
  selectedConnectionId = $state<string | null>(null);
  isConnected = $state<boolean>(false);
  tables = $state<TableInfo[]>([]);
  isLoading = $state(false);
  activeTable = $state<string | null>('users');

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
    await this.loadConnections();
    this.selectedConnectionId = cfg.id;
  }

  async deleteConnection(id: string) {
    await invoke('delete_connection', { id });
    await this.loadConnections();
    if (this.selectedConnectionId === id) {
      this.selectedConnectionId = this.connections[0]?.id || null;
    }
  }

  async testConnection(cfg: ConnectionConfig): Promise<ConnectionTestResult> {
    return invoke<ConnectionTestResult>('test_database_connection', { config: cfg });
  }

  async testSshTunnel(config: SshTunnelConfig): Promise<{ success: boolean; message: string; latency_ms?: number }> {
    return invoke<{ success: boolean; message: string; latency_ms?: number }>('test_ssh_tunnel', { config });
  }

  async connect(conn: ConnectionConfig) {
    this.activeConnection = conn;
    this.selectedConnectionId = conn.id;
    this.isConnected = true;
    await this.loadTables();
  }

  disconnect() {
    this.isConnected = false;
  }

  async loadTables() {
    this.isLoading = true;
    try {
      const tbls = await invoke<TableInfo[]>('list_database_tables', {
        config: this.activeConnection,
      });
      this.tables = tbls || [];
      if (this.tables.length > 0 && !this.tables.some((t) => t.name === this.activeTable)) {
        this.activeTable = this.tables[0].name;
      }
    } catch {
      this.tables = [];
    } finally {
      this.isLoading = false;
    }
  }

  async executeQuery(sql: string): Promise<QueryResult> {
    return invoke<QueryResult>('execute_database_query', {
      config: this.activeConnection,
      sql,
    });
  }

  async evaluateSafety(sql: string): Promise<GuardVerdict> {
    return invoke<GuardVerdict>('evaluate_query_safety', {
      sql,
      config: this.activeConnection,
    });
  }

  setActiveConnection(conn: ConnectionConfig) {
    this.activeConnection = conn;
    void this.loadTables();
  }
}

export const dbStore = new DatabaseStore();
