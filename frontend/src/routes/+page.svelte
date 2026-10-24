<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n/index.svelte';
  import { dbStore, type ConnectionConfig, DEFAULT_SANDBOX_CONNECTION } from '$lib/stores/database.svelte';
  import { showToast } from '$lib/stores/uiNotifications.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { goto } from '$app/navigation';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';

  let isModalOpen = $state(false);
  let editingId = $state<string | null>(null);
  let form = $state<ConnectionConfig>({ ...DEFAULT_SANDBOX_CONNECTION, id: '' });
  let isTesting = $state(false);
  let testResult = $state<{ success: boolean; message: string } | null>(null);
  let activeSection = $state<'connections'>('connections');

  onMount(async () => {
    await dbStore.loadConnections();
  });

  const ENV_COLORS: Record<string, string> = {
    development: 'text-emerald-500 bg-emerald-500/10',
    staging: 'text-amber-500 bg-amber-500/10',
    production: 'text-rose-500 bg-rose-500/10',
  };

  const DRV_LABELS: Record<string, string> = {
    sqlite: 'SQLite',
    postgres: 'PostgreSQL',
    mysql: 'MySQL',
    mariadb: 'MariaDB',
    duckdb: 'DuckDB',
    redis: 'Redis',
  };

  function openAdd() {
    editingId = null;
    form = { ...DEFAULT_SANDBOX_CONNECTION, id: '', name: '', database: '', host: '', port: 5432, user: 'postgres', driver: 'postgres' };
    testResult = null;
    isModalOpen = true;
  }

  function editConn(c: ConnectionConfig) {
    editingId = c.id;
    form = { ...c };
    testResult = null;
    isModalOpen = true;
  }

  async function handleTest() {
    if (!form.host && form.driver !== 'sqlite' && form.driver !== 'duckdb') {
      testResult = { success: false, message: 'Host address is required for network drivers.' };
      return;
    }
    isTesting = true;
    testResult = null;
    try {
      const res = await dbStore.testConnection(form);
      testResult = { success: res.success, message: res.message + ' · ' + res.server_version };
    } catch (e: any) {
      testResult = { success: false, message: e?.message || 'Failed to establish network test.' };
    } finally {
      isTesting = false;
    }
  }

  async function handleSave() {
    if (!form.name.trim()) {
      showToast('Connection label is required.', 'error');
      return;
    }
    try {
      // Ensure id exists for upsert
      if (!form.id) {
        form.id = 'conn-' + Date.now();
      }
      await dbStore.saveConnection(form);
      showToast(`Connection "${form.name}" saved successfully.`, 'success');
      isModalOpen = false;
    } catch (e: any) {
      showToast(e?.message || 'Failed to save connection.', 'error');
    }
  }

  async function handleSaveAndConnect() {
    if (!form.name.trim()) {
      showToast('Connection label is required.', 'error');
      return;
    }
    try {
      // Ensure id exists for upsert
      if (!form.id) {
        form.id = 'conn-' + Date.now();
      }
      await dbStore.saveConnection(form);
      dbStore.setActiveConnection(form);
      isModalOpen = false;
      showToast(`Connected to "${form.name}".`, 'success');
      await goto('/table-browser');
    } catch (e: any) {
      showToast(e?.message || 'Failed to save and connect.', 'error');
    }
  }

  async function handleDelete(c: ConnectionConfig) {
    const ok = await confirm(`Delete connection "${c.name}"?`, 'Delete Connection', true, 'Delete', 'Cancel');
    if (!ok) return;
    try {
      await dbStore.deleteConnection(c.id);
      showToast(`Connection "${c.name}" deleted.`, 'success');
    } catch (e: any) {
      showToast(e?.message || 'Failed to delete connection.', 'error');
    }
  }

  function confirm(msg: string, title: string, confirm: boolean, yesLabel: string, noLabel: string): Promise<boolean> {
    return new Promise((resolve) => {
      const result = window.confirm(`${msg}`);
      resolve(result);
    });
  }
</script>

