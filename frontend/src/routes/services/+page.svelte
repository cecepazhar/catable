<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fade, fly, scale, slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { dbStore, type ConnectionConfig } from '$lib/stores/database.svelte';
  import { showToast, confirmModal } from '$lib/stores/uiNotifications.svelte';
  import { goto } from '$app/navigation';
  import DatabaseIcon from '$lib/components/DatabaseIcon.svelte';
  import { errorText } from '$lib/errors';

  type Engine = 'postgres' | 'mysql' | 'mariadb' | 'redis';
  type State = 'stopped' | 'starting' | 'running' | 'stopping' | 'failing';

  interface DbService {
    id: string;
    engine: Engine;
    version: string;
    name: string;
    port: number;
    dataDir: string;
    binDir: string;
    state: State;
    createdAt: number;
    pid: number | null;
    autoStart: boolean;
    database: string;
    lastError: string | null;
  }

  interface EngineVersionInfo {
    engine: Engine;
    version: string;
    sizeBytes: number;
    installed: boolean;
    defaultPort: number;
  }

  interface Progress {
    id: string;
    phase: 'download' | 'extract' | 'init' | 'start' | 'stop' | 'ready' | 'error';
    done: number;
    total: number;
    message: string;
  }

  const ENGINES: { key: Engine; label: string; blurb: string }[] = [
    { key: 'postgres', label: 'PostgreSQL', blurb: 'Relational · ACID · extensions' },
    { key: 'mysql', label: 'MySQL', blurb: 'The popular open-source SQL DB' },
    { key: 'mariadb', label: 'MariaDB', blurb: 'Community fork of MySQL' },
    { key: 'redis', label: 'Redis', blurb: 'In-memory key-value store' },
  ];

  let services = $state<DbService[]>([]);
  let versions = $state<EngineVersionInfo[]>([]);
  let loaded = $state(false);
  let selectedId = $state<string | null>(null);
  let busy = $state<Record<string, 'starting' | 'stopping' | 'restarting' | undefined>>({});
  let progress = $state<Record<string, Progress | undefined>>({});
  let search = $state('');

  // Logs drawer
  let logsOpen = $state(false);
  let logsText = $state('');
  let logsTimer: ReturnType<typeof setInterval> | null = null;

  // New service modal
  let modalOpen = $state(false);
  let fEngine = $state<Engine>('postgres');
  let fVersion = $state('');
  let fName = $state('');
  let fPort = $state(5432);
  let fAutoStart = $state(false);
  let fStartNow = $state(true);
  let creating = $state(false);

  // Inline edit (detail panel)
  let editName = $state('');
  let editPort = $state(0);

  const selected = $derived(services.find((s) => s.id === selectedId) ?? null);
  const filtered = $derived(
    services.filter((s) => {
      const q = search.trim().toLowerCase();
      return !q || s.name.toLowerCase().includes(q) || s.engine.includes(q) || String(s.port).includes(q);
    })
  );
  const runningCount = $derived(services.filter((s) => s.state === 'running').length);
  const versionsFor = $derived(versions.filter((v) => v.engine === fEngine));

  $effect(() => {
    // Sync editable fields when selection changes.
    const s = selected;
    if (s) {
      editName = s.name;
      editPort = s.port;
    }
  });

  let unlisten: UnlistenFn[] = [];
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  onMount(async () => {
    await Promise.all([loadServices(), loadVersions()]);
    loaded = true;
    if (!selectedId && services.length) selectedId = services[0].id;

    unlisten.push(
      await listen<Progress>('svc-progress', (ev) => {
        const p = ev.payload;
        progress[p.id] = p;
        if (p.phase === 'ready' || p.phase === 'error') {
          setTimeout(() => {
            if (progress[p.id]?.phase === p.phase) progress[p.id] = undefined;
          }, 1200);
          void loadServices();
          if (p.phase === 'ready') void loadVersions();
        }
      })
    );
    unlisten.push(await listen('svc-changed', () => void loadServices()));
    // Light background reconciliation (detects crashes / external stops).
    pollTimer = setInterval(() => {
      if (!document.hidden) void loadServices();
    }, 5000);
  });

  onDestroy(() => {
    unlisten.forEach((u) => u());
    if (pollTimer) clearInterval(pollTimer);
    stopLogPolling();
  });

  async function loadServices() {
    try {
      const next = await invoke<DbService[]>('svc_list');
      // Keep transient UI state while an operation runs.
      services = next.map((s) => (busy[s.id] === 'starting' || busy[s.id] === 'restarting' ? { ...s, state: 'starting' as State } : busy[s.id] === 'stopping' ? { ...s, state: 'stopping' as State } : s));
      if (selectedId && !services.some((s) => s.id === selectedId)) selectedId = services[0]?.id ?? null;
    } catch (e) {
      console.warn('svc_list failed', e);
    }
  }

  async function loadVersions() {
    try {
      versions = await invoke<EngineVersionInfo[]>('svc_engine_versions');
    } catch {
      versions = [];
    }
  }

  function engineLabel(e: Engine) {
    return ENGINES.find((x) => x.key === e)?.label ?? e;
  }

  function fmtBytes(n: number) {
    if (!n) return '0 B';
    const u = ['B', 'KB', 'MB', 'GB'];
    const i = Math.min(u.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
    return `${(n / 1024 ** i).toFixed(i >= 2 ? 1 : 0)} ${u[i]}`;
  }

  function pct(p?: Progress) {
    if (!p || !p.total) return null;
    return Math.max(0, Math.min(100, Math.round((p.done / p.total) * 100)));
  }

  function connectionUrl(s: DbService) {
    switch (s.engine) {
      case 'postgres':
        return `postgresql://postgres@127.0.0.1:${s.port}/${s.database || 'postgres'}`;
      case 'mysql':
      case 'mariadb':
        return `mysql://root@127.0.0.1:${s.port}/${s.database}`;
      case 'redis':
        return `redis://127.0.0.1:${s.port}/0`;
    }
  }

  async function copy(text: string, label = 'Copied') {
    try {
      await navigator.clipboard.writeText(text);
      showToast(label, 'success');
    } catch {
      showToast('Clipboard unavailable', 'error');
    }
  }

  async function startSvc(s: DbService) {
    busy[s.id] = 'starting';
    patch(s.id, { state: 'starting', lastError: null });
    try {
      const updated = await invoke<DbService>('svc_start', { serviceId: s.id });
      showToast(`${updated.name} is running on port ${updated.port}`, 'success');
    } catch (e) {
      showToast(`${s.name}: ${firstLine(errorText(e))}`, 'error');
    } finally {
      busy[s.id] = undefined;
      await loadServices();
    }
  }

  async function stopSvc(s: DbService) {
    busy[s.id] = 'stopping';
    patch(s.id, { state: 'stopping' });
    try {
      await invoke('svc_stop', { serviceId: s.id });
      showToast(`${s.name} stopped`, 'info');
    } catch (e) {
      showToast(`${s.name}: ${firstLine(errorText(e))}`, 'error');
    } finally {
      busy[s.id] = undefined;
      await loadServices();
    }
  }

  async function restartSvc(s: DbService) {
    busy[s.id] = 'restarting';
    patch(s.id, { state: 'starting' });
    try {
      await invoke('svc_restart', { serviceId: s.id });
      showToast(`${s.name} restarted`, 'success');
    } catch (e) {
      showToast(`${s.name}: ${firstLine(errorText(e))}`, 'error');
    } finally {
      busy[s.id] = undefined;
      await loadServices();
    }
  }

  function toggle(s: DbService) {
    if (busy[s.id]) return;
    if (s.state === 'running') void stopSvc(s);
    else void startSvc(s);
  }

  async function stopAll() {
    if (!runningCount) return;
    try {
      const n = await invoke<number>('svc_stop_all');
      showToast(`Stopped ${n} service${n === 1 ? '' : 's'}`, 'info');
    } catch (e) {
      showToast(errorText(e), 'error');
    } finally {
      await loadServices();
    }
  }

  function patch(id: string, p: Partial<DbService>) {
    services = services.map((s) => (s.id === id ? { ...s, ...p } : s));
  }

  function firstLine(s: string) {
    return s.split('\n')[0];
  }

  async function saveEdits() {
    const s = selected;
    if (!s) return;
    const changes: Record<string, unknown> = { serviceId: s.id };
    if (editName.trim() && editName.trim() !== s.name) changes.name = editName.trim();
    if (editPort && editPort !== s.port) changes.port = Number(editPort);
    if (Object.keys(changes).length === 1) return;
    try {
      await invoke('svc_update', changes);
      showToast('Service updated', 'success');
    } catch (e) {
      showToast(errorText(e), 'error');
      editName = s.name;
      editPort = s.port;
    } finally {
      await loadServices();
    }
  }

  async function setAutoStart(s: DbService, v: boolean) {
    patch(s.id, { autoStart: v });
    try {
      await invoke('svc_update', { serviceId: s.id, autoStart: v });
    } catch (e) {
      showToast(errorText(e), 'error');
      await loadServices();
    }
  }

  async function removeSvc(s: DbService) {
    const ok = await confirmModal(
      `Delete "${s.name}"? The service will be stopped and its data directory permanently deleted. Engine binaries stay cached for other services.`,
      'Delete Service',
      true,
      'Delete',
      'Cancel'
    );
    if (!ok) return;
    try {
      await invoke('svc_remove', { serviceId: s.id, deleteData: true });
      showToast(`${s.name} deleted`, 'success');
      if (selectedId === s.id) selectedId = null;
    } catch (e) {
      showToast(errorText(e), 'error');
    } finally {
      await loadServices();
      if (!selectedId && services.length) selectedId = services[0].id;
    }
  }

  async function openInStudio(s: DbService) {
    if (s.state !== 'running') {
      showToast('Start the service first', 'info');
      return;
    }
    try {
      const conn = await invoke<ConnectionConfig>('svc_open_in_studio', { serviceId: s.id });
      await dbStore.connect(conn);
      showToast(`Connected to ${s.name}`, 'success');
      void goto(s.engine === 'redis' ? '/sql-studio' : '/table-browser');
    } catch (e) {
      showToast(`Could not connect: ${firstLine(errorText(e))}`, 'error');
    }
  }

  async function revealData(s: DbService) {
    try {
      await invoke('svc_reveal_data_dir', { serviceId: s.id });
    } catch (e) {
      showToast(errorText(e), 'error');
    }
  }

  // ---- Logs ----
  async function refreshLogs() {
    if (!selected) return;
    try {
      logsText = (await invoke<string>('svc_logs', { serviceId: selected.id, lines: 400 })) || '(log is empty)';
    } catch (e) {
      logsText = errorText(e);
    }
  }
  function openLogs() {
    logsOpen = true;
    void refreshLogs();
    stopLogPolling();
    logsTimer = setInterval(() => void refreshLogs(), 2000);
  }
  function stopLogPolling() {
    if (logsTimer) clearInterval(logsTimer);
    logsTimer = null;
  }
  function closeLogs() {
    logsOpen = false;
    stopLogPolling();
  }
  function scrollBottom(node: HTMLElement, _text: string) {
    node.scrollTop = node.scrollHeight;
    return {
      update() {
        node.scrollTop = node.scrollHeight;
      },
    };
  }

  // ---- New service ----
  async function openNew() {
    fName = '';
    fAutoStart = false;
    fStartNow = true;
    await pickEngine('postgres');
    modalOpen = true;
  }

  async function pickEngine(e: Engine) {
    fEngine = e;
    fVersion = versions.find((v) => v.engine === e)?.version ?? '';
    try {
      fPort = await invoke<number>('svc_suggest_port', { engine: e });
    } catch {
      fPort = e === 'postgres' ? 5432 : e === 'redis' ? 6379 : 3306;
    }
    if (!fName || ENGINES.some((x) => fName.startsWith(x.key))) {
      const n = services.filter((s) => s.engine === e).length + 1;
      fName = `${e}-${n}`;
    }
  }

  async function createSvc() {
    if (!fName.trim()) {
      showToast('Service name is required', 'error');
      return;
    }
    creating = true;
    try {
      const svc = await invoke<DbService>('svc_create', {
        engine: fEngine,
        version: fVersion,
        name: fName.trim(),
        preferredPort: Number(fPort),
        autoStart: fAutoStart,
      });
      modalOpen = false;
      await loadServices();
      selectedId = svc.id;
      if (fStartNow) void startSvc(svc);
      else showToast(`${svc.name} created`, 'success');
    } catch (e) {
      showToast(errorText(e), 'error');
    } finally {
      creating = false;
    }
  }

  function stateText(s: DbService) {
    const b = busy[s.id];
    if (b === 'restarting') return 'Restarting…';
    switch (s.state) {
      case 'running': return 'Running';
      case 'starting': return 'Starting…';
      case 'stopping': return 'Stopping…';
      case 'failing': return 'Failed';
      default: return 'Stopped';
    }
  }

  function dotClass(st: State) {
    switch (st) {
      case 'running': return 'bg-emerald-500 shadow-[0_0_0_3px_rgba(16,185,129,0.18)]';
      case 'starting':
      case 'stopping': return 'bg-amber-400 animate-pulse';
      case 'failing': return 'bg-rose-500';
      default: return 'bg-neutral-400 dark:bg-neutral-600';
    }
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key === 'Escape') { modalOpen = false; closeLogs(); } }} />

