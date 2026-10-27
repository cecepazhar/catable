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

<div class="flex flex-col h-full overflow-y-auto p-6 md:p-8 max-w-5xl mx-auto w-full">
  
  <!-- Connection Header (Antares SQL Style) -->
  <div class="flex items-center justify-between pb-4 border-b border-neutral-200 dark:border-neutral-800 shrink-0 mb-6">
    <div class="flex items-center gap-3">
      <div class="w-10 h-10 rounded-xl bg-blue-100 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 font-mono font-bold text-sm shadow-xs">
        {form.driver.slice(0, 2).toUpperCase()}
      </div>
      <div>
        <div class="flex items-center gap-2">
          <input
            type="text"
            bind:value={form.name}
            placeholder="Connection Name"
            class="text-base font-bold bg-transparent border-b border-transparent hover:border-neutral-300 dark:hover:border-neutral-700 focus:border-blue-500 focus:outline-none text-neutral-900 dark:text-white px-1 py-0.5 rounded transition-colors"
          />
          <span class="text-[10px] uppercase font-mono px-2 py-0.5 rounded border {ENV_COLORS[form.environment]}">
            {form.environment}
          </span>
          {#if form.ssh_tunnel?.enabled}
            <span class="text-[10px] font-mono px-2 py-0.5 rounded border border-sky-500/30 text-sky-500 bg-sky-500/10 font-bold">
              SSH TUNNEL
            </span>
          {/if}
        </div>
        <p class="text-xs text-neutral-500 dark:text-neutral-400 pl-1 mt-0.5">
          {DRV_LABELS[form.driver]} · {isFileDriver(form.driver) ? form.database : `${form.user}@${form.host}:${form.port}`}
        </p>
      </div>
    </div>

    <div class="flex items-center gap-2">
      <button
        type="button"
        onclick={openAdd}
        class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-colors cursor-pointer"
      >
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M12 4v16m8-8H4" />
        </svg>
        <span>New Connection</span>
      </button>
      {#if form.id}
        <button
          type="button"
          onclick={handleDelete}
          class="text-xs text-rose-500 hover:text-rose-600 dark:hover:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/40 px-3 py-1.5 rounded-lg transition-colors cursor-pointer border border-rose-200 dark:border-rose-900/50"
          title="Delete Connection"
        >
          Delete
        </button>
      {/if}
    </div>
  </div>

  <!-- Form Tabs: General, SSH Tunnel, Advanced -->
  <div class="flex border-b border-neutral-200 dark:border-neutral-800 mb-6 gap-2">
    <button
      type="button"
      onclick={() => (activeTab = 'general')}
      class="px-4 py-2 text-xs font-semibold border-b-2 transition-colors cursor-pointer {activeTab === 'general' ? 'border-blue-500 text-blue-600 dark:text-blue-400' : 'border-transparent text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200'}"
    >
      General
    </button>
    <button
      type="button"
      onclick={() => (activeTab = 'ssh')}
      class="px-4 py-2 text-xs font-semibold border-b-2 transition-colors cursor-pointer flex items-center gap-1.5 {activeTab === 'ssh' ? 'border-blue-500 text-blue-600 dark:text-blue-400' : 'border-transparent text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200'}"
    >
      <span>SSH Tunnel</span>
      {#if form.ssh_tunnel?.enabled}
        <span class="w-1.5 h-1.5 rounded-full bg-sky-500"></span>
      {/if}
    </button>
    <button
      type="button"
      onclick={() => (activeTab = 'advanced')}
      class="px-4 py-2 text-xs font-semibold border-b-2 transition-colors cursor-pointer {activeTab === 'advanced' ? 'border-blue-500 text-blue-600 dark:text-blue-400' : 'border-transparent text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200'}"
    >
      Advanced & Safety
    </button>
  </div>

  <!-- Tab Contents -->
  <div class="flex-1 space-y-6">
    
    {#if activeTab === 'general'}
      <!-- GENERAL TAB -->
      <div class="grid grid-cols-1 md:grid-cols-2 gap-5">
        <div class="space-y-1.5">
          <label for="driver-select" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
            Driver Type
          </label>
          <select
            id="driver-select"
            value={form.driver}
            onchange={handleDriverChange}
            class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-medium focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
          >
            <option value="postgres">PostgreSQL</option>
            <option value="mysql">MySQL</option>
            <option value="mariadb">MariaDB</option>
            <option value="sqlite">SQLite / SQLCipher</option>
            <option value="duckdb">DuckDB</option>
            <option value="redis">Redis / Valkey</option>
          </select>
        </div>

        <div class="space-y-1.5">
          <label for="env-select" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
            Environment
          </label>
          <select
            id="env-select"
            bind:value={form.environment}
            class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-medium focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
          >
            <option value="development">Development</option>
            <option value="staging">Staging</option>
            <option value="production">Production (Auto Read-Only)</option>
          </select>
        </div>

        {#if !isFileDriver(form.driver)}
          <div class="space-y-1.5">
            <label for="host-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
              Host / IP Address
            </label>
            <input
              id="host-input"
              type="text"
              bind:value={form.host}
              placeholder="127.0.0.1"
              class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono text-neutral-900 dark:text-white"
            />
          </div>

          <div class="space-y-1.5">
            <label for="port-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
              Port (Auto Default)
            </label>
            <input
              id="port-input"
              type="number"
              bind:value={form.port}
              class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono text-neutral-900 dark:text-white"
            />
          </div>

          <div class="space-y-1.5">
            <label for="db-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
              Database Name
            </label>
            <input
              id="db-input"
              type="text"
              bind:value={form.database}
              placeholder="production_db"
              class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
            />
          </div>

          <div class="space-y-1.5">
            <label for="user-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
              Username
            </label>
            <input
              id="user-input"
              type="text"
              bind:value={form.user}
              placeholder="root"
              class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
            />
          </div>

          <div class="space-y-1.5 md:col-span-2">
            <label for="pass-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
              Password (Encrypted in Vault)
            </label>
            <div class="relative">
              <input
                id="pass-input"
                type={showDbPassword ? 'text' : 'password'}
                bind:value={form.password}
                placeholder="••••••••"
                class="w-full pl-3 pr-10 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono text-neutral-900 dark:text-white"
              />
              <button
                type="button"
                onclick={() => (showDbPassword = !showDbPassword)}
                class="absolute right-2.5 top-2.5 text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200 cursor-pointer"
                aria-label={showDbPassword ? 'Hide password' : 'Show password'}
              >
                {#if showDbPassword}
                  <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l18 18" /></svg>
                {:else}
                  <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" /><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" /></svg>
                {/if}
              </button>
            </div>
          </div>
        {:else}
          <div class="space-y-1.5 md:col-span-2">
            <label for="file-db-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
              Database File Path
            </label>
            <input
              id="file-db-input"
              type="text"
              bind:value={form.database}
              placeholder="/path/to/database.db or :memory:"
              class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono text-neutral-900 dark:text-white"
            />
          </div>
        {/if}
      </div>

    {:else if activeTab === 'ssh'}
      <!-- SSH TUNNEL TAB -->
      <div class="space-y-5">
        <div class="flex items-center justify-between p-4 bg-neutral-50 dark:bg-neutral-800/50 rounded-xl border border-neutral-200 dark:border-neutral-700/80">
          <div>
            <h4 class="text-xs font-bold text-neutral-900 dark:text-white">Use SSH Tunnel (Bastion / Jump Host)</h4>
            <p class="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
              Securely connect to databases isolated inside private VPCs or behind firewalls.
            </p>
          </div>
          <button
            type="button"
            onclick={toggleSshTunnel}
            class="relative inline-flex h-6 w-11 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-200 ease-in-out focus:outline-none {form.ssh_tunnel?.enabled ? 'bg-blue-600' : 'bg-neutral-300 dark:bg-neutral-700'}"
            role="switch"
            aria-checked={form.ssh_tunnel?.enabled}
          >
            <span class="inline-block h-5 w-5 transform rounded-full bg-white shadow transition duration-200 ease-in-out {form.ssh_tunnel?.enabled ? 'translate-x-5' : 'translate-x-0'}"></span>
          </button>
        </div>

        {#if form.ssh_tunnel?.enabled}
          <div class="grid grid-cols-1 md:grid-cols-2 gap-4 p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900/60">
            <div class="space-y-1.5">
              <label for="ssh-bastion-host" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                SSH Bastion Host / IP
              </label>
              <input
                id="ssh-bastion-host"
                type="text"
                bind:value={form.ssh_tunnel.host}
                placeholder="bastion.example.com"
                class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
              />
            </div>

            <div class="space-y-1.5">
              <label for="ssh-bastion-port" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                SSH Port
              </label>
              <input
                id="ssh-bastion-port"
                type="number"
                bind:value={form.ssh_tunnel.port}
                placeholder="22"
                class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
              />
            </div>

            <div class="space-y-1.5">
              <label for="ssh-bastion-user" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                SSH Username
              </label>
              <input
                id="ssh-bastion-user"
                type="text"
                bind:value={form.ssh_tunnel.user}
                placeholder="ubuntu"
                class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
              />
            </div>

            <div class="space-y-1.5">
              <label for="ssh-auth-method" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                Authentication Method
              </label>
              <select
                id="ssh-auth-method"
                bind:value={form.ssh_tunnel.auth_type}
                class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
              >
                <option value="key">Private Key File</option>
                <option value="password">SSH Password</option>
                <option value="agent">SSH Agent</option>
              </select>
            </div>

            {#if form.ssh_tunnel.auth_type === 'key'}
              <div class="space-y-1.5 md:col-span-2">
                <label for="ssh-key-file" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                  Private Key Path
                </label>
                <input
                  id="ssh-key-file"
                  type="text"
                  bind:value={form.ssh_tunnel.key_path}
                  placeholder="~/.ssh/id_ed25519"
                  class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                />
              </div>

              <div class="space-y-1.5 md:col-span-2">
                <label for="ssh-key-pass" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                  Key Passphrase (Optional)
                </label>
                <div class="relative">
                  <input
                    id="ssh-key-pass"
                    type={showSshPassphrase ? 'text' : 'password'}
                    bind:value={form.ssh_tunnel.key_passphrase}
                    placeholder="••••••••"
                    class="w-full pl-3 pr-10 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                  />
                  <button
                    type="button"
                    onclick={() => (showSshPassphrase = !showSshPassphrase)}
                    class="absolute right-2.5 top-2.5 text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200 cursor-pointer"
                    aria-label={showSshPassphrase ? 'Hide passphrase' : 'Show passphrase'}
                  >
                    {#if showSshPassphrase}
                      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l18 18" /></svg>
                    {:else}
                      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" /><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" /></svg>
                    {/if}
                  </button>
                </div>
              </div>
            {:else if form.ssh_tunnel.auth_type === 'password'}
              <div class="space-y-1.5 md:col-span-2">
                <label for="ssh-pass-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                  SSH Password
                </label>
                <div class="relative">
                  <input
                    id="ssh-pass-input"
                    type={showSshPassword ? 'text' : 'password'}
                    bind:value={form.ssh_tunnel.password}
                    placeholder="••••••••"
                    class="w-full pl-3 pr-10 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                  />
                  <button
                    type="button"
                    onclick={() => (showSshPassword = !showSshPassword)}
                    class="absolute right-2.5 top-2.5 text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200 cursor-pointer"
                    aria-label={showSshPassword ? 'Hide password' : 'Show password'}
                  >
                    {#if showSshPassword}
                      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l18 18" /></svg>
                    {:else}
                      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" /><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" /></svg>
                    {/if}
                  </button>
                </div>
              </div>
            {/if}

            <div class="md:col-span-2 pt-2">
              <button
                type="button"
                onclick={handleTestSsh}
                disabled={isTestingSsh}
                class="px-3.5 py-1.5 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors disabled:opacity-50 cursor-pointer text-neutral-800 dark:text-neutral-200"
              >
                {isTestingSsh ? 'Testing Handshake...' : 'Test SSH Handshake'}
              </button>
            </div>

            {#if sshTestResult}
              <div class="md:col-span-2 p-3 rounded-lg text-xs font-medium border {sshTestResult.success ? 'bg-emerald-50 dark:bg-emerald-950/40 border-emerald-300 dark:border-emerald-800 text-emerald-700 dark:text-emerald-300' : 'bg-rose-50 dark:bg-rose-950/40 border-rose-300 dark:border-rose-800 text-rose-700 dark:text-rose-300'}">
                <div class="font-bold">{sshTestResult.success ? 'SSH Handshake Verified ✓' : 'SSH Handshake Failed'}</div>
                <div>{sshTestResult.message}</div>
              </div>
            {/if}
          </div>
        {/if}
      </div>

    {:else if activeTab === 'advanced'}
      <!-- ADVANCED & SAFETY TAB -->
      <div class="space-y-4 p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/40">
        <div class="flex items-center justify-between">
          <div>
            <h4 class="text-xs font-bold text-neutral-900 dark:text-white">Read-Only Mode</h4>
            <p class="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
              Prevents destructive queries (DELETE, UPDATE, DROP) from being sent to this database.
            </p>
          </div>
          <input
            type="checkbox"
            bind:checked={form.read_only}
            disabled={form.environment === 'production'}
            class="h-4 w-4 rounded border-neutral-300 text-blue-600 focus:ring-blue-500"
          />
        </div>

        {#if form.environment === 'production'}
          <div class="p-3 rounded-lg bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-800/60 text-rose-600 dark:text-rose-400 text-xs">
            <strong>Production Safety Active:</strong> Connections marked as Production are automatically locked in READ-ONLY mode by CATable's internal guardrail engine.
          </div>
        {/if}
      </div>
    {/if}

  </div>

  <!-- Real-time Test Result Feedback -->
  {#if testResult}
    <div class="mt-6 p-3 rounded-xl text-xs font-medium border {testResult.success ? 'bg-emerald-50 dark:bg-emerald-950/40 border-emerald-300 dark:border-emerald-800 text-emerald-700 dark:text-emerald-300' : 'bg-rose-50 dark:bg-rose-950/40 border-rose-300 dark:border-rose-800 text-rose-700 dark:text-rose-300'}">
      <div class="font-bold">{testResult.success ? 'Database Connection Successful ✓' : 'Connection Failed'}</div>
      <div class="mt-0.5 font-mono text-[11px]">{testResult.message}</div>
    </div>
  {/if}

  <!-- Bottom Actions Footer (Antares SQL Style) -->
  <div class="mt-8 pt-4 border-t border-neutral-200 dark:border-neutral-800 flex items-center justify-between shrink-0">
    <button
      type="button"
      onclick={handleTest}
      disabled={isTesting}
      class="px-4 py-2 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 text-neutral-700 dark:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors disabled:opacity-50 cursor-pointer"
    >
      {isTesting ? 'Testing Connection...' : 'Test Connection'}
    </button>

    <div class="flex items-center gap-3">
      <button
        type="button"
        onclick={handleSave}
        class="px-4 py-2 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 text-neutral-700 dark:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
      >
        Save
      </button>

      <button
        type="button"
        onclick={handleSaveAndConnect}
        class="flex items-center gap-2 px-5 py-2 text-xs font-bold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-all cursor-pointer hover:shadow-md"
      >
        <span>Connect & Browse Tables</span>
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M14 5l7 7m0 0l-7 7m7-7H3" />
        </svg>
      </button>
    </div>
  </div>

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