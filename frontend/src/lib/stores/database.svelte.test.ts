import { describe, it, expect, vi, beforeEach, type Mock } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import { dbStore, DEFAULT_SANDBOX_CONNECTION, type ConnectionConfig } from './database.svelte';

const mockInvoke = invoke as unknown as Mock;

const SECRET = 'hunter2-do-not-leak';
const connA: ConnectionConfig = {
  ...DEFAULT_SANDBOX_CONNECTION,
  id: 'conn-a',
  name: 'A',
  password: SECRET,
};
const connB: ConnectionConfig = { ...DEFAULT_SANDBOX_CONNECTION, id: 'conn-b', name: 'B' };

type Script = {
  openFails?: string;
  executeFailsOnce?: string;
  browseFailsOnce?: string;
};

function backend(script: Script = {}) {
  let opened = 0;
  let executeFailures = script.executeFailsOnce ? 1 : 0;
  let browseFailures = script.browseFailsOnce ? 1 : 0;
  mockInvoke.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    switch (cmd) {
      case 'session_open':
        if (script.openFails) throw script.openFails;
        opened++;
        return {
          session_id: `s${opened}`,
          connection_id: args?.connectionId,
          name: 'x',
          driver: 'sqlite',
          tunneled: false,
          open_ms: 1,
        };
      case 'session_list_tables':
        return [{ name: 'users', table_type: 'table', columns: [], row_count: 5 }];
      case 'session_execute':
        if (executeFailures > 0) {
          executeFailures--;
          throw script.executeFailsOnce;
        }
        return { columns: ['n'], rows: [[1]], rows_affected: 1, duration_ms: 1, error: null };
      case 'session_browse':
        if (browseFailures > 0) {
          browseFailures--;
          throw script.browseFailsOnce;
        }
        return {
          result: { columns: ['id'], rows: [[1], [2]], rows_affected: 2, duration_ms: 1, error: null },
          total: 42,
          sql: 'SELECT * FROM `users` LIMIT 2 OFFSET 0',
        };
      case 'session_describe_table':
        return { table: args?.table, schema: args?.schema ?? null, columns: [], foreign_keys: [], indexes: [], ddl: 'CREATE TABLE x ();' };
      case 'session_preview_changes':
        return ["UPDATE `users` SET `name` = 'x' WHERE `id` = '1'"];
      case 'session_apply_changes':
        return { statements: 1, rows_affected: 1, sql: ["UPDATE `users` SET `name` = 'x' WHERE `id` = '1'"] };
      case 'session_build_select':
        return 'SELECT * FROM `users` LIMIT 50 OFFSET 0';
      case 'session_close':
        return true;
      case 'evaluate_query_safety':
        return { risk: 'safe', reason: null };
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  });
  return {
    opened: () => opened,
    calls: (cmd: string) => mockInvoke.mock.calls.filter((c) => c[0] === cmd),
  };
}

beforeEach(async () => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue(true);
  await dbStore.closeSession();
  dbStore.activeConnection = connA;
  dbStore.tables = [];
  dbStore.activeTable = null;
  dbStore.loadError = null;
  dbStore.invalidateSchema();
  mockInvoke.mockReset();
});

describe('DatabaseStore sessions', () => {
  it('runs many queries on a single backend session', async () => {
    const be = backend();
    for (let i = 0; i < 20; i++) await dbStore.executeQuery('SELECT 1');
    expect(be.opened()).toBe(1);
    expect(be.calls('session_execute')).toHaveLength(20);
  });

  it('concurrent first queries share one session_open', async () => {
    const be = backend();
    await Promise.all(Array.from({ length: 5 }, () => dbStore.executeQuery('SELECT 1')));
    expect(be.opened()).toBe(1);
  });

  it('never sends credentials with a query (only the session id)', async () => {
    const be = backend();
    await dbStore.executeQuery('SELECT 1');
    await dbStore.loadTables(true);
    for (const [, args] of [...be.calls('session_execute'), ...be.calls('session_list_tables')]) {
      expect(JSON.stringify(args)).not.toContain(SECRET);
      expect(Object.keys(args as object)).not.toContain('config');
    }
  });

  it('does not send the password or SSH secrets for a safety check either', async () => {
    const be = backend();
    await dbStore.evaluateSafety('DELETE FROM t');
    const [, args] = be.calls('evaluate_query_safety')[0];
    expect(JSON.stringify(args)).not.toContain(SECRET);
    expect((args as { config: ConnectionConfig }).config.password).toBeNull();
  });

  it('switching connection closes the previous session', async () => {
    const be = backend();
    await dbStore.executeQuery('SELECT 1');
    dbStore.setActiveConnection(connB);
    await dbStore.loadTables(true);
    expect(be.calls('session_close').map((c) => (c[1] as { sessionId: string }).sessionId)).toContain('s1');
    expect(be.opened()).toBe(2);
  });

  it('reconnects once when the backend reports the session is gone', async () => {
    const be = backend({ executeFailsOnce: "Session 's1' is not open (it may have been closed). Connect again." });
    const res = await dbStore.executeQuery('SELECT 1');
    expect(res.error).toBeFalsy();
    expect(be.opened()).toBe(2);
  });

  it('returns an error result (does not throw) when the connection cannot be opened', async () => {
    backend({ openFails: 'postgres connection failed: Connection refused' });
    const res = await dbStore.executeQuery('SELECT 1');
    expect(res.error).toContain('Connection refused');
    expect(res.rows).toEqual([]);
  });

  it('connect() rejects and stays disconnected when the session cannot be opened', async () => {
    backend({ openFails: 'mysql connection timed out after 10s' });
    await expect(dbStore.connect(connA)).rejects.toBeTruthy();
    expect(dbStore.isConnected).toBe(false);
    expect(dbStore.loadError).toContain('timed out');
  });
});