<div class="h-full flex flex-col -mx-4 -my-6 md:-mx-8 md:-my-8 min-h-[calc(100vh-3rem)]">
  <!-- Toolbar -->
  <div class="flex items-center gap-3 px-5 h-14 border-b border-neutral-200 dark:border-neutral-800 bg-white/70 dark:bg-[#161616]/80 backdrop-blur shrink-0">
    <div class="flex items-center gap-2.5 min-w-0">
      <div class="w-8 h-8 rounded-lg bg-emerald-500/10 text-emerald-500 border border-emerald-500/20 flex items-center justify-center">
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01" /></svg>
      </div>
      <div class="min-w-0">
        <h1 class="text-sm font-bold tracking-tight text-neutral-900 dark:text-white leading-tight">Local Services</h1>
        <p class="text-[11px] text-neutral-500 dark:text-neutral-400 leading-tight">
          {services.length} service{services.length === 1 ? '' : 's'} · <span class="text-emerald-600 dark:text-emerald-400">{runningCount} running</span>
        </p>
      </div>
    </div>
    <div class="flex-1"></div>
    <div class="relative hidden sm:block">
      <input
        bind:value={search}
        placeholder="Search services…"
        class="w-48 text-xs pl-7 pr-2 py-1.5 rounded-lg bg-neutral-100 dark:bg-neutral-800/80 border border-transparent focus:border-blue-500/60 focus:outline-none text-neutral-800 dark:text-neutral-200"
      />
      <svg class="w-3.5 h-3.5 absolute left-2 top-2 text-neutral-400" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" /></svg>
    </div>
    <button
      type="button"
      onclick={stopAll}
      disabled={!runningCount}
      class="px-3 py-1.5 text-xs font-semibold rounded-lg border border-neutral-200 dark:border-neutral-700 text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
    >
      Stop all
    </button>
    <button
      type="button"
      onclick={openNew}
      class="px-3 py-1.5 text-xs font-semibold text-white bg-emerald-600 hover:bg-emerald-500 active:scale-[0.98] rounded-lg shadow-sm transition-[background-color,transform] flex items-center gap-1.5"
    >
      <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M12 4v16m8-8H4" /></svg>
      New Service
    </button>
  </div>

  <div class="flex-1 flex min-h-0">
    <!-- Service list -->
    <section class="w-full md:w-[360px] shrink-0 border-r border-neutral-200 dark:border-neutral-800 overflow-y-auto scrollbar-thin {selected ? 'hidden md:block' : ''}">
      {#if !loaded}
        <div class="p-3 space-y-2">
          {#each [0, 1, 2] as _}
            <div class="h-[68px] rounded-xl bg-neutral-100 dark:bg-neutral-800/50 animate-pulse"></div>
          {/each}
        </div>
      {:else if services.length === 0}
        <div class="p-8 text-center" in:fade={{ duration: 150 }}>
          <div class="w-12 h-12 mx-auto rounded-2xl bg-emerald-500/10 text-emerald-500 flex items-center justify-center mb-3">
            <svg class="w-6 h-6" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.6" d="M4 7c0-1.7 3.6-3 8-3s8 1.3 8 3-3.6 3-8 3-8-1.3-8-3zm0 0v10c0 1.7 3.6 3 8 3s8-1.3 8-3V7M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3" /></svg>
          </div>
          <p class="text-sm font-semibold text-neutral-800 dark:text-neutral-200">No local services yet</p>
          <p class="text-xs text-neutral-500 dark:text-neutral-400 mt-1 mb-4">Spin up PostgreSQL, MySQL, MariaDB or Redis in one click — no installer, no admin rights.</p>
          <button type="button" onclick={openNew} class="px-3 py-1.5 text-xs font-semibold text-white bg-emerald-600 hover:bg-emerald-500 rounded-lg">Create your first service</button>
        </div>
      {:else}
        <ul class="p-2 space-y-1">
          {#each filtered as s (s.id)}
            {@const p = progress[s.id]}
            {@const pp = pct(p)}
            <li>
              <div
                role="button"
                tabindex="0"
                onclick={() => (selectedId = s.id)}
                onkeydown={(e) => e.key === 'Enter' && (selectedId = s.id)}
                class="group relative w-full flex items-center gap-3 px-3 py-2.5 rounded-xl border transition-colors duration-100 cursor-pointer {selectedId === s.id ? 'bg-blue-500/10 border-blue-500/30' : 'border-transparent hover:bg-neutral-100 dark:hover:bg-neutral-800/60'}"
              >
                <div class="relative shrink-0">
                  <DatabaseIcon driver={s.engine} size={28} />
                  <span class="absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5 rounded-full ring-2 ring-white dark:ring-[#161616] {dotClass(s.state)}"></span>
                </div>
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-1.5">
                    <span class="text-[13px] font-semibold text-neutral-900 dark:text-neutral-100 truncate">{s.name}</span>
                    {#if s.autoStart}
                      <span class="text-[9px] font-bold uppercase tracking-wide px-1 rounded bg-sky-500/10 text-sky-500" title="Starts with CATable">auto</span>
                    {/if}
                  </div>
                  <div class="text-[11px] text-neutral-500 dark:text-neutral-400 font-mono truncate">
                    {engineLabel(s.engine)} {s.version} · :{s.port}
                  </div>
                  {#if p && p.phase !== 'ready'}
                    <div class="mt-1.5" transition:slide={{ duration: 150 }}>
                      <div class="h-1 rounded-full bg-neutral-200 dark:bg-neutral-800 overflow-hidden">
                        {#if pp !== null}
                          <div class="h-full bg-emerald-500 rounded-full transition-[width] duration-200 ease-out" style:width="{pp}%"></div>
                        {:else}
                          <div class="h-full w-1/3 bg-emerald-500/80 rounded-full indeterminate"></div>
                        {/if}
                      </div>
                      <div class="text-[10px] text-neutral-500 mt-0.5 truncate">
                        {p.message}{#if p.phase === 'download' && p.total} · {fmtBytes(p.done)} / {fmtBytes(p.total)}{/if}
                      </div>
                    </div>
                  {/if}
                </div>
                <!-- DBngin-style switch -->
                <button
                  type="button"
                  role="switch"
                  aria-checked={s.state === 'running'}
                  aria-label={s.state === 'running' ? `Stop ${s.name}` : `Start ${s.name}`}
                  onclick={(e) => { e.stopPropagation(); toggle(s); }}
                  disabled={!!busy[s.id]}
                  class="relative shrink-0 w-10 h-[22px] rounded-full transition-colors duration-200 disabled:opacity-70 {s.state === 'running' ? 'bg-emerald-500' : s.state === 'starting' || s.state === 'stopping' ? 'bg-amber-400' : 'bg-neutral-300 dark:bg-neutral-700'}"
                >
                  <span class="absolute top-[3px] left-[3px] w-4 h-4 rounded-full bg-white shadow transition-transform duration-200 ease-out {s.state === 'running' || s.state === 'starting' ? 'translate-x-[18px]' : ''}"></span>
                </button>
              </div>
            </li>
          {/each}
          {#if filtered.length === 0}
            <li class="text-xs text-neutral-500 text-center py-6">No services match “{search}”.</li>
          {/if}
        </ul>
      {/if}
    </section>

    <!-- Detail panel -->
    <section class="flex-1 min-w-0 overflow-y-auto scrollbar-thin {selected ? '' : 'hidden md:block'}">
      {#if selected}
        {@const s = selected}
        {#key s.id}
          <div class="p-6 max-w-3xl space-y-6" in:fade={{ duration: 120 }}>
            <button type="button" class="md:hidden text-xs text-blue-500" onclick={() => (selectedId = null)}>← All services</button>
            <!-- Header -->
            <div class="flex items-start gap-4">
              <DatabaseIcon driver={s.engine} size={44} />
              <div class="min-w-0 flex-1">
                <h2 class="text-lg font-bold text-neutral-900 dark:text-white truncate">{s.name}</h2>
                <div class="flex items-center gap-2 text-xs text-neutral-500 dark:text-neutral-400 mt-0.5">
                  <span>{engineLabel(s.engine)} {s.version}</span>
                  <span>·</span>
                  <span class="inline-flex items-center gap-1.5">
                    <span class="w-2 h-2 rounded-full {dotClass(s.state)}"></span>{stateText(s)}
                  </span>
                  {#if s.pid && s.state === 'running'}<span class="font-mono opacity-70">PID {s.pid}</span>{/if}
                </div>
              </div>
              <div class="flex items-center gap-2 shrink-0">
                {#if s.state === 'running'}
                  <button type="button" onclick={() => stopSvc(s)} disabled={!!busy[s.id]} class="btn-secondary">Stop</button>
                  <button type="button" onclick={() => restartSvc(s)} disabled={!!busy[s.id]} class="btn-secondary">Restart</button>
                {:else}
                  <button type="button" onclick={() => startSvc(s)} disabled={!!busy[s.id]} class="btn-primary bg-emerald-600 hover:bg-emerald-500">
                    {#if busy[s.id]}<span class="spinner"></span>{/if}
                    Start
                  </button>
                {/if}
                <button type="button" onclick={() => openInStudio(s)} disabled={s.state !== 'running'} class="btn-primary bg-blue-600 hover:bg-blue-500">Open in Studio</button>
              </div>
            </div>

            {#if s.state === 'failing' && s.lastError}
              <div class="rounded-xl border border-rose-500/30 bg-rose-500/5 p-3" transition:slide={{ duration: 150 }}>
                <div class="text-xs font-semibold text-rose-600 dark:text-rose-400 mb-1">Service failed to start</div>
                <pre class="text-[11px] font-mono whitespace-pre-wrap text-rose-700/90 dark:text-rose-300/90 max-h-40 overflow-auto scrollbar-thin">{s.lastError}</pre>
                <button type="button" class="mt-2 text-[11px] font-semibold text-rose-600 dark:text-rose-400 hover:underline" onclick={openLogs}>Open full log →</button>
              </div>
            {/if}

            <!-- Connection card -->
            <div class="rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900/60 overflow-hidden">
              <div class="px-4 py-2.5 border-b border-neutral-200 dark:border-neutral-800 text-[11px] font-bold uppercase tracking-wider text-neutral-500">Connection</div>
              <dl class="grid grid-cols-[120px_1fr] text-xs">
                {#each [
                  ['Host', '127.0.0.1'],
                  ['Port', String(s.port)],
                  ...(s.engine !== 'redis' ? [['User', s.engine === 'postgres' ? 'postgres' : 'root'], ['Password', '(none — local trust)'], ['Database', s.database || (s.engine === 'postgres' ? 'postgres' : '')]] : []),
                ] as [k, v]}
                  <dt class="px-4 py-2 text-neutral-500 border-b border-neutral-100 dark:border-neutral-800/70">{k}</dt>
                  <dd class="px-4 py-2 font-mono text-neutral-800 dark:text-neutral-200 border-b border-neutral-100 dark:border-neutral-800/70 flex items-center justify-between gap-2">
                    <span class="truncate">{v}</span>
                    {#if k !== 'Password'}
                      <button type="button" class="copy-btn" onclick={() => copy(v, `${k} copied`)} aria-label="Copy {k}">Copy</button>
                    {/if}
                  </dd>
                {/each}
                <dt class="px-4 py-2 text-neutral-500">URL</dt>
                <dd class="px-4 py-2 font-mono text-neutral-800 dark:text-neutral-200 flex items-center justify-between gap-2">
                  <span class="truncate">{connectionUrl(s)}</span>
                  <button type="button" class="copy-btn" onclick={() => copy(connectionUrl(s), 'Connection URL copied')}>Copy</button>
                </dd>
              </dl>
            </div>

            <!-- Settings card -->
            <div class="rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900/60 overflow-hidden">
              <div class="px-4 py-2.5 border-b border-neutral-200 dark:border-neutral-800 text-[11px] font-bold uppercase tracking-wider text-neutral-500">Settings</div>
              <div class="p-4 grid grid-cols-1 sm:grid-cols-2 gap-4 text-xs">
                <label class="space-y-1">
                  <span class="block text-neutral-500 font-medium">Name</span>
                  <input bind:value={editName} onblur={saveEdits} onkeydown={(e) => e.key === 'Enter' && (e.currentTarget as HTMLInputElement).blur()} class="field" />
                </label>
                <label class="space-y-1">
                  <span class="block text-neutral-500 font-medium">Port {#if s.state === 'running'}<span class="text-neutral-400">(stop to change)</span>{/if}</span>
                  <input type="number" min="1024" max="65535" bind:value={editPort} disabled={s.state === 'running'} onblur={saveEdits} onkeydown={(e) => e.key === 'Enter' && (e.currentTarget as HTMLInputElement).blur()} class="field font-mono disabled:opacity-60" />
                </label>
                <label class="flex items-center justify-between gap-3 sm:col-span-2 py-1 cursor-pointer">
                  <span>
                    <span class="block font-medium text-neutral-800 dark:text-neutral-200">Start automatically</span>
                    <span class="block text-neutral-500">Launch this service when CATable opens.</span>
                  </span>
                  <input type="checkbox" class="sr-only peer" checked={s.autoStart} onchange={(e) => setAutoStart(s, (e.currentTarget as HTMLInputElement).checked)} />
                  <span class="relative w-9 h-5 rounded-full bg-neutral-300 dark:bg-neutral-700 peer-checked:bg-sky-500 transition-colors after:content-[''] after:absolute after:top-0.5 after:left-0.5 after:w-4 after:h-4 after:rounded-full after:bg-white after:shadow after:transition-transform peer-checked:after:translate-x-4"></span>
                </label>
              </div>
            </div>

            <!-- Maintenance -->
            <div class="flex flex-wrap items-center gap-2">
              <button type="button" class="btn-secondary" onclick={openLogs}>View logs</button>
              <button type="button" class="btn-secondary" onclick={() => revealData(s)}>Open data folder</button>
              <button type="button" class="btn-secondary" onclick={() => copy(s.dataDir, 'Data path copied')}>Copy data path</button>
              <div class="flex-1"></div>
              <button type="button" class="px-3 py-1.5 text-xs font-semibold rounded-lg text-rose-600 dark:text-rose-400 hover:bg-rose-500/10 transition-colors" onclick={() => removeSvc(s)}>Delete service</button>
            </div>
          </div>
        {/key}
      {:else if loaded && services.length > 0}
        <div class="h-full flex items-center justify-center text-xs text-neutral-500">Select a service</div>
      {/if}
    </section>
  </div>
</div>

<!-- Logs drawer -->
{#if logsOpen && selected}
  <div class="fixed inset-0 z-50 bg-black/40" transition:fade={{ duration: 120 }} onclick={closeLogs} role="presentation"></div>
  <aside class="fixed right-0 top-0 bottom-0 z-50 w-full max-w-2xl bg-white dark:bg-[#121212] border-l border-neutral-200 dark:border-neutral-800 shadow-2xl flex flex-col" transition:fly={{ x: 40, duration: 180, easing: cubicOut }}>
    <div class="h-12 px-4 flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 shrink-0">
      <div class="text-sm font-semibold text-neutral-900 dark:text-white">Logs · {selected.name}</div>
      <div class="flex items-center gap-2">
        <button type="button" class="btn-secondary" onclick={() => copy(logsText, 'Log copied')}>Copy</button>
        <button type="button" class="p-1.5 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-500" onclick={closeLogs} aria-label="Close logs">✕</button>
      </div>
    </div>
    <pre use:scrollBottom={logsText} class="flex-1 overflow-auto scrollbar-thin p-4 text-[11px] leading-relaxed font-mono text-neutral-700 dark:text-neutral-300 whitespace-pre-wrap select-text">{logsText}</pre>
  </aside>
{/if}

<!-- New Service modal -->
{#if modalOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4" transition:fade={{ duration: 120 }} onclick={(e) => e.target === e.currentTarget && (modalOpen = false)} role="presentation">
    <div class="w-full max-w-lg bg-white dark:bg-[#171717] border border-neutral-200 dark:border-neutral-800 rounded-2xl shadow-2xl overflow-hidden" in:scale={{ start: 0.96, duration: 160, easing: cubicOut }}>
      <div class="px-5 py-4 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between">
        <h3 class="text-sm font-bold text-neutral-900 dark:text-white">New Local Service</h3>
        <button type="button" onclick={() => (modalOpen = false)} class="p-1 rounded text-neutral-400 hover:text-neutral-900 dark:hover:text-white" aria-label="Close">✕</button>
      </div>
      <div class="p-5 space-y-4 text-xs">
        <div class="grid grid-cols-2 gap-2">
          {#each ENGINES as e}
            <button
              type="button"
              onclick={() => pickEngine(e.key)}
              class="flex items-center gap-2.5 p-2.5 rounded-xl border text-left transition-colors {fEngine === e.key ? 'border-emerald-500 bg-emerald-500/10' : 'border-neutral-200 dark:border-neutral-700 hover:border-neutral-400 dark:hover:border-neutral-500'}"
            >
              <DatabaseIcon driver={e.key} size={26} />
              <span class="min-w-0">
                <span class="block font-semibold text-neutral-900 dark:text-neutral-100">{e.label}</span>
                <span class="block text-[10px] text-neutral-500 truncate">{e.blurb}</span>
              </span>
            </button>
          {/each}
        </div>

        <label class="block space-y-1">
          <span class="block font-medium text-neutral-500">Version</span>
          <select bind:value={fVersion} class="field">
            {#each versionsFor as v}
              <option value={v.version}>{v.version} {v.installed ? '· downloaded' : `· ${fmtBytes(v.sizeBytes)} download`}</option>
            {/each}
          </select>
        </label>

        <div class="grid grid-cols-[1fr_120px] gap-3">
          <label class="block space-y-1">
            <span class="block font-medium text-neutral-500">Name</span>
            <input bind:value={fName} placeholder="e.g. shop-dev" class="field" />
          </label>
          <label class="block space-y-1">
            <span class="block font-medium text-neutral-500">Port</span>
            <input type="number" min="1024" max="65535" bind:value={fPort} class="field font-mono" />
          </label>
        </div>

        <div class="space-y-2 pt-1">
          <label class="flex items-center gap-2 cursor-pointer"><input type="checkbox" bind:checked={fStartNow} class="accent-emerald-600" /> Start immediately</label>
          <label class="flex items-center gap-2 cursor-pointer"><input type="checkbox" bind:checked={fAutoStart} class="accent-emerald-600" /> Start automatically when CATable opens</label>
        </div>

        {#if !versionsFor.find((v) => v.version === fVersion)?.installed}
          <p class="text-[11px] text-neutral-500 bg-neutral-50 dark:bg-neutral-800/50 rounded-lg p-2.5">
            The official portable build is downloaded once over HTTPS and cached for every service using this version.
          </p>
        {/if}
      </div>
      <div class="px-5 py-3 border-t border-neutral-200 dark:border-neutral-800 flex justify-end gap-2 bg-neutral-50/60 dark:bg-neutral-900/40">
        <button type="button" onclick={() => (modalOpen = false)} class="btn-secondary">Cancel</button>
        <button type="button" onclick={createSvc} disabled={creating || !fVersion} class="btn-primary bg-emerald-600 hover:bg-emerald-500">
          {#if creating}<span class="spinner"></span>{/if}
          Create service
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .btn-primary {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.75rem;
    font-size: 0.75rem;
    font-weight: 600;
    color: white;
    border-radius: 0.5rem;
    transition: background-color 120ms ease, transform 120ms ease, opacity 120ms ease;
  }
  .btn-primary:active:not(:disabled) {
    transform: scale(0.98);
  }
  .btn-primary:disabled,
  .btn-secondary:disabled {
    opacity: 0.45;
    cursor: not-allowed !important;
  }
  .btn-secondary {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
    padding: 0.375rem 0.75rem;
    font-size: 0.75rem;
    font-weight: 600;
    border-radius: 0.5rem;
    border: 1px solid rgb(229 229 229);
    color: rgb(64 64 64);
    transition: background-color 120ms ease;
  }
  .btn-secondary:hover:not(:disabled) {
    background: rgb(245 245 245);
  }
  :global(.dark) .btn-secondary {
    border-color: rgb(64 64 64);
    color: rgb(212 212 212);
  }
  :global(.dark) .btn-secondary:hover:not(:disabled) {
    background: rgb(38 38 38);
  }
  .field {
    width: 100%;
    padding: 0.45rem 0.65rem;
    border-radius: 0.5rem;
    border: 1px solid rgb(229 229 229);
    background: rgb(250 250 250);
    color: rgb(23 23 23);
    outline: none;
    transition: border-color 120ms ease, box-shadow 120ms ease;
  }
  .field:focus {
    border-color: rgb(16 185 129);
    box-shadow: 0 0 0 3px rgb(16 185 129 / 0.15);
  }
  :global(.dark) .field {
    border-color: rgb(64 64 64);
    background: rgb(38 38 38);
    color: rgb(245 245 245);
  }
  .copy-btn {
    font-family: ui-sans-serif, system-ui, sans-serif;
    font-size: 10px;
    font-weight: 600;
    color: rgb(59 130 246);
    opacity: 0;
    transition: opacity 100ms ease;
  }
  dd:hover .copy-btn,
  .copy-btn:focus-visible {
    opacity: 1;
  }
  .spinner {
    width: 0.75rem;
    height: 0.75rem;
    border-radius: 9999px;
    border: 2px solid rgb(255 255 255 / 0.4);
    border-top-color: white;
    animation: spin 0.7s linear infinite;
  }
  .indeterminate {
    animation: indeterminate 1.1s ease-in-out infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes indeterminate {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(300%);
    }
  }
</style>
