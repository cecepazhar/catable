<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { dbStore, type ConnectionConfig } from '$lib/stores/database.svelte';
  import { showToast, confirmModal } from '$lib/stores/uiNotifications.svelte';
  import { goto } from '$app/navigation';

  export interface DbService {
    id: string;
    engine: 'postgres' | 'mysql' | 'mariadb';
    version: string;
    name: string;
    port: number;
    data_dir: string;
    bin_dir: string;
    state: 'stopped' | 'starting' | 'running' | 'stopping' | 'failing';
    created_at: number;
  }

  let services = $state<DbService[]>([]);
  let isLoading = $state(false);
  let isModalOpen = $state(false);
  let probing = $state<Record<string, boolean>>({});

  let formEngine = $state<'postgres' | 'mysql' | 'mariadb'>('postgres');
  let formVersion = $state('16');
  let formName = $state('');
  let formPort = $state(5432);
  let formStartAuto = $state(true);
  let availableVersions = $state<Record<string, string[]>>({
    postgres: [],
    mysql: [],
    mariadb: [],
  });

  const ENGINE_VERSIONS: Record<string, string[]> = {
    postgres: ['17.5', '16.9', '15.10'],
    mysql: ['8.4', '8.0', '5.7'],
    mariadb: ['11.8', '11.2', '10.11'],
  };

  function engineLabel(e: DbService['engine']): string {
    switch (e) {
      case 'postgres': return 'PostgreSQL';
      case 'mysql': return 'MySQL';
      case 'mariadb': return 'MariaDB';
    }
  }

  function stateLabel(s: DbService['state']): string {
    switch (s) {
      case 'running': return '● Running';
      case 'starting': return '◌ Starting…';
      case 'stopped': return '○ Stopped';
      case 'stopping': return '◌ Stopping…';
      case 'failing': return '✕ Failing';
    }
  }

  function stateColor(s: DbService['state']): string {
    switch (s) {
      case 'running': return 'text-emerald-500';
      case 'starting': return 'text-amber-500';
      case 'stopped': return 'text-neutral-400';
      case 'failing': return 'text-rose-500';
      default: return 'text-amber-500';
    }
  }

  function engineIconColor(e: DbService['engine']): string {
    switch (e) {
      case 'postgres': return 'bg-sky-500/10 text-sky-500 border-sky-500/20';
      case 'mysql': return 'bg-orange-500/10 text-orange-500 border-orange-500/20';
      case 'mariadb': return 'bg-cyan-500/10 text-cyan-500 border-cyan-500/20';
    }
  }

  function engineIconPath(e: DbService['engine']): string {
    switch (e) {
      case 'postgres': return 'M4 7c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3zm0 10c0 1.7 3.6 3 8 3s8-1.3 8-3-3.6-3-8-3-8 1.3-8 3zm0-5c0 1.7 3.6 3 8 3s8-1.3 8-3-3.6-3-8-3-8 1.3-8 3z';
      case 'mysql': return 'M5 19a2 2 0 01-2-2V7a2 2 0 012-2h8l4 4v8a2 2 0 01-2 2H5zm12-6l-4-4';
      case 'mariadb': return 'M4 19V5a2 2 0 012-2h9.2l5.8 5.8V19a2 2 0 01-2 2H6a2 2 0 01-2-2z';
    }
  }

  async function loadServices() {
    isLoading = true;
    try {
      const list = await invoke<DbService[]>('svc_list');
      services = list || [];
      // Load available versions for each engine
      for (const eng of ['postgres', 'mysql', 'mariadb'] as const) {
        try {
          const versions = await invoke<string[]>('svc_available_versions', { engine: eng });
          if (versions?.length) availableVersions[eng] = versions;
        } catch {
          // Fallback to static list
        }
      }
    } catch (e: any) {
      console.warn('Failed to load db services:', e);
      services = [];
    } finally {
      isLoading = false;
    }
  }

  async function probeService(svc: DbService) {
    probing[svc.id] = true;
    try {
      const result = await invoke<{
        reachable: boolean;
        state: string;
        engine_version: string | null;
      }>('svc_probe', { serviceId: svc.id });
      // Update service state based on probe result
      const svcIdx = services.findIndex((s) => s.id === svc.id);
      if (svcIdx >= 0) {
        if (result.reachable) {
          services[svcIdx] = { ...services[svcIdx], state: 'running' };
          showToast(`Service "${svc.name}" is reachable on port ${svc.port}`, 'success');
        } else {
          services[svcIdx] = { ...services[svcIdx], state: 'stopped' };
          showToast(`Service "${svc.name}" not reachable (state: ${result.state})`, 'error');
        }
      }
    } catch (e: any) {
      showToast(e?.message || 'Probe failed', 'error');
    } finally {
      probing[svc.id] = false;
    }
  }

  onMount(loadServices);

  function openNew() {
    formEngine = 'postgres';
    formVersion = '16';
    formName = '';
    formPort = 5432;
    formStartAuto = true;
    isModalOpen = true;
  }

  function setEngine(e: 'postgres' | 'mysql' | 'mariadb') {
    formEngine = e;
    formVersion = ENGINE_VERSIONS[e][0];
    formPort = e === 'postgres' ? 5432 : 3306;
  }

  async function handleCreate() {
    if (!formName.trim()) {
      showToast('Service name is required', 'error');
      return;
    }
    try {
      const svc = await invoke<DbService>('svc_create', {
        engine: formEngine,
        version: formVersion,
        name: formName.trim(),
        preferredPort: formPort,
      });
      showToast(`Service "${svc.name}" created on port ${svc.port}`, 'success');
      isModalOpen = false;
      await loadServices();
    } catch (e: any) {
      showToast(e?.message || 'Failed to create service', 'error');
    }
  }

  async function handleStop(svc: DbService) {
    try {
      await invoke('svc_stop', { serviceId: svc.id });
      showToast(`Service "${svc.name}" stopped`, 'success');
      await loadServices();
    } catch (e: any) {
      showToast(e?.message || 'Failed to stop service', 'error');
    }
  }

  async function handleRemove(svc: DbService) {
    const ok = await confirmModal(
      `Remove service "${svc.name}"? Its data directory will be removed.`,
      'Remove Service',
      true,
      'Remove',
      'Cancel'
    );
    if (!ok) return;
    try {
      await invoke('svc_remove', { serviceId: svc.id });
      showToast(`Service "${svc.name}" removed`, 'success');
      await loadServices();
    } catch (e: any) {
      showToast(e?.message || 'Failed to remove service', 'error');
    }
  }

  async function handleOpenInStudio(svc: DbService) {
    try {
      const conn = await invoke<ConnectionConfig>('svc_open_in_studio', { serviceId: svc.id });
      await dbStore.connect(conn);
      showToast(`Connected to "${svc.name}"`, 'success');
      void goto('/table-browser');
    } catch (e: any) {
      showToast(`Could not connect to "${svc.name}": ${e?.message || e}`, 'error');
    }
  }