<div class="flex flex-col h-full overflow-y-auto px-6 py-5 space-y-4 max-w-7xl mx-auto w-full">
  <!-- Page Header -->
  <div class="flex items-center justify-between">
    <div class="flex items-center gap-3">
      <div class="w-8 h-8 rounded-lg bg-blue-100 dark:bg-blue-950/60 border border-blue-200/80 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 font-mono font-bold text-xs shadow-2xs">
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 00-2 2v5a2 2 0 002 2h14a2 2 0 002-2v-5a2 2 0 00-2-2M5 12a2 2 0 01-2-2V5a2 2 0 012-2h14a2 2 0 012 2v5a2 2 0 01-2 2M5 12v5" />
        </svg>
      </div>
      <div>
        <h1 class="text-lg font-bold tracking-tight text-neutral-900 dark:text-neutral-100 leading-tight">Database Connections</h1>
        <p class="text-[11px] text-neutral-500 dark:text-neutral-400">{dbStore.connections.length} connections saved in encrypted vault</p>
      </div>
    </div>

    <button
      onclick={openAdd}
      class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-colors cursor-pointer"
    >
      <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M12 4v16m8-8H4" />
      </svg>
      <span>New Connection</span>
    </button>
  </div>

  <!-- Connections List -->
  {#if dbStore.connections.length === 0}
    <EmptyState
      title="No Saved Connections"
      description="Create a new database connection to start managing and querying data."
      icon="connections"
      actionLabel="New Connection"
      onaction={openAdd}
    />
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4 pt-1">
      {#each dbStore.connections as conn (conn.id)}
        <div
          class="group relative flex flex-col justify-between p-4 bg-white dark:bg-neutral-900/90 border border-neutral-200/90 dark:border-neutral-800 rounded-2xl shadow-2xs hover:shadow-md transition-all duration-150 space-y-3 cursor-pointer"
          onclick={() => {
            dbStore.setActiveConnection(conn);
            void goto('/table-browser');
          }}
        >
          <div class="flex items-start justify-between gap-2">
            <div class="flex items-center gap-3 min-w-0">
              <div class="w-9 h-9 rounded-lg bg-blue-500/10 text-blue-500 flex items-center justify-center shrink-0">
                <svg class="w-4.5 h-4.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4m0 10c0 2.21-3.582 4-8 4s-8-1.79-8-4" />
                </svg>
              </div>

              <div class="min-w-0">
                <div class="flex items-center gap-2">
                  <h3 class="font-bold text-sm text-neutral-900 dark:text-neutral-100 truncate">{conn.name}</h3>
                  {#if conn.id === dbStore.activeConnection.id}
                    <span class="text-[9px] font-bold bg-blue-500 text-white rounded px-1.5 py-0.5 tracking-wider">ACTIVE</span>
                  {/if}
                </div>
                <div class="flex items-center gap-2 mt-1 text-[10px] font-mono text-neutral-500 dark:text-neutral-400">
                  <span class="uppercase bg-neutral-100 dark:bg-neutral-800 px-1.5 py-0.5 rounded text-neutral-700 dark:text-neutral-300">{DRV_LABELS[conn.driver]}</span>
                  <span class={`px-1.5 py-0.5 rounded font-semibold uppercase tracking-wide ${ENV_COLORS[conn.environment]}`}>{conn.environment}</span>
                </div>
              </div>
            </div>

            <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
              <button
                onclick={(e) => {
                  e.stopPropagation();
                  dbStore.setActiveConnection(conn);
                  void goto('/table-browser');
                }}
                class="p-1.5 rounded hover:bg-blue-50 dark:hover:bg-blue-900/30 text-neutral-400 hover:text-blue-500"
                title="Manage"
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M4 10h16M4 14h16M9 6v12M15 6v12" /></svg>
              </button>
              <button
                onclick={(e) => { e.stopPropagation(); editConn(conn); }}
                class="p-1.5 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-400"
                title="Edit"
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" /></svg>
              </button>
              <button
                onclick={(e) => { e.stopPropagation(); handleDelete(conn); }}
                class="p-1.5 rounded hover:bg-rose-50 dark:hover:bg-rose-900/30 text-neutral-400 hover:text-rose-500"
                title="Delete"
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" /></svg>
              </button>
            </div>
          </div>

          <div class="pt-2 border-t border-neutral-100 dark:border-neutral-800/80 text-[10px] font-mono text-neutral-500 dark:text-neutral-400 flex justify-between items-center">
            <span class="truncate">
              {conn.driver === 'sqlite' ? conn.database : `${conn.user}@${conn.host}:${conn.port}/${conn.database}`}
            </span>
            {#if conn.read_only}
              <span class="text-amber-500 font-semibold flex items-center gap-1">
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" /></svg>
                Read-Only
              </span>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<!-- Add/Edit Connection Modal -->
{#if isModalOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-xs p-4">
    <div class="w-full max-w-2xl bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-2xl p-5 shadow-2xl space-y-4 text-xs text-neutral-800 dark:text-neutral-200 max-h-[90vh] overflow-y-auto">
      <div class="flex items-center justify-between pb-2 border-b border-neutral-200 dark:border-neutral-800">
        <h3 class="text-sm font-bold text-neutral-900 dark:text-white">
          {editingId ? 'Edit Database Connection' : 'New Database Connection'}
        </h3>
        <button onclick={() => (isModalOpen = false)} class="p-1 rounded text-neutral-400 hover:text-neutral-700 dark:hover:text-white">
          <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" /></svg>
        </button>
      </div>

      <div class="grid grid-cols-2 gap-4">
        <div class="space-y-1">
          <label class="font-semibold text-neutral-500 uppercase text-[10px]">Driver Type</label>
          <select bind:value={form.driver} class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500">
            <option value="sqlite">SQLite / SQLCipher</option>
            <option value="postgres">PostgreSQL</option>
            <option value="mysql">MySQL</option>
            <option value="mariadb">MariaDB</option>
            <option value="duckdb">DuckDB</option>
            <option value="redis">Redis / Valkey</option>
          </select>
        </div>

        <div class="space-y-1">
          <label class="font-semibold text-neutral-500 uppercase text-[10px]">Environment</label>
          <select bind:value={form.environment} class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500">
            <option value="development">Development</option>
            <option value="staging">Staging</option>
            <option value="production">Production (Read-Only)</option>
          </select>
          {#if form.environment === 'production'}
            <div class="text-[10px] text-rose-500 font-medium flex items-center gap-1 mt-1">
              <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M18.364 18.364A9 9 0 005.636 5.636m12.728 12.728A9 9 0 015.636 5.636m12.728 12.728L5.636 5.636" /></svg>
              Production connections are automatically forced to READ-ONLY mode.
            </div>
          {/if}
        </div>
      </div>

      <div class="space-y-1">
        <label class="font-semibold text-neutral-500 uppercase text-[10px]">Connection Label</label>
        <input bind:value={form.name} placeholder="My Postgres Database" class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500" />
      </div>

      <div class="grid grid-cols-2 gap-4">
        <div class="space-y-1">
          <label class="font-semibold text-neutral-500 uppercase text-[10px]">
            {form.driver === 'sqlite' ? 'File Path' : 'Host / Address'}
          </label>
          <input bind:value={form.host} placeholder={form.driver === 'sqlite' ? '/path/to/db.sqlite' : '127.0.0.1'} class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500" />
        </div>

        <div class="space-y-1">
          <label class="font-semibold text-neutral-500 uppercase text-[10px]">
            {form.driver === 'sqlite' ? 'Database Name' : 'Port'}
          </label>
          {#if form.driver === 'sqlite'}
            <input bind:value={form.database} placeholder="app.sqlite" class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500" />
          {:else}
            <input type="number" bind:value={form.port} class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500" />
          {/if}
        </div>

        {#if form.driver !== 'sqlite'}
          <div class="space-y-1">
            <label class="font-semibold text-neutral-500 uppercase text-[10px]">Database Name</label>
            <input bind:value={form.database} placeholder="production_db" class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500" />
          </div>

          <div class="space-y-1">
            <label class="font-semibold text-neutral-500 uppercase text-[10px]">Username</label>
            <input bind:value={form.user} placeholder="postgres" class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500" />
          </div>

          <div class="space-y-1">
            <label class="font-semibold text-neutral-500 uppercase text-[10px]">Password (encrypted in vault)</label>
            <input type="password" bind:value={form.password} placeholder="••••••••" class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500" />
          </div>
        {/if}
      </div>

      {#if testResult}
        <div class={`p-3 rounded-lg text-xs font-medium border ${testResult.success ? 'bg-emerald-50 dark:bg-emerald-900/30 border-emerald-200 dark:border-emerald-800 text-emerald-700 dark:text-emerald-400' : 'bg-rose-50 dark:bg-rose-900/30 border-rose-200 dark:border-rose-800 text-rose-700 dark:text-rose-400'}`}>
          <div class="font-bold mb-0.5">{testResult.success ? 'Success' : 'Error'}</div>
          {testResult.message}
        </div>
      {/if}

      <div class="flex items-center justify-between pt-3 border-t border-neutral-200 dark:border-neutral-800">
        <button
          onclick={handleTest}
          disabled={isTesting}
          class="px-3 py-1.5 text-xs font-medium text-neutral-700 dark:text-neutral-200 border border-neutral-300 dark:border-neutral-700 rounded-lg transition-colors disabled:opacity-50 hover:bg-neutral-50 dark:hover:bg-neutral-800"
        >
          {isTesting ? 'Testing…' : 'Test Connection'}
        </button>
        <div class="flex items-center gap-2">
          <button onclick={() => (isModalOpen = false)} class="px-3.5 py-1.5 text-xs font-medium text-neutral-500 hover:text-neutral-800 dark:hover:text-white rounded-lg transition-colors">Cancel</button>
          <button
            onclick={handleSave}
            class="px-3.5 py-1.5 text-xs font-medium text-neutral-700 dark:text-neutral-200 border border-neutral-300 dark:border-neutral-700 rounded-lg transition-colors hover:bg-neutral-50 dark:hover:bg-neutral-800"
          >
            {editingId ? 'Update Connection' : 'Save Connection'}
          </button>
          <button
            onclick={handleSaveAndConnect}
            class="px-4 py-1.5 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-colors"
          >
            Save & Connect
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}