describe('DatabaseStore schema cache', () => {
  it('loads the schema once per connection, not once per page mount', async () => {
    const be = backend();
    await dbStore.loadTables();
    await dbStore.loadTables();
    await dbStore.loadTables();
    expect(be.calls('session_list_tables')).toHaveLength(1);
    expect(dbStore.activeTable).toBe('users');
  });

  it('a read keeps the cache; a write invalidates it', async () => {
    const be = backend();
    await dbStore.loadTables();
    await dbStore.executeQuery('SELECT * FROM users');
    await dbStore.loadTables();
    expect(be.calls('session_list_tables')).toHaveLength(1);

    await dbStore.executeQuery('INSERT INTO users(name) VALUES (1)');
    await dbStore.loadTables();
    expect(be.calls('session_list_tables')).toHaveLength(2);
  });

  it('force refetches', async () => {
    const be = backend();
    await dbStore.loadTables();
    await dbStore.loadTables(true);
    expect(be.calls('session_list_tables')).toHaveLength(2);
  });

  it('a failed query does not invalidate the cache', async () => {
    const be = backend();
    await dbStore.loadTables();
    mockInvoke.mockImplementationOnce(async () => ({
      columns: [], rows: [], rows_affected: 0, duration_ms: 1, error: 'syntax error',
    }));
    await dbStore.executeQuery('INSRT INTO x');
    await dbStore.loadTables();
    expect(be.calls('session_list_tables')).toHaveLength(1);
  });
});

describe('DatabaseStore browse', () => {
  it('asks the backend for a page — the UI sends a description, not SQL', async () => {
    const be = backend();
    const res = await dbStore.browseTable({ table: 'users', limit: 2, offset: 0, with_total: true });
    expect(res.total).toBe(42);
    expect(res.result.rows).toHaveLength(2);

    const [, args] = be.calls('session_browse')[0];
    const sent = args as { sessionId: string; request: { table: string } };
    expect(sent.sessionId).toBe('s1');
    expect(sent.request.table).toBe('users');
    expect(JSON.stringify(args)).not.toMatch(/select\s/i);
    expect(JSON.stringify(args)).not.toContain(SECRET);
  });

  it('never throws: a backend failure becomes an error result', async () => {
    backend({ openFails: 'mysql connection timed out after 10s' });
    const res = await dbStore.browseTable({ table: 'users' });
    expect(res.result.error).toContain('timed out');
    expect(res.total).toBeNull();
  });

  it('reconnects once when the session is gone', async () => {
    const be = backend({ browseFailsOnce: "Session 's1' is not open (it may have been closed). Connect again." });
    const res = await dbStore.browseTable({ table: 'users' });
    expect(res.result.error).toBeFalsy();
    expect(be.opened()).toBe(2);
  });

  it('buildSelect returns the backend-quoted SQL', async () => {
    backend();
    const sql = await dbStore.buildSelect({ table: 'users', limit: 50, with_total: false });
    expect(sql).toContain('`users`');
  });
});

describe('DatabaseStore row edits', () => {
  const changes = {
    table: 'users',
    schema: null,
    updates: [{ key: [{ column: 'id', value: { kind: 'text' as const, value: '1' } }], changes: [{ column: 'name', value: { kind: 'text' as const, value: 'x' } }] }],
    inserts: [],
    deletes: [],
  };

  it('previews through the backend and never sends credentials', async () => {
    const be = backend();
    const sql = await dbStore.previewChanges(changes);
    expect(sql[0]).toContain('UPDATE');
    const [, args] = be.calls('session_preview_changes')[0];
    expect((args as { sessionId: string }).sessionId).toBe('s1');
    expect(JSON.stringify(args)).not.toContain(SECRET);
  });

  it('applying invalidates the cached schema so row counts refresh', async () => {
    const be = backend();
    await dbStore.loadTables();
    await dbStore.applyChanges(changes);
    await dbStore.loadTables();
    expect(be.calls('session_list_tables')).toHaveLength(2);
  });

  it('a failed apply rejects and keeps the cache', async () => {
    const be = backend();
    await dbStore.loadTables();
    mockInvoke.mockImplementationOnce(async () => {
      throw 'Change 1 (an update) affected 0 rows instead of 1. Nothing was saved.';
    });
    await expect(dbStore.applyChanges(changes)).rejects.toContain('Nothing was saved');
    await dbStore.loadTables();
    expect(be.calls('session_list_tables')).toHaveLength(1);
  });
});

describe('DatabaseStore structure', () => {
  it('asks the backend for a table definition over the open session, without credentials', async () => {
    const be = backend();
    const structure = await dbStore.describeTable('orders');
    expect(structure.table).toBe('orders');
    expect(structure.ddl).toContain('CREATE TABLE');
    const [, args] = be.calls('session_describe_table')[0];
    expect(args).toEqual({ sessionId: 's1', table: 'orders', schema: null });
    expect(JSON.stringify(args)).not.toContain(SECRET);
  });
});