</script>

<div class="max-w-6xl mx-auto space-y-6 pb-12">
  <!-- Header -->
  <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-4 border-b border-neutral-200 dark:border-neutral-800">
    <div class="flex items-center gap-3">
      <div class="w-10 h-10 rounded-xl bg-emerald-500/10 text-emerald-500 border border-emerald-500/20 flex items-center justify-center">
        <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2" />
        </svg>
      </div>
      <div>
        <h1 class="text-xl font-bold tracking-tight text-neutral-900 dark:text-white">DB Service Engine</h1>
        <p class="text-xs text-neutral-500 dark:text-neutral-400">
          Local PostgreSQL / MySQL / MariaDB server manager — {services.length} services
        </p>
      </div>
    </div>

    <button
      type="button"
      onclick={openNew}
      class="px-3.5 py-1.5 text-xs font-semibold text-white bg-emerald-600 hover:bg-emerald-500 rounded-lg shadow-sm transition-colors flex items-center gap-1.5 cursor-pointer"
    >
      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M12 4v16m8-8H4" />
      </svg>
      <span>New Service</span>
    </button>
  </div>

  <!-- Services Grid -->
  {#if services.length === 0 && !isLoading}
    <div class="p-10 rounded-2xl border border-dashed border-neutral-300 dark:border-neutral-700 bg-neutral-50/50 dark:bg-neutral-900/40 text-center">
      <svg class="w-10 h-10 mx-auto text-neutral-400 mb-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2" />
      </svg>
      <p class="text-sm font-semibold text-neutral-700 dark:text-neutral-300">No Database Services Yet</p>
      <p class="text-xs text-neutral-500 dark:text-neutral-400 mt-1">
        Create a local PostgreSQL, MySQL, or MariaDB instance to manage in CATable.
      </p>
    </div>
  {:else if services.length > 0}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
      {#each services as svc (svc.id)}
        <div class="p-5 rounded-2xl border border-neutral-200/90 dark:border-neutral-800 bg-white dark:bg-neutral-900/90 shadow-2xs hover:shadow-md transition-all flex flex-col justify-between space-y-4 text-left">
          <div class="flex items-start justify-between gap-3">
            <div class="flex items-center gap-3 min-w-0">
              <div class="w-10 h-10 rounded-xl flex items-center justify-center shrink-0 border {engineIconColor(svc.engine)}">
                <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d={engineIconPath(svc.engine)} />
                </svg>
              </div>
              <div class="min-w-0">
                <h3 class="font-bold text-sm text-neutral-900 dark:text-neutral-100 truncate">{svc.name}</h3>
                <p class="text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                  {engineLabel(svc.engine)} {svc.version}
                </p>
              </div>
            </div>
            <span class="text-[11px] font-mono font-semibold {stateColor(svc.state)} shrink-0">
              {stateLabel(svc.state)}
            </span>
          </div>

          <div class="text-[11px] font-mono text-neutral-400 flex items-center gap-1.5">
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 10V3L4 14h7v7l9-11h-7z" />
            </svg>
            Port {svc.port} · 127.0.0.1
          </div>

          <div class="pt-3 border-t border-neutral-100 dark:border-neutral-800 flex items-center justify-between gap-2 flex-wrap">
            <div class="flex items-center gap-1.5">
              <button
                type="button"
                onclick={() => handleOpenInStudio(svc)}
                class="px-2.5 py-1.5 text-[11px] font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg transition-colors flex items-center gap-1 cursor-pointer"
                title="Open connection in CATable Studio"
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
                </svg>
                Open in Studio
              </button>
              {#if svc.state === 'running'}
                <button
                  type="button"
                  onclick={() => handleStop(svc)}
                  class="px-2.5 py-1.5 text-[11px] font-semibold text-neutral-700 dark:text-neutral-300 border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 rounded-lg transition-colors cursor-pointer"
                >
                  Stop
                </button>
              {/if}
              <button
                type="button"
                onclick={() => probeService(svc)}
                disabled={!!probing[svc.id]}
                class="px-2.5 py-1.5 text-[11px] font-semibold text-neutral-700 dark:text-neutral-300 border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 rounded-lg transition-colors cursor-pointer disabled:opacity-50 flex items-center gap-1"
                title="Test connection status"
              >
                {#if probing[svc.id]}
                  <span class="inline-block w-3 h-3 border-2 border-neutral-400 border-t-emerald-500 rounded-full animate-spin"></span>
                  Testing…
                {:else}
                  <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                  </svg>
                  Test
                {/if}
              </button>
            </div>
            <button
              type="button"
              onclick={() => handleRemove(svc)}
              class="p-1.5 rounded-lg hover:bg-rose-50 dark:hover:bg-rose-950/40 text-neutral-400 hover:text-rose-500 transition-colors"
              title="Remove Service"
            >
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
              </svg>
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<!-- New Service Modal -->
{#if isModalOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-xs p-4">
    <div class="w-full max-w-md bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-2xl p-6 shadow-2xl space-y-5 text-neutral-800 dark:text-neutral-200">
      <div class="flex items-center justify-between pb-3 border-b border-neutral-200 dark:border-neutral-800">
        <h3 class="text-base font-bold text-neutral-900 dark:text-white">Create Database Service</h3>
        <button
          type="button"
          onclick={() => (isModalOpen = false)}
          class="p-1 rounded text-neutral-400 hover:text-neutral-900 dark:hover:text-white"
        >
          ✕
        </button>
      </div>

      <div class="space-y-4 text-xs">
        <div>
          <span class="block font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-2">Engine</span>
          <div class="grid grid-cols-3 gap-2">
            {#each ['postgres', 'mysql', 'mariadb'] as eng}
              <button
                type="button"
                onclick={() => setEngine(eng as any)}
                class="px-3 py-2.5 rounded-lg border text-xs font-semibold transition-colors cursor-pointer {formEngine === eng
                  ? 'border-emerald-500 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400'
                  : 'border-neutral-300 dark:border-neutral-700 text-neutral-600 dark:text-neutral-300 hover:border-neutral-400'}"
              >
                {engineLabel(eng as any)}
              </button>
            {/each}
          </div>
        </div>

        <div>
          <label class="block font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-1">Version</label>
          <select
            bind:value={formVersion}
            class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-emerald-500 text-neutral-900 dark:text-white text-xs"
          >
            {#each (availableVersions[formEngine]?.length ? availableVersions[formEngine] : ENGINE_VERSIONS[formEngine]) as v}
              <option value={v}>{v}</option>
            {/each}
          </select>
        </div>

        <div>
          <label class="block font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-1">Service Name</label>
          <input
            type="text"
            bind:value={formName}
            placeholder="e.g. fathforce-dev-db, analytics-lake"
            class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-emerald-500 text-neutral-900 dark:text-white"
          />
        </div>

        <div>
          <label class="block font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-1">Port</label>
          <div class="flex items-center gap-2">
            <input
              type="number"
              bind:value={formPort}
              min="1024"
              max="65535"
              class="flex-1 px-3 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-emerald-500 text-neutral-900 dark:text-white font-mono"
            />
            <button
              type="button"
              onclick={() => {
                const used = new Set(services.map((s) => s.port));
                let p = formPort;
                while (used.has(p) && p < 65535) p++;
                formPort = p;
              }}
              class="px-2.5 py-2 text-[11px] font-semibold text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/10 rounded-lg transition-colors cursor-pointer whitespace-nowrap"
              title="Find next free port"
            >
              Auto
            </button>
          </div>
        </div>

        <label class="flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            bind:checked={formStartAuto}
            class="accent-emerald-600"
          />
          <span>Start service automatically after creation</span>
        </label>
      </div>

      <div class="flex items-center justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
        <button
          type="button"
          onclick={() => (isModalOpen = false)}
          class="px-4 py-2 text-xs font-semibold rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-600 dark:text-neutral-300"
        >
          Cancel
        </button>
        <button
          type="button"
          onclick={handleCreate}
          class="px-4 py-2 text-xs font-semibold rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white shadow-sm"
        >
          Create Service
        </button>
      </div>
    </div>
  </div>
{/if}
