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
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { getAmbientStore } from '$lib/stores/ambient.svelte';
  import { getPro } from '$lib/stores/pro.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';

  const ambient = getAmbientStore();
  const pro = getPro();

  // Main page view filters & state
  let searchQuery = $state('');
  let envFilter = $state<'all' | 'development' | 'staging' | 'production' | 'ssh'>('all');
  let selectedWorkspace = $state<string>('all');
  let revealedIds = $state<Set<string>>(new Set());
  let testingCardId = $state<string | null>(null);
  let cardTestResults = $state<Record<string, { success: boolean; message: string; latency_ms?: number }>>({});

  // Antares Split Modal State
  let isModalOpen = $state(false);
  let editingId = $state<string | null>(null);
  let form = $state<ConnectionConfig>({ ...DEFAULT_SANDBOX_CONNECTION, id: '', group_name: '' });
  let activeTab = $state<'general' | 'ssh' | 'advanced'>('general');
  let modalSearch = $state('');
  let showDbPassword = $state(false);
  let isTesting = $state(false);
  let testResult = $state<{ success: boolean; message: string; latency_ms?: number } | null>(null);

  // SSH states in modal
  let showSshPassphrase = $state(false);
  let showSshPassword = $state(false);
  let isTestingSsh = $state(false);
  let sshTestResult = $state<{ success: boolean; message: string; latency_ms?: number } | null>(null);

  onMount(async () => {
    await dbStore.loadConnections();
    if (page.url.searchParams.get('action') === 'new') {
      openAdd();
    }
  });

  // Watch for external requests to create or edit from sidebar
  $effect(() => {
    if (dbStore.selectedConnectionId === 'new') {
      openAdd();
      dbStore.selectedConnectionId = null;
    }
  });

  const ENV_COLORS: Record<string, string> = {
    development: 'text-emerald-500 bg-emerald-500/10 border-emerald-500/20',
    staging: 'text-amber-500 bg-amber-500/10 border-amber-500/20',
    production: 'text-rose-500 bg-rose-500/10 border-rose-500/20',
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

  function initForm(base?: Partial<ConnectionConfig>): ConnectionConfig {
    return {
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
      group_name: '',
      ...base,
    };
  }

  function getDriverStyle(driver: DriverKind): string {
    switch (driver) {
      case 'postgres':
        return 'bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20';
      case 'mysql':
        return 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20';
      case 'mariadb':
        return 'bg-teal-500/10 text-teal-600 dark:text-teal-400 border border-teal-500/20';
      case 'redis':
        return 'bg-rose-500/10 text-rose-600 dark:text-rose-400 border border-rose-500/20';
      case 'duckdb':
        return 'bg-yellow-500/10 text-yellow-600 dark:text-yellow-400 border border-yellow-500/20';
      case 'sqlite':
      default:
        return 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20';
    }
  }

  function getCardGlowClass(): string {
    if (!ambient.config.enabled || !ambient.config.cardGlowEnabled || !pro.isPro) {
      return 'border-neutral-200/90 dark:border-neutral-800 hover:border-neutral-300 dark:hover:border-neutral-700';
    }
    if (ambient.config.cardGlowStyle === 'neon-border') {
      return 'border-sky-400 dark:border-sky-400 shadow-[inset_0_0_8px_rgba(56,189,248,0.15)] hover:shadow-[0_0_12px_rgba(56,189,248,0.25)]';
    }
    if (ambient.config.cardGlowStyle === 'chroma-beam') {
      return 'border-sky-400/60 dark:border-sky-400/50 shadow-md shadow-sky-500/10 hover:shadow-lg hover:shadow-sky-500/20';
    }
    return 'border-sky-400/40 dark:border-sky-400/30 hover:border-sky-400/70 hover:shadow-md';
  }

  const availableGroups = $derived.by(() => {
    const set = new Set<string>();
    for (const c of dbStore.connections) {
      if (c.group_name && c.group_name.trim()) {
        set.add(c.group_name.trim());
      }
    }
    return Array.from(set).sort();
  });

  const filteredConnections = $derived.by(() => {
    return dbStore.connections.filter((c) => {
      // Workspace / Group filter
      if (selectedWorkspace === 'ungrouped' && c.group_name && c.group_name.trim()) {
        return false;
      }
      if (selectedWorkspace !== 'all' && selectedWorkspace !== 'ungrouped') {
        if ((c.group_name || '').trim() !== selectedWorkspace) {
          return false;
        }
      }

      // Environment filter
      if (envFilter === 'ssh') {
        if (!c.ssh_tunnel?.enabled) return false;
      } else if (envFilter !== 'all') {
        if (c.environment !== envFilter) return false;
      }

      // Search query
      if (searchQuery.trim()) {
        const q = searchQuery.toLowerCase().trim();
        const matchName = c.name.toLowerCase().includes(q);
        const matchHost = c.host.toLowerCase().includes(q);
        const matchDb = c.database.toLowerCase().includes(q);
        const matchDriver = c.driver.toLowerCase().includes(q);
        const matchGroup = (c.group_name || '').toLowerCase().includes(q);
        const matchUser = c.user.toLowerCase().includes(q);
        if (!matchName && !matchHost && !matchDb && !matchDriver && !matchGroup && !matchUser) {
          return false;
        }
      }

      return true;
    });
  });

  const modalFilteredConnections = $derived.by(() => {
    if (!modalSearch.trim()) return dbStore.connections;
    const q = modalSearch.toLowerCase().trim();
    return dbStore.connections.filter((c) =>
      c.name.toLowerCase().includes(q) ||
      c.host.toLowerCase().includes(q) ||
      c.driver.toLowerCase().includes(q) ||
      (c.group_name || '').toLowerCase().includes(q)
    );
  });

  function toggleReveal(id: string, e: MouseEvent) {
    e.stopPropagation();
    const next = new Set(revealedIds);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    revealedIds = next;
  }

  function formatAddress(c: ConnectionConfig): string {
    if (isFileDriver(c.driver)) {
      return c.database;
    }
    const isRevealed = revealedIds.has(c.id);
    if (isRevealed) {
      return `${c.user}@${c.host}:${c.port}/${c.database}`;
    }
    const user = c.user ? (c.user.length > 2 ? `${c.user.slice(0, 2)}***` : '***') : '***';
    const hostParts = c.host.split('.');
    const maskedHost = hostParts.length === 4
      ? `${hostParts[0]}.***.***.${hostParts[3]}`
      : c.host.length > 4
      ? `${c.host.slice(0, 3)}***`
      : '***';
    return `${user}@${maskedHost}:${c.port}/${c.database}`;
  }

  function openAdd() {
    editingId = null;
    form = initForm();
    testResult = null;
    sshTestResult = null;
    showDbPassword = false;
    showSshPassphrase = false;
    showSshPassword = false;
    activeTab = 'general';
    isModalOpen = true;
  }

  function editConn(c: ConnectionConfig) {
    editingId = c.id;
    form = {
      ...c,
      password: c.password || '',
      group_name: c.group_name || '',
      ssh_tunnel: c.ssh_tunnel ? { ...c.ssh_tunnel } : initSshTunnel(),
    };
    testResult = null;
    sshTestResult = null;
    showDbPassword = false;
    showSshPassphrase = false;
    showSshPassword = false;
    activeTab = 'general';
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
    if (form.ssh_tunnel.enabled && !form.ssh_tunnel.port) {
      form.ssh_tunnel.port = 22;
    }
  }

  async function handleTestSsh() {
    if (!form.ssh_tunnel?.host?.trim()) {
      sshTestResult = { success: false, message: 'SSH Host is required.' };
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
        message: res.message || (res.success ? 'SSH handshake established successfully.' : 'SSH handshake failed.'),
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
      testResult = {
        success: res.success,
        message: res.message + (res.server_version ? ` (${res.server_version})` : ''),
        latency_ms: res.latency_ms,
      };
    } catch (e: any) {
      testResult = { success: false, message: e?.message || 'Connection test failed.' };
    } finally {
      isTesting = false;
    }
  }

  async function handleTestCard(c: ConnectionConfig, e: MouseEvent) {
    e.stopPropagation();
    testingCardId = c.id;
    try {
      const res = await dbStore.testConnection(c);
      cardTestResults[c.id] = {
        success: res.success,
        message: res.message || (res.success ? 'Connected' : 'Failed'),
        latency_ms: res.latency_ms,
      };
    } catch (err: any) {
      cardTestResults[c.id] = {
        success: false,
        message: err?.message || 'Connection test failed',
      };
    } finally {
      testingCardId = null;
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
      const toSave: ConnectionConfig = {
        ...form,
        name: form.name.trim(),
        group_name: form.group_name?.trim() || null,
      };
      if (isFileDriver(toSave.driver)) {
        toSave.ssh_tunnel = null;
        toSave.port = 0;
        toSave.user = '';
        toSave.password = null;
        toSave.host = 'localhost';
      } else {
        if (!toSave.ssh_tunnel?.enabled) {
          toSave.ssh_tunnel = null;
        }
      }
      await dbStore.saveConnection(toSave);
      showToast(`Connection "${toSave.name}" saved successfully.`, 'success');
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
      const toSave: ConnectionConfig = {
        ...form,
        name: form.name.trim(),
        group_name: form.group_name?.trim() || null,
      };
      if (isFileDriver(toSave.driver)) {
        toSave.ssh_tunnel = null;
        toSave.port = 0;
        toSave.user = '';
        toSave.password = null;
        toSave.host = 'localhost';
      } else {
        if (!toSave.ssh_tunnel?.enabled) {
          toSave.ssh_tunnel = null;
        }
      }
      await dbStore.saveConnection(toSave);
      await dbStore.connect(toSave);
      showToast(`Connected to "${toSave.name}".`, 'success');
      isModalOpen = false;
      await goto('/table-browser');
    } catch (e: any) {
      showToast(e?.message || 'Failed to save and connect.', 'error');
    }
  }

  async function handleDelete(c: ConnectionConfig) {
    if (!c || !c.id) return;
    const ok = window.confirm(`Delete connection "${c.name}"?`);
    if (!ok) return;
    try {
      await dbStore.deleteConnection(c.id);
      showToast(`Connection "${c.name}" deleted.`, 'success');
      if (isModalOpen && editingId === c.id) {
        if (dbStore.connections.length > 0) {
          editConn(dbStore.connections[0]);
        } else {
          openAdd();
        }
      }
    } catch (e: any) {
      showToast(e?.message || 'Failed to delete connection.', 'error');
    }
  }

  async function handleQuickConnect(c: ConnectionConfig, e?: MouseEvent) {
    if (e) e.stopPropagation();
    try {
      await dbStore.connect(c);
      showToast(`Connected to "${c.name}".`, 'success');
      await goto('/table-browser');
    } catch (err: any) {
      showToast(err?.message || 'Failed to connect.', 'error');
    }
  }
</script>

<div class="flex flex-col h-full overflow-y-auto p-4 md:p-6 space-y-5 max-w-7xl mx-auto w-full select-none">
  
  <!-- ============================================== -->
  <!-- PAGE HEADER: Title, Stats, Search, Filters, New -->
  <!-- ============================================== -->
  <div class="flex flex-col lg:flex-row lg:items-center justify-between gap-4 pb-4 border-b border-neutral-200 dark:border-neutral-800">
    <div class="flex items-center gap-3">
      <div class="w-10 h-10 rounded-xl bg-blue-100 dark:bg-blue-950/60 border border-blue-200 dark:border-blue-800/60 flex items-center justify-center text-blue-600 dark:text-blue-400 shadow-xs shrink-0">
        <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M5 12h14M5 12a2 2 0 00-2 2v5a2 2 0 002 2h14a2 2 0 002-2v-5a2 2 0 00-2-2M5 12a2 2 0 01-2-2V5a2 2 0 012-2h14a2 2 0 012 2v5a2 2 0 01-2 2M5 12v5" />
        </svg>
      </div>
      <div>
        <h1 class="text-lg font-bold tracking-tight text-neutral-900 dark:text-neutral-100 leading-tight">Database Connections</h1>
        <p class="text-xs text-neutral-500 dark:text-neutral-400">
          {dbStore.connections.length} connections across {availableGroups.length > 0 ? `${availableGroups.length} workspaces` : 'default workspace'} · Zero-Knowledge Encrypted
        </p>
      </div>
    </div>

    <!-- Quick Search, Filter Pills & + New Connection Button -->
    <div class="flex flex-wrap items-center gap-2.5">
      <!-- Search Input -->
      <div class="relative min-w-[200px] flex-1 sm:flex-initial">
        <input
          type="text"
          bind:value={searchQuery}
          placeholder="Search connections..."
          class="w-full text-xs pl-8 pr-3 py-1.5 bg-neutral-50 dark:bg-neutral-800/90 rounded-lg border border-neutral-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-blue-500 text-neutral-800 dark:text-neutral-200 transition-colors"
        />
        <svg class="w-3.5 h-3.5 absolute left-2.5 top-2.5 text-neutral-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
        </svg>
        {#if searchQuery}
          <button
            onclick={() => (searchQuery = '')}
            class="absolute right-2 top-2 text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-200"
            aria-label="Clear search"
          >
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        {/if}
      </div>

      <!-- Environment Filter Pills -->
      <div class="flex items-center gap-1 bg-neutral-100 dark:bg-neutral-800/80 p-0.5 rounded-lg border border-neutral-200 dark:border-neutral-700/60 text-xs">
        {#each [
          { id: 'all', label: 'All' },
          { id: 'development', label: 'Dev' },
          { id: 'staging', label: 'Staging' },
          { id: 'production', label: 'Prod' },
          { id: 'ssh', label: 'SSH' }
        ] as filter}
          <button
            onclick={() => (envFilter = filter.id as any)}
            class="px-2.5 py-1 rounded-md text-[11px] font-semibold transition-all cursor-pointer {envFilter === filter.id ? 'bg-white dark:bg-neutral-900 text-neutral-900 dark:text-white shadow-2xs' : 'text-neutral-500 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white'}"
          >
            {filter.label}
          </button>
        {/each}
      </div>

      <!-- New Connection Button -->
      <button
        type="button"
        onclick={openAdd}
        class="flex items-center gap-1.5 px-3.5 py-1.5 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-all cursor-pointer hover:shadow-md shrink-0"
      >
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M12 4v16m8-8H4" />
        </svg>
        <span>New Connection</span>
      </button>
    </div>
  </div>

  <!-- ============================================== -->
  <!-- WORKSPACES & GROUPS TABS BAR                  -->
  <!-- ============================================== -->
  <div class="flex items-center gap-1.5 overflow-x-auto scrollbar-none pb-1 text-xs">
    <button
      onclick={() => (selectedWorkspace = 'all')}
      class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-semibold transition-colors shrink-0 cursor-pointer {selectedWorkspace === 'all' ? 'bg-blue-600 text-white shadow-xs' : 'bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/70 dark:hover:bg-neutral-700/60'}"
    >
      <span>All Workspaces</span>
      <span class="px-1.5 py-0.2 rounded-full text-[10px] font-mono {selectedWorkspace === 'all' ? 'bg-blue-700 text-blue-100' : 'bg-neutral-200 dark:bg-neutral-700 text-neutral-500 dark:text-neutral-300'}">
        {dbStore.connections.length}
      </span>
    </button>

    {#each availableGroups as grp}
      {@const count = dbStore.connections.filter((c) => (c.group_name || '').trim() === grp).length}
      <button
        onclick={() => (selectedWorkspace = grp)}
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-semibold transition-colors shrink-0 cursor-pointer {selectedWorkspace === grp ? 'bg-blue-600 text-white shadow-xs' : 'bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/70 dark:hover:bg-neutral-700/60'}"
      >
        <svg class="w-3.5 h-3.5 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
        </svg>
        <span>{grp}</span>
        <span class="px-1.5 py-0.2 rounded-full text-[10px] font-mono {selectedWorkspace === grp ? 'bg-blue-700 text-blue-100' : 'bg-neutral-200 dark:bg-neutral-700 text-neutral-500 dark:text-neutral-300'}">
          {count}
        </span>
      </button>
    {/each}

    {#if dbStore.connections.some((c) => !c.group_name || !c.group_name.trim()) && availableGroups.length > 0}
      {@const ungrpCount = dbStore.connections.filter((c) => !c.group_name || !c.group_name.trim()).length}
      <button
        onclick={() => (selectedWorkspace = 'ungrouped')}
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg font-semibold transition-colors shrink-0 cursor-pointer {selectedWorkspace === 'ungrouped' ? 'bg-blue-600 text-white shadow-xs' : 'bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/70 dark:hover:bg-neutral-700/60'}"
      >
        <span>Ungrouped</span>
        <span class="px-1.5 py-0.2 rounded-full text-[10px] font-mono {selectedWorkspace === 'ungrouped' ? 'bg-blue-700 text-blue-100' : 'bg-neutral-200 dark:bg-neutral-700 text-neutral-500 dark:text-neutral-300'}">
          {ungrpCount}
        </span>
      </button>
    {/if}
  </div>

  <!-- ============================================== -->
  <!-- MAIN CONTENT: CATERM CARD GRID / EMPTY STATE    -->
  <!-- ============================================== -->
  {#if filteredConnections.length === 0}
    <EmptyState
      title="No Connections Found"
      description={searchQuery ? 'No database connections match your search criteria.' : 'Create a new database connection to get started.'}
      icon="connections"
      actionLabel="Create Connection"
      onaction={openAdd}
    />
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4 pt-1">
      {#each filteredConnections as conn (conn.id)}
        <div
          role="button"
          tabindex="0"
          onclick={() => handleQuickConnect(conn)}
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              void handleQuickConnect(conn);
            }
          }}
          class="group relative flex flex-col justify-between p-4.5 bg-white dark:bg-neutral-900/90 border {getCardGlowClass()} rounded-2xl shadow-2xs hover:shadow-lg transition-all duration-200 space-y-3.5 cursor-pointer text-left"
        >
          <!-- Top Row: Driver Icon, Title, Badges & Quick Actions -->
          <div class="flex items-start justify-between gap-2">
            <div class="flex items-center gap-3 min-w-0">
              <div class="w-10 h-10 rounded-xl {getDriverStyle(conn.driver)} flex items-center justify-center shrink-0 font-bold font-mono text-xs shadow-2xs">
                {conn.driver.slice(0, 2).toUpperCase()}
              </div>

              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <h3 class="font-bold text-sm text-neutral-900 dark:text-neutral-100 truncate">{conn.name}</h3>
                  {#if conn.id === dbStore.activeConnection.id && dbStore.isConnected}
                    <span class="text-[9px] font-bold bg-emerald-500 text-white rounded px-1.5 py-0.5 tracking-wider flex items-center gap-1 shadow-2xs">
                      <span class="w-1 h-1 rounded-full bg-white animate-pulse"></span>
                      ACTIVE
                    </span>
                  {/if}
                </div>

                <div class="flex items-center gap-1.5 mt-1 text-[10px] font-mono flex-wrap">
                  <span class="uppercase bg-neutral-100 dark:bg-neutral-800 px-1.5 py-0.5 rounded font-semibold text-neutral-700 dark:text-neutral-300">
                    {DRV_LABELS[conn.driver] || conn.driver}
                  </span>
                  <span class={`px-1.5 py-0.5 rounded font-semibold uppercase tracking-wide border ${ENV_COLORS[conn.environment]}`}>
                    {conn.environment}
                  </span>
                  {#if conn.ssh_tunnel?.enabled}
                    <span class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded font-bold uppercase tracking-wider text-[9px] bg-sky-500/10 text-sky-600 dark:text-sky-400 border border-sky-500/20 shadow-2xs" title={`SSH Tunnel: ${conn.ssh_tunnel.user}@${conn.ssh_tunnel.host}:${conn.ssh_tunnel.port || 22}`}>
                      <svg class="w-2.5 h-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" />
                      </svg>
                      SSH
                    </span>
                  {/if}
                  {#if conn.group_name}
                    <span class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[9px] bg-neutral-100 dark:bg-neutral-800 text-neutral-500 dark:text-neutral-400 border border-neutral-200/60 dark:border-neutral-700/60">
                      <svg class="w-2.5 h-2.5 opacity-60" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                      </svg>
                      {conn.group_name}
                    </span>
                  {/if}
                </div>
              </div>
            </div>

            <!-- Quick Card Action Icons (Test, Edit, Delete) -->
            <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
              <button
                type="button"
                onclick={(e) => handleTestCard(conn, e)}
                class="p-1.5 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-400 hover:text-emerald-500 transition-colors"
                title="Test Connection"
                aria-label="Test connection"
              >
                {#if testingCardId === conn.id}
                  <svg class="w-3.5 h-3.5 animate-spin text-blue-500" fill="none" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
                  </svg>
                {:else}
                  <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z" />
                  </svg>
                {/if}
              </button>

              <button
                type="button"
                onclick={(e) => { e.stopPropagation(); editConn(conn); }}
                class="p-1.5 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-400 hover:text-blue-500 transition-colors"
                title="Edit in Connection Hub"
                aria-label="Edit connection"
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" />
                </svg>
              </button>

              <button
                type="button"
                onclick={(e) => { e.stopPropagation(); handleDelete(conn); }}
                class="p-1.5 rounded-lg hover:bg-rose-50 dark:hover:bg-rose-950/40 text-neutral-400 hover:text-rose-500 transition-colors"
                title="Delete Connection"
                aria-label="Delete connection"
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                </svg>
              </button>
            </div>
          </div>

          <!-- Middle Row: Masked/Unmasked Host/Database -->
          <div class="flex items-center justify-between gap-2 text-[11px] font-mono text-neutral-600 dark:text-neutral-400 bg-neutral-50/70 dark:bg-neutral-800/40 px-2.5 py-1.5 rounded-lg border border-neutral-200/50 dark:border-neutral-800/60">
            <span class="truncate" title={conn.host}>
              {formatAddress(conn)}
            </span>
            {#if !isFileDriver(conn.driver)}
              <button
                type="button"
                onclick={(e) => toggleReveal(conn.id, e)}
                class="p-0.5 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 shrink-0 cursor-pointer"
                title={revealedIds.has(conn.id) ? 'Hide details' : 'Show details'}
              >
                {#if revealedIds.has(conn.id)}
                  <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                  </svg>
                {:else}
                  <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l18 18" />
                  </svg>
                {/if}
              </button>
            {/if}
          </div>

          <!-- Bottom Row: Status Tag & Connect Button -->
          <div class="pt-2 border-t border-neutral-100 dark:border-neutral-800/80 flex items-center justify-between text-[11px]">
            <div class="flex items-center gap-2">
              {#if cardTestResults[conn.id]}
                {@const res = cardTestResults[conn.id]}
                <span class="inline-flex items-center gap-1 font-mono text-[10px] {res.success ? 'text-emerald-500' : 'text-rose-500'}">
                  {#if res.success}
                    <span class="w-1.5 h-1.5 rounded-full bg-emerald-500"></span>
                    Verified {res.latency_ms ? `· ${res.latency_ms}ms` : ''}
                  {:else}
                    <span class="w-1.5 h-1.5 rounded-full bg-rose-500"></span>
                    Failed
                  {/if}
                </span>
              {:else if conn.read_only}
                <span class="text-amber-500 font-semibold flex items-center gap-1 text-[10px]">
                  <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" /></svg>
                  Read-Only
                </span>
              {:else}
                <span class="text-neutral-400 dark:text-neutral-500 text-[10px] font-mono flex items-center gap-1">
                  <svg class="w-2.5 h-2.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" /></svg>
                  Encrypted Vault
                </span>
              {/if}
            </div>

            <button
              type="button"
              onclick={(e) => handleQuickConnect(conn, e)}
              class="text-blue-600 dark:text-blue-400 font-semibold hover:underline flex items-center gap-1 group/btn cursor-pointer"
            >
              <span>Connect</span>
              <span class="group-hover/btn:translate-x-0.5 transition-transform">→</span>
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

</div>


<!-- ============================================================== -->
<!-- ANTARES SPLIT CONNECTION MODAL / HUB                            -->
<!-- Left: Connection list & workspaces | Right: Tabbed form        -->
<!-- ============================================================== -->
{#if isModalOpen}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-xs p-3 md:p-6"
    role="dialog"
    aria-modal="true"
  >
    <div
      class="w-full max-w-5xl h-[88vh] max-h-[820px] bg-white dark:bg-[#121212] border border-neutral-200 dark:border-neutral-800 rounded-2xl shadow-2xl flex flex-col md:flex-row overflow-hidden text-neutral-900 dark:text-neutral-100"
    >
      
      <!-- ============================================== -->
      <!-- MODAL LEFT SIDEBAR: Existing Connections & Hub  -->
      <!-- ============================================== -->
      <div class="w-full md:w-72 shrink-0 border-b md:border-b-0 md:border-r border-neutral-200 dark:border-neutral-800 bg-neutral-50/70 dark:bg-[#161616] flex flex-col">
        <!-- Sidebar Header -->
        <div class="p-3 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between gap-2 shrink-0">
          <div>
            <h4 class="text-xs font-bold uppercase tracking-wider text-neutral-800 dark:text-neutral-200">Connections</h4>
            <p class="text-[10px] text-neutral-400">{dbStore.connections.length} saved in vault</p>
          </div>
          <button
            type="button"
            onclick={openAdd}
            class="flex items-center gap-1 px-2.5 py-1 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-colors cursor-pointer"
            title="Create New Connection"
          >
            <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M12 4v16m8-8H4" />
            </svg>
            <span>New</span>
          </button>
        </div>

        <!-- Sidebar Filter Input -->
        <div class="p-2 border-b border-neutral-200 dark:border-neutral-800/60 shrink-0">
          <div class="relative">
            <input
              type="text"
              bind:value={modalSearch}
              placeholder="Search connections..."
              class="w-full text-xs pl-7 pr-2 py-1.5 bg-white dark:bg-neutral-800/90 rounded-lg border border-neutral-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-blue-500 text-neutral-800 dark:text-neutral-200"
            />
            <svg class="w-3.5 h-3.5 absolute left-2 top-2 text-neutral-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
          </div>
        </div>

        <!-- Connection Items List -->
        <div class="flex-1 overflow-y-auto p-2 space-y-1 scrollbar-thin">
          {#each modalFilteredConnections as conn (conn.id)}
            <button
              type="button"
              onclick={() => editConn(conn)}
              class="w-full text-left p-2.5 rounded-xl cursor-pointer transition-all border {editingId === conn.id ? 'bg-blue-50 dark:bg-blue-950/50 border-blue-400 dark:border-blue-600 shadow-xs' : 'border-transparent hover:bg-neutral-200/50 dark:hover:bg-neutral-800/50'}"
            >
              <div class="flex items-center justify-between gap-1 mb-1">
                <span class="font-semibold text-xs text-neutral-900 dark:text-white truncate">{conn.name}</span>
                <div class="flex items-center gap-1 shrink-0">
                  {#if conn.ssh_tunnel?.enabled}
                    <span class="px-1.5 py-0.2 rounded text-[9px] font-mono font-bold bg-sky-500/10 text-sky-500 border border-sky-500/30">SSH</span>
                  {/if}
                  <span class="px-1.5 py-0.2 rounded text-[9px] font-mono font-bold uppercase bg-neutral-200 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300">{conn.driver}</span>
                </div>
              </div>
              <div class="flex items-center justify-between gap-1 text-[10px] text-neutral-500 dark:text-neutral-400 font-mono">
                <span class="truncate">{isFileDriver(conn.driver) ? conn.database : `${conn.host}:${conn.port}`}</span>
                {#if conn.group_name}
                  <span class="text-neutral-400 truncate max-w-[65px]">[{conn.group_name}]</span>
                {/if}
              </div>
            </button>
          {/each}
        </div>
      </div>

      <!-- ============================================== -->
      <!-- MODAL RIGHT SIDE: Tabbed Connection Form       -->
      <!-- ============================================== -->
      <div class="flex-1 flex flex-col overflow-hidden bg-white dark:bg-[#121212]">
        
        <!-- Right Modal Header: Icon, Name Input, Environment Badge, Close Button -->
        <div class="p-4 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between gap-3 shrink-0">
          <div class="flex items-center gap-3 min-w-0 flex-1">
            <div class="w-9 h-9 rounded-xl {getDriverStyle(form.driver)} flex items-center justify-center font-bold font-mono text-xs shadow-2xs shrink-0">
              {form.driver.slice(0, 2).toUpperCase()}
            </div>
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <input
                  type="text"
                  bind:value={form.name}
                  placeholder="Connection Label (e.g. Production Cluster)"
                  class="text-sm font-bold bg-transparent border-b border-transparent hover:border-neutral-300 dark:hover:border-neutral-700 focus:border-blue-500 focus:outline-none text-neutral-900 dark:text-white px-1 py-0.5 rounded transition-colors w-full max-w-sm"
                />
                <span class={`text-[10px] font-semibold uppercase px-2 py-0.5 rounded border ${ENV_COLORS[form.environment]}`}>
                  {form.environment}
                </span>
              </div>
              <p class="text-[11px] text-neutral-500 dark:text-neutral-400 pl-1 mt-0.5 font-mono">
                {DRV_LABELS[form.driver]} · {isFileDriver(form.driver) ? form.database : `${form.user || 'user'}@${form.host || 'host'}:${form.port}`}
              </p>
            </div>
          </div>

          <button
            type="button"
            onclick={() => (isModalOpen = false)}
            aria-label="Close modal"
            class="p-1.5 rounded-lg text-neutral-400 hover:text-neutral-700 dark:hover:text-white hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
          >
            <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <!-- Form Tab Switcher -->
        <div class="flex border-b border-neutral-200 dark:border-neutral-800 px-4 gap-2 shrink-0">
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

        <!-- Scrollable Tab Content -->
        <div class="flex-1 overflow-y-auto p-5 space-y-5">
          
          {#if activeTab === 'general'}
            <!-- ===================== GENERAL TAB ===================== -->
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
              <!-- Driver Type -->
              <div class="space-y-1.5">
                <label for="modal-driver-sel" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                  Driver Type
                </label>
                <select
                  id="modal-driver-sel"
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

              <!-- Environment -->
              <div class="space-y-1.5">
                <label for="modal-env-sel" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                  Environment
                </label>
                <select
                  id="modal-env-sel"
                  bind:value={form.environment}
                  class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-medium focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                >
                  <option value="development">Development</option>
                  <option value="staging">Staging</option>
                  <option value="production">Production (Auto Read-Only)</option>
                </select>
              </div>

              <!-- Workspace / Group Name -->
              <div class="space-y-1.5">
                <label for="modal-group-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                  Workspace / Group Name
                </label>
                <input
                  id="modal-group-input"
                  type="text"
                  bind:value={form.group_name}
                  list="group-suggestions"
                  placeholder="e.g. Production Cluster, Client Alpha..."
                  class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                />
                <datalist id="group-suggestions">
                  {#each availableGroups as g}
                    <option value={g}></option>
                  {/each}
                </datalist>
              </div>

              <!-- Connection Name -->
              <div class="space-y-1.5">
                <label for="modal-name-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                  Connection Label
                </label>
                <input
                  id="modal-name-input"
                  type="text"
                  bind:value={form.name}
                  placeholder="My Database"
                  class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                />
              </div>

              <!-- Driver Specific Fields -->
              {#if isFileDriver(form.driver)}
                <div class="space-y-1.5 md:col-span-2">
                  <label for="modal-file-path" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                    Database File Path
                  </label>
                  <input
                    id="modal-file-path"
                    type="text"
                    bind:value={form.database}
                    placeholder={form.driver === 'sqlite' ? 'sandbox.db or /path/to/db.sqlite or :memory:' : 'app.duckdb or /path/to/data.duckdb'}
                    class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                  />
                  <p class="text-[10px] text-neutral-500">File-based database. Host, port, username, and password are not required.</p>
                </div>
              {:else}
                <div class="space-y-1.5">
                  <label for="modal-host-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                    Host / IP Address
                  </label>
                  <input
                    id="modal-host-input"
                    type="text"
                    bind:value={form.host}
                    placeholder="127.0.0.1"
                    class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                  />
                </div>

                <div class="space-y-1.5">
                  <label for="modal-port-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                    Port
                  </label>
                  <input
                    id="modal-port-input"
                    type="number"
                    bind:value={form.port}
                    class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                  />
                </div>

                <div class="space-y-1.5">
                  <label for="modal-db-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                    {form.driver === 'redis' ? 'Database Index' : 'Database Name'}
                  </label>
                  <input
                    id="modal-db-input"
                    type="text"
                    bind:value={form.database}
                    placeholder={form.driver === 'redis' ? '0' : 'production_db'}
                    class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white font-mono"
                  />
                </div>

                <div class="space-y-1.5">
                  <label for="modal-user-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                    Username
                  </label>
                  <input
                    id="modal-user-input"
                    type="text"
                    bind:value={form.user}
                    placeholder="root"
                    class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white font-mono"
                  />
                </div>

                <div class="space-y-1.5 md:col-span-2">
                  <label for="modal-pass-input" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                    Password (Stored Zero-Knowledge in Vault)
                  </label>
                  <div class="relative">
                    <input
                      id="modal-pass-input"
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
                        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.88 9.88l-3.29-3.29m7.532 7.532l3.29 3.29M3 3l18 18" />
                        </svg>
                      {:else}
                        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                        </svg>
                      {/if}
                    </button>
                  </div>
                </div>
              {/if}
            </div>

          {:else if activeTab === 'ssh'}
            <!-- ===================== SSH TUNNEL TAB ===================== -->
            <div class="space-y-4">
              <div class="flex items-center justify-between p-3.5 bg-neutral-50 dark:bg-neutral-800/50 rounded-xl border border-neutral-200 dark:border-neutral-700/80">
                <div>
                  <h4 class="text-xs font-bold text-neutral-900 dark:text-white">Use SSH Tunnel (Bastion / Jump Host)</h4>
                  <p class="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
                    Route connection through a remote bastion host to access private VPC databases.
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
                <div class="grid grid-cols-1 md:grid-cols-2 gap-4 p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/40 dark:bg-neutral-900/40">
                  <div class="space-y-1.5">
                    <label for="ssh-bastion-host" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                      Bastion Host / IP
                    </label>
                    <input
                      id="ssh-bastion-host"
                      type="text"
                      bind:value={form.ssh_tunnel.host}
                      placeholder="bastion.example.com"
                      class="w-full px-3 py-2 bg-white dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
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
                      class="w-full px-3 py-2 bg-white dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
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
                      class="w-full px-3 py-2 bg-white dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
                    />
                  </div>

                  <div class="space-y-1.5">
                    <label for="ssh-auth-method" class="font-semibold text-neutral-500 dark:text-neutral-400 uppercase text-[10px] tracking-wider">
                      Authentication Method
                    </label>
                    <select
                      id="ssh-auth-method"
                      bind:value={form.ssh_tunnel.auth_type}
                      class="w-full px-3 py-2 bg-white dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
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
                        class="w-full px-3 py-2 bg-white dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
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
                          class="w-full pl-3 pr-10 py-2 bg-white dark:bg-neutral-800/90 border border-neutral-300 dark:border-neutral-700 rounded-lg text-xs font-mono focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
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

                  <div class="md:col-span-2 pt-1">
                    <button
                      type="button"
                      onclick={handleTestSsh}
                      disabled={isTestingSsh}
                      class="px-3 py-1.5 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors disabled:opacity-50 cursor-pointer text-neutral-800 dark:text-neutral-200"
                    >
                      {isTestingSsh ? 'Testing Handshake...' : 'Test SSH Tunnel Handshake'}
                    </button>
                  </div>

                  {#if sshTestResult}
                    <div class="md:col-span-2 p-3 rounded-lg text-xs font-medium border {sshTestResult.success ? 'bg-emerald-50 dark:bg-emerald-950/40 border-emerald-300 dark:border-emerald-800 text-emerald-700 dark:text-emerald-300' : 'bg-rose-50 dark:bg-rose-950/40 border-rose-300 dark:border-rose-800 text-rose-700 dark:text-rose-300'}">
                      <div class="font-bold">{sshTestResult.success ? 'SSH Handshake Verified ✓' : 'SSH Handshake Failed'}</div>
                      <div class="mt-0.5">{sshTestResult.message}</div>
                    </div>
                  {/if}
                </div>
              {/if}
            </div>

          {:else if activeTab === 'advanced'}
            <!-- ===================== ADVANCED & SAFETY TAB ===================== -->
            <div class="space-y-4">
              <div class="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/40 space-y-3">
                <div class="flex items-center justify-between">
                  <div>
                    <h4 class="text-xs font-bold text-neutral-900 dark:text-white">Read-Only Mode</h4>
                    <p class="text-[11px] text-neutral-500 dark:text-neutral-400 mt-0.5">
                      Enforces driver-level read locks. Destructive queries (DELETE, UPDATE, DROP, ALTER) are intercepted before dispatch.
                    </p>
                  </div>
                  <input
                    type="checkbox"
                    bind:checked={form.read_only}
                    disabled={form.environment === 'production'}
                    class="h-4 w-4 rounded border-neutral-300 text-blue-600 focus:ring-blue-500 cursor-pointer"
                  />
                </div>

                {#if form.environment === 'production'}
                  <div class="p-2.5 rounded-lg bg-rose-50 dark:bg-rose-950/40 border border-rose-200 dark:border-rose-800/60 text-rose-600 dark:text-rose-400 text-xs">
                    <strong>Production Safety Policy:</strong> Connections marked as Production are automatically locked in READ-ONLY mode.
                  </div>
                {/if}
              </div>

              <div class="p-4 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/50 dark:bg-neutral-900/40 space-y-2">
                <h4 class="text-xs font-bold text-neutral-900 dark:text-white">Internal Configuration Metadata</h4>
                <div class="text-[11px] font-mono text-neutral-500 dark:text-neutral-400">
                  <div>ID: {form.id || '(Will be generated on save)'}</div>
                  <div>Driver: {form.driver}</div>
                  <div>Security: Zero-Knowledge AES-256-GCM + PBKDF2</div>
                </div>
              </div>
            </div>
          {/if}

          <!-- Test Result Feedback in Modal -->
          {#if testResult}
            <div class="p-3 rounded-xl text-xs font-medium border {testResult.success ? 'bg-emerald-50 dark:bg-emerald-950/40 border-emerald-300 dark:border-emerald-800 text-emerald-700 dark:text-emerald-300' : 'bg-rose-50 dark:bg-rose-950/40 border-rose-300 dark:border-rose-800 text-rose-700 dark:text-rose-300'}">
              <div class="font-bold flex items-center gap-1.5">
                <span>{testResult.success ? '✓ Connection Succeeded' : '✗ Connection Failed'}</span>
                {#if testResult.latency_ms !== undefined}
                  <span class="font-mono text-[10px] opacity-80">({testResult.latency_ms}ms)</span>
                {/if}
              </div>
              <div class="mt-0.5 font-mono text-[11px]">{testResult.message}</div>
            </div>
          {/if}

        </div>

        <!-- Modal Bottom Actions Bar -->
        <div class="p-4 border-t border-neutral-200 dark:border-neutral-800 flex items-center justify-between shrink-0 bg-neutral-50/50 dark:bg-[#151515]">
          <button
            type="button"
            onclick={handleTest}
            disabled={isTesting}
            class="px-3.5 py-2 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 text-neutral-700 dark:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors disabled:opacity-50 cursor-pointer"
          >
            {isTesting ? 'Testing...' : 'Test Connection'}
          </button>

          <div class="flex items-center gap-2">
            {#if form.id && editingId}
              <button
                type="button"
                onclick={() => handleDelete(form)}
                class="px-3 py-2 text-xs font-semibold text-rose-600 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/40 rounded-lg transition-colors cursor-pointer mr-1"
              >
                Delete
              </button>
            {/if}

            <button
              type="button"
              onclick={() => (isModalOpen = false)}
              class="px-3.5 py-2 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
            >
              Cancel
            </button>

            <button
              type="button"
              onclick={handleSave}
              class="px-4 py-2 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 text-neutral-800 dark:text-neutral-100 hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
            >
              Save
            </button>

            <button
              type="button"
              onclick={handleSaveAndConnect}
              class="flex items-center gap-1.5 px-4 py-2 text-xs font-bold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-all cursor-pointer hover:shadow-md"
            >
              <span>Connect & Browse</span>
              <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M14 5l7 7m0 0l-7 7m7-7H3" />
              </svg>
            </button>
          </div>
        </div>

      </div>

    </div>
  </div>
{/if}
