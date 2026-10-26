<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n/index.svelte';
  import {
    dbStore,
    type ConnectionConfig,
    type DriverKind,
    type SshTunnelConfig,
    DEFAULT_SANDBOX_CONNECTION,
  } from '$lib/stores/database.svelte';
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

  // SSH Tunnel states
  let isSshAccordionOpen = $state(false);
  let showSshPassphrase = $state(false);
  let showSshPassword = $state(false);
  let isTestingSsh = $state(false);
  let sshTestResult = $state<{ success: boolean; message: string; latency_ms?: number } | null>(null);

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

  const DRIVER_DEFAULTS: Record<DriverKind, { port: number; user: string; database: string }> = {
    postgres: { port: 5432, user: 'postgres', database: 'postgres' },
    mysql: { port: 3306, user: 'root', database: 'app_db' },
    mariadb: { port: 3306, user: 'root', database: 'app_db' },
    redis: { port: 6379, user: 'default', database: '0' },
    sqlite: { port: 0, user: '', database: 'sandbox.db' },
    duckdb: { port: 0, user: '', database: 'app.duckdb' },
  };

  function isFileDriver(driver: DriverKind): boolean {
    return driver === 'sqlite' || driver === 'duckdb';
  }

  function initSshTunnel(): SshTunnelConfig {
    return {
      enabled: false,
      host: '',
      port: 22,
      user: '',
      auth_type: 'key',
      key_path: '~/.ssh/id_ed25519',
      key_passphrase: '',
      password: '',
    };
  }

  function openAdd() {
    editingId = null;
    form = {
      ...DEFAULT_SANDBOX_CONNECTION,
      id: '',
      name: '',
      driver: 'postgres',
      port: 5432,
      user: 'postgres',
      database: 'postgres',
      host: '127.0.0.1',
      password: '',
      read_only: false,
      ssh_tunnel: initSshTunnel(),
    };
    testResult = null;
    sshTestResult = null;
    isSshAccordionOpen = false;
    showSshPassphrase = false;
    showSshPassword = false;
    isModalOpen = true;
  }

  function editConn(c: ConnectionConfig) {
    editingId = c.id;
    form = {
      ...c,
      ssh_tunnel: c.ssh_tunnel ? { ...c.ssh_tunnel } : initSshTunnel(),
    };
    testResult = null;
    sshTestResult = null;
    isSshAccordionOpen = !!c.ssh_tunnel?.enabled;
    showSshPassphrase = false;
    showSshPassword = false;
    isModalOpen = true;
  }

  function handleDriverChange(e: Event) {
    const driver = (e.currentTarget as HTMLSelectElement).value as DriverKind;
    form.driver = driver;
    const def = DRIVER_DEFAULTS[driver];
    if (def) {
      form.port = def.port;
      form.user = def.user;
      form.database = def.database;
      if (isFileDriver(driver)) {
        form.host = 'localhost';
        form.password = '';
        if (form.ssh_tunnel) {
          form.ssh_tunnel.enabled = false;
        }
      } else if (!form.host || form.host === 'localhost') {
        form.host = '127.0.0.1';
      }
    }
  }

  function toggleSshTunnel() {
    if (!form.ssh_tunnel) {
      form.ssh_tunnel = initSshTunnel();
    }
    form.ssh_tunnel.enabled = !form.ssh_tunnel.enabled;
    if (form.ssh_tunnel.enabled) {
      isSshAccordionOpen = true;
      if (!form.ssh_tunnel.port) {
        form.ssh_tunnel.port = 22;
      }
    }
  }

  async function handleTestSsh() {
    if (!form.ssh_tunnel?.host?.trim()) {
      sshTestResult = { success: false, message: 'SSH Host / address is required.' };
      return;
    }
    if (!form.ssh_tunnel?.user?.trim()) {
      sshTestResult = { success: false, message: 'SSH Username is required.' };
      return;
    }
    isTestingSsh = true;
    sshTestResult = null;
    try {
      const res = await dbStore.testSshTunnel(form.ssh_tunnel);
      sshTestResult = {
        success: res.success,
        message: res.message || (res.success ? 'SSH tunnel connection established successfully.' : 'SSH handshake failed.'),
        latency_ms: res.latency_ms,
      };
    } catch (e: any) {
      sshTestResult = {
        success: false,
        message: e?.message || 'Failed to establish SSH handshake.',
      };
    } finally {
      isTestingSsh = false;
    }
  }

  async function handleTest() {
    if (!form.host && !isFileDriver(form.driver)) {
      testResult = { success: false, message: 'Host address is required for network drivers.' };
      return;
    }
    isTesting = true;
    testResult = null;
    try {
      const res = await dbStore.testConnection(form);
      testResult = { success: res.success, message: res.message + (res.server_version ? ' · ' + res.server_version : '') };
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
      if (!form.id) {
        form.id = 'conn-' + Date.now();
      }
      if (isFileDriver(form.driver)) {
        form.ssh_tunnel = null;
        form.port = 0;
        form.user = '';
        form.password = null;
        form.host = 'localhost';
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
      if (!form.id) {
        form.id = 'conn-' + Date.now();
      }
      if (isFileDriver(form.driver)) {
        form.ssh_tunnel = null;
        form.port = 0;
        form.user = '';
        form.password = null;
        form.host = 'localhost';
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
          role="button"
          tabindex="0"
          class="group relative flex flex-col justify-between p-4 bg-white dark:bg-neutral-900/90 border border-neutral-200/90 dark:border-neutral-800 rounded-2xl shadow-2xs hover:shadow-md transition-all duration-150 space-y-3 cursor-pointer text-left"
          onclick={() => {
            dbStore.setActiveConnection(conn);
            void goto('/table-browser');
          }}
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              dbStore.setActiveConnection(conn);
              void goto('/table-browser');
            }
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
                <div class="flex items-center gap-2 mt-1 text-[10px] font-mono text-neutral-500 dark:text-neutral-400 flex-wrap">
                  <span class="uppercase bg-neutral-100 dark:bg-neutral-800 px-1.5 py-0.5 rounded text-neutral-700 dark:text-neutral-300">{DRV_LABELS[conn.driver]}</span>
                  {#if conn.ssh_tunnel?.enabled}
                    <span class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded font-bold uppercase tracking-wider text-[9px] bg-sky-500/10 text-sky-600 dark:text-sky-400 border border-sky-500/20 shadow-2xs" title={`SSH Tunnel: ${conn.ssh_tunnel.user}@${conn.ssh_tunnel.host}:${conn.ssh_tunnel.port || 22}`}>
                      <svg class="w-2.5 h-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
                      </svg>
                      SSH
                    </span>
                  {/if}
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
                aria-label="Manage connection"
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M4 10h16M4 14h16M9 6v12M15 6v12" /></svg>
              </button>
              <button
                onclick={(e) => { e.stopPropagation(); editConn(conn); }}
                class="p-1.5 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-400"
                title="Edit"
                aria-label="Edit connection"
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" /></svg>
              </button>
              <button
                onclick={(e) => { e.stopPropagation(); handleDelete(conn); }}
                class="p-1.5 rounded hover:bg-rose-50 dark:hover:bg-rose-900/30 text-neutral-400 hover:text-rose-500"
                title="Delete"
                aria-label="Delete connection"
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" /></svg>
              </button>
            </div>
          </div>

          <div class="pt-2 border-t border-neutral-100 dark:border-neutral-800/80 text-[10px] font-mono text-neutral-500 dark:text-neutral-400 flex justify-between items-center">
            <span class="truncate">
              {isFileDriver(conn.driver) ? conn.database : `${conn.user}@${conn.host}:${conn.port}/${conn.database}`}
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
        <button
          onclick={() => (isModalOpen = false)}
          aria-label="Close modal"
          class="p-1 rounded text-neutral-400 hover:text-neutral-700 dark:hover:text-white cursor-pointer"
        >
          <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" /></svg>
        </button>
      </div>

      <!-- Driver Type & Environment -->
      <div class="grid grid-cols-2 gap-4">
        <div class="space-y-1">
          <label for="modal-driver" class="font-semibold text-neutral-500 uppercase text-[10px]">Driver Type</label>
          <select
            id="modal-driver"
            value={form.driver}
            onchange={handleDriverChange}
            class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500"
          >
            <option value="sqlite">SQLite / SQLCipher</option>
            <option value="postgres">PostgreSQL</option>
            <option value="mysql">MySQL</option>
            <option value="mariadb">MariaDB</option>
            <option value="duckdb">DuckDB</option>
            <option value="redis">Redis / Valkey</option>
          </select>
        </div>

        <div class="space-y-1">
          <label for="modal-env" class="font-semibold text-neutral-500 uppercase text-[10px]">Environment</label>
          <select
            id="modal-env"
            bind:value={form.environment}
            class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500"
          >
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

      <!-- Connection Label -->
      <div class="space-y-1">
        <label for="modal-conn-name" class="font-semibold text-neutral-500 uppercase text-[10px]">Connection Label</label>
        <input
          id="modal-conn-name"
          bind:value={form.name}
          placeholder={isFileDriver(form.driver) ? 'Local Database File' : 'My Database Connection'}
          class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500"
        />
      </div>

      <!-- Driver Specific Fields -->
      {#if isFileDriver(form.driver)}
        <div class="space-y-1">
          <label for="modal-file-path" class="font-semibold text-neutral-500 uppercase text-[10px]">File Path</label>
          <input
            id="modal-file-path"
            bind:value={form.database}
            placeholder={form.driver === 'sqlite' ? 'sandbox.db or /path/to/db.sqlite' : 'app.duckdb or /path/to/data.duckdb'}
            class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 font-mono text-xs"
          />
          <p class="text-[10px] text-neutral-500">File-based database. Port, username, and password credentials are not required.</p>
        </div>
      {:else}
        <div class="grid grid-cols-2 gap-4">
          <div class="space-y-1">
            <label for="modal-host" class="font-semibold text-neutral-500 uppercase text-[10px]">Host / Address</label>
            <input
              id="modal-host"
              bind:value={form.host}
              placeholder="127.0.0.1"
              class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
          </div>

          <div class="space-y-1">
            <label for="modal-port" class="font-semibold text-neutral-500 uppercase text-[10px]">Port</label>
            <input
              id="modal-port"
              type="number"
              bind:value={form.port}
              class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
          </div>

          <div class="space-y-1">
            <label for="modal-db-name" class="font-semibold text-neutral-500 uppercase text-[10px]">Database Name</label>
            <input
              id="modal-db-name"
              bind:value={form.database}
              placeholder="production_db"
              class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
          </div>

          <div class="space-y-1">
            <label for="modal-user" class="font-semibold text-neutral-500 uppercase text-[10px]">Username</label>
            <input
              id="modal-user"
              bind:value={form.user}
              placeholder="postgres"
              class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
          </div>

          <div class="space-y-1 col-span-2">
            <label for="modal-pass" class="font-semibold text-neutral-500 uppercase text-[10px]">Password (encrypted in vault)</label>
            <input
              id="modal-pass"
              type="password"
              bind:value={form.password}
              placeholder="••••••••"
              class="w-full px-2.5 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500"
            />
          </div>
        </div>

        <!-- SSH Tunnel Accordion Section -->
        <div class="border border-neutral-200 dark:border-neutral-800 rounded-xl overflow-hidden bg-neutral-50/50 dark:bg-neutral-900/50">
          <div
            role="button"
            tabindex="0"
            class="flex items-center justify-between px-3.5 py-2.5 cursor-pointer select-none hover:bg-neutral-100/50 dark:hover:bg-neutral-800/40 transition-colors"
            onclick={() => (isSshAccordionOpen = !isSshAccordionOpen)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                isSshAccordionOpen = !isSshAccordionOpen;
              }
            }}
          >
            <div class="flex items-center gap-2">
              <svg
                class={`w-3.5 h-3.5 text-neutral-400 transition-transform duration-200 ${isSshAccordionOpen ? 'rotate-90' : ''}`}
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
              </svg>
              <div class="flex items-center gap-2">
                <svg class="w-4 h-4 text-blue-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
                </svg>
                <span class="font-semibold text-neutral-800 dark:text-neutral-200 text-xs">SSH Tunnel (Bastion Host)</span>
                {#if form.ssh_tunnel?.enabled}
                  <span class="px-1.5 py-0.5 rounded font-bold uppercase tracking-wider text-[9px] bg-sky-500/10 text-sky-600 dark:text-sky-400 border border-sky-500/20">ACTIVE</span>
                {/if}
              </div>
            </div>

            <div
              class="flex items-center gap-2"
              role="none"
              onclick={(e) => e.stopPropagation()}
              onkeydown={(e) => e.stopPropagation()}
            >
              <span class="text-[10px] text-neutral-500 font-medium">{form.ssh_tunnel?.enabled ? 'Enabled' : 'Disabled'}</span>
              <button
                type="button"
                role="switch"
                aria-label="Toggle SSH Tunnel"
                aria-checked={form.ssh_tunnel?.enabled ?? false}
                onclick={toggleSshTunnel}
                class="relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none {form.ssh_tunnel?.enabled ? 'bg-blue-600' : 'bg-neutral-300 dark:bg-neutral-700'}"
              >
                <span
                  class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition duration-200 ease-in-out {form.ssh_tunnel?.enabled ? 'translate-x-4' : 'translate-x-0'}"
                ></span>
              </button>
            </div>
          </div>

          {#if isSshAccordionOpen}
            <div class="p-3.5 border-t border-neutral-200/80 dark:border-neutral-800/80 space-y-3 bg-white/50 dark:bg-neutral-900/50">
              {#if !form.ssh_tunnel?.enabled}
                <div class="text-[11px] text-neutral-500 dark:text-neutral-400 flex items-center justify-between">
                  <span>Enable SSH tunneling to route database traffic through an encrypted bastion host.</span>
                  <button
                    type="button"
                    onclick={toggleSshTunnel}
                    class="text-xs text-blue-600 dark:text-blue-400 font-semibold hover:underline cursor-pointer"
                  >
                    Enable Tunnel
                  </button>
                </div>
              {:else}
                <div class="grid grid-cols-2 gap-3">
                  <div class="space-y-1">
                    <label for="ssh-host" class="font-semibold text-neutral-500 uppercase text-[10px]">SSH Host</label>
                    <input
                      id="ssh-host"
                      bind:value={form.ssh_tunnel.host}
                      placeholder="bastion.example.com or 192.168.1.50"
                      class="w-full px-2.5 py-1.5 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 text-xs font-mono"
                    />
                  </div>

                  <div class="space-y-1">
                    <label for="ssh-port" class="font-semibold text-neutral-500 uppercase text-[10px]">SSH Port</label>
                    <input
                      id="ssh-port"
                      type="number"
                      bind:value={form.ssh_tunnel.port}
                      placeholder="22"
                      class="w-full px-2.5 py-1.5 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 text-xs"
                    />
                  </div>

                  <div class="space-y-1">
                    <label for="ssh-user" class="font-semibold text-neutral-500 uppercase text-[10px]">SSH User</label>
                    <input
                      id="ssh-user"
                      bind:value={form.ssh_tunnel.user}
                      placeholder="ubuntu / ec2-user"
                      class="w-full px-2.5 py-1.5 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 text-xs font-mono"
                    />
                  </div>

                  <div class="space-y-1">
                    <label for="ssh-auth-type" class="font-semibold text-neutral-500 uppercase text-[10px]">Auth Type</label>
                    <select
                      id="ssh-auth-type"
                      bind:value={form.ssh_tunnel.auth_type}
                      class="w-full px-2.5 py-1.5 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 text-xs"
                    >
                      <option value="key">Private Key</option>
                      <option value="password">Password</option>
                      <option value="agent">SSH Agent</option>
                    </select>
                  </div>

                  {#if form.ssh_tunnel.auth_type === 'key'}
                    <div class="space-y-1 col-span-2 sm:col-span-1">
                      <label for="ssh-key-path" class="font-semibold text-neutral-500 uppercase text-[10px]">Key Path</label>
                      <input
                        id="ssh-key-path"
                        bind:value={form.ssh_tunnel.key_path}
                        placeholder="~/.ssh/id_ed25519"
                        class="w-full px-2.5 py-1.5 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 text-xs font-mono"
                      />
                    </div>

                    <div class="space-y-1 col-span-2 sm:col-span-1">
                      <label for="ssh-passphrase" class="font-semibold text-neutral-500 uppercase text-[10px]">Passphrase (optional)</label>
                      <div class="relative">
                        <input
                          id="ssh-passphrase"
                          type={showSshPassphrase ? 'text' : 'password'}
                          bind:value={form.ssh_tunnel.key_passphrase}
                          placeholder="Passphrase if encrypted"
                          class="w-full pl-2.5 pr-8 py-1.5 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 text-xs"
                        />
                        <button
                          type="button"
                          aria-label={showSshPassphrase ? 'Hide passphrase' : 'Show passphrase'}
                          onclick={() => (showSshPassphrase = !showSshPassphrase)}
                          class="absolute right-2 top-1/2 -translate-y-1/2 text-neutral-400 hover:text-neutral-700 dark:hover:text-white p-0.5 cursor-pointer"
                        >
                          {#if showSshPassphrase}
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21" /></svg>
                          {:else}
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" /><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" /></svg>
                          {/if}
                        </button>
                      </div>
                    </div>
                  {:else if form.ssh_tunnel.auth_type === 'password'}
                    <div class="space-y-1 col-span-2">
                      <label for="ssh-password" class="font-semibold text-neutral-500 uppercase text-[10px]">Password</label>
                      <div class="relative">
                        <input
                          id="ssh-password"
                          type={showSshPassword ? 'text' : 'password'}
                          bind:value={form.ssh_tunnel.password}
                          placeholder="SSH user password"
                          class="w-full pl-2.5 pr-8 py-1.5 bg-neutral-50 dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 text-xs"
                        />
                        <button
                          type="button"
                          aria-label={showSshPassword ? 'Hide password' : 'Show password'}
                          onclick={() => (showSshPassword = !showSshPassword)}
                          class="absolute right-2 top-1/2 -translate-y-1/2 text-neutral-400 hover:text-neutral-700 dark:hover:text-white p-0.5 cursor-pointer"
                        >
                          {#if showSshPassword}
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l3.59 3.59m0 0A9.953 9.953 0 0112 5c4.478 0 8.268 2.943 9.543 7a10.025 10.025 0 01-4.132 5.411m0 0L21 21" /></svg>
                          {:else}
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" /><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" /></svg>
                          {/if}
                        </button>
                      </div>
                    </div>
                  {:else}
                    <div class="col-span-2 p-2 rounded-lg bg-blue-500/10 border border-blue-500/20 text-[11px] text-blue-600 dark:text-blue-400">
                      SSH Agent authentication will query keys from your running SSH agent (via <code>SSH_AUTH_SOCK</code>).
                    </div>
                  {/if}
                </div>

                <div class="pt-2 flex items-center justify-between">
                  <button
                    type="button"
                    onclick={handleTestSsh}
                    disabled={isTestingSsh}
                    class="px-2.5 py-1 text-xs font-medium text-neutral-700 dark:text-neutral-200 border border-neutral-300 dark:border-neutral-700 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors disabled:opacity-50 flex items-center gap-1.5 cursor-pointer"
                  >
                    {#if isTestingSsh}
                      <svg class="w-3 h-3 animate-spin text-blue-500" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path></svg>
                      <span>Testing Handshake…</span>
                    {:else}
                      <svg class="w-3 h-3 text-neutral-500" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z" /></svg>
                      <span>Test SSH Handshake</span>
                    {/if}
                  </button>
                </div>

                {#if sshTestResult}
                  <div class={`p-2.5 rounded-lg text-xs font-medium border ${sshTestResult.success ? 'bg-emerald-50 dark:bg-emerald-900/30 border-emerald-200 dark:border-emerald-800 text-emerald-700 dark:text-emerald-400' : 'bg-rose-50 dark:bg-rose-900/30 border-rose-200 dark:border-rose-800 text-rose-700 dark:text-rose-400'}`}>
                    <div class="font-bold flex items-center gap-1">
                      <span>{sshTestResult.success ? '✓ SSH Handshake Succeeded' : '✗ SSH Handshake Failed'}</span>
                      {#if sshTestResult.latency_ms !== undefined}
                        <span class="font-mono text-[10px] opacity-75">({sshTestResult.latency_ms}ms)</span>
                      {/if}
                    </div>
                    <p class="mt-0.5 text-[11px]">{sshTestResult.message}</p>
                  </div>
                {/if}
              {/if}
            </div>
          {/if}
        </div>
      {/if}

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
          class="px-3 py-1.5 text-xs font-medium text-neutral-700 dark:text-neutral-200 border border-neutral-300 dark:border-neutral-700 rounded-lg transition-colors disabled:opacity-50 hover:bg-neutral-50 dark:hover:bg-neutral-800 cursor-pointer"
        >
          {isTesting ? 'Testing…' : 'Test Connection'}
        </button>
        <div class="flex items-center gap-2">
          <button onclick={() => (isModalOpen = false)} class="px-3.5 py-1.5 text-xs font-medium text-neutral-500 hover:text-neutral-800 dark:hover:text-white rounded-lg transition-colors cursor-pointer">Cancel</button>
          <button
            onclick={handleSave}
            class="px-3.5 py-1.5 text-xs font-medium text-neutral-700 dark:text-neutral-200 border border-neutral-300 dark:border-neutral-700 rounded-lg transition-colors hover:bg-neutral-50 dark:hover:bg-neutral-800 cursor-pointer"
          >
            {editingId ? 'Update Connection' : 'Save Connection'}
          </button>
          <button
            onclick={handleSaveAndConnect}
            class="px-4 py-1.5 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-colors cursor-pointer"
          >
            Save & Connect
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}