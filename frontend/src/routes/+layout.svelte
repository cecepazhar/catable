<script lang="ts">
  import "../app.css";
  import NotificationCenter from '$lib/components/NotificationCenter.svelte';
  import AiChatPanel from '$lib/components/AiChatPanel.svelte';
  import { getAiChatState, toggleAiChat, closeAiChat } from '$lib/stores/aiChat.svelte';
  import { page } from '$app/state';
  import { getTheme, initTheme } from '$lib/stores/theme.svelte';
  import { getToasts, showToast, confirmModal } from '$lib/stores/uiNotifications.svelte';
  import ProfileMenu from '$lib/components/ProfileMenu.svelte';
  import FeedbackModal from '$lib/components/FeedbackModal.svelte';
  import CrashReportModal from '$lib/components/CrashReportModal.svelte';
  import { getPendingCrashReport, type ScrubbedCrashReport } from '$lib/api/crash';
  import { getFeedbackPromptState } from '$lib/stores/feedbackStore.svelte';
  import { checkForUpdates } from '$lib/stores/updater.svelte';
  import { APP_VERSION, APP_NAME } from '$lib/appInfo';
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import HeaderQuickControls from '$lib/components/HeaderQuickControls.svelte';
  import WindowControls from '$lib/components/WindowControls.svelte';
  import Logo from '$lib/components/Logo.svelte';
  import CommandPalette from '$lib/components/CommandPalette.svelte';
  import { getPalette } from '$lib/stores/commandPalette.svelte';
  import AmbientGlow from '$lib/components/AmbientGlow.svelte';
  import BottomNav from '$lib/components/BottomNav.svelte';
  import AllMenusSheet from '$lib/components/AllMenusSheet.svelte';
  import { getAmbientStore } from '$lib/stores/ambient.svelte';
  import { getPro } from '$lib/stores/pro.svelte';
  import { dbStore, type ConnectionConfig } from '$lib/stores/database.svelte';
  import DatabaseIcon from '$lib/components/DatabaseIcon.svelte';
  import { goto } from '$app/navigation';

  let isCollapsed = $state(false);
  let mobileDrawerOpen = $state(false);
  let allMenusSheetOpen = $state(false);
  let pendingCrashReport = $state<ScrubbedCrashReport | null>(null);
  let connectionSearch = $state('');
  let tableSearch = $state('');

  const theme = getTheme();
  const aiChat = getAiChatState();
  const feedbackPrompt = getFeedbackPromptState();
  const toasts = $derived(getToasts());
  const palette = getPalette();
  const ambient = getAmbientStore();
  const pro = getPro();

  const cardBorderClass = $derived.by(() => {
    if (!ambient.config.enabled || !ambient.config.cardGlowEnabled) {
      return 'border-t border-l border-neutral-200 dark:border-neutral-800';
    }
    if (ambient.config.cardGlowStyle === 'neon-border') {
      return 'border-t border-l border-sky-400 dark:border-sky-400 shadow-[0_0_15px_rgba(56,189,248,0.35),inset_0_0_10px_rgba(56,189,248,0.15)]';
    }
    if (ambient.config.cardGlowStyle === 'chroma-beam') {
      return 'border-t border-l border-sky-400/80 dark:border-sky-400/70 shadow-[0_0_12px_rgba(56,189,248,0.3)]';
    }
    return 'border-t border-l border-sky-400 dark:border-sky-400 shadow-[0_0_15px_rgba(56,189,248,0.35),inset_0_0_10px_rgba(56,189,248,0.15)]';
  });

  const filteredConnections = $derived(
    dbStore.connections.filter((c) =>
      c.name.toLowerCase().includes(connectionSearch.toLowerCase()) ||
      c.host.toLowerCase().includes(connectionSearch.toLowerCase()) ||
      c.driver.toLowerCase().includes(connectionSearch.toLowerCase())
    )
  );

  const filteredTables = $derived(
    dbStore.tables.filter((t) =>
      t.name.toLowerCase().includes(tableSearch.toLowerCase())
    )
  );

  onMount(async () => {
    initTheme();
    await dbStore.loadConnections();
    getPendingCrashReport()
      .then((report) => {
        pendingCrashReport = report;
      })
      .catch(() => {});
    checkForUpdates().catch(() => {});
  });

  function toggleSidebar() {
    isCollapsed = !isCollapsed;
  }

  function handleAiToggle() {
    toggleAiChat();
  }

  function startDragging(e: MouseEvent) {
    const target = e.target as HTMLElement | null;
    if (target?.closest('button, input, textarea, a, select, [role="button"], .no-drag')) {
      return;
    }
    invoke('window_start_dragging').catch(() => {});
  }

  function maximizeWindow() {
    invoke('window_maximize').catch(() => {});
  }

  function handleNewConnection() {
    dbStore.selectedConnectionId = 'new';
    if (page.url.pathname !== '/') {
      void goto('/');
    }
  }

  function selectConnection(c: ConnectionConfig) {
    dbStore.selectedConnectionId = c.id;
    if (page.url.pathname !== '/') {
      void goto('/');
    }
  }

  async function connectToDb(c: ConnectionConfig) {
    await dbStore.connect(c);
    showToast(`Connected to "${c.name}"`, 'success');
    void goto('/table-browser');
  }

  function handleDisconnect() {
    dbStore.disconnect();
    void goto('/');
  }

  async function deleteConnFromSidebar(conn: ConnectionConfig) {
    const ok = await confirmModal(
      `Delete connection "${conn.name}"?`,
      'Delete Connection',
      true,
      'Delete',
      'Cancel'
    );
    if (!ok) return;
    try {
      await dbStore.deleteConnection(conn.id);
      showToast(`Connection "${conn.name}" deleted`, 'success');
    } catch (e: any) {
      showToast(e?.message || 'Failed to delete connection', 'error');
    }
  }

  let { children } = $props();
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape') {
      mobileDrawerOpen = false;
      allMenusSheetOpen = false;
      closeAiChat();
    }
  }}
/>

<NotificationCenter />
<CommandPalette onLock={() => {}} />

<!-- Shell: Direct Launch, No LockScreen / No Login gatekeeper -->
<div class="flex flex-col h-screen w-screen overflow-hidden bg-neutral-100 dark:bg-[#0e0e0e] text-neutral-800 dark:text-neutral-300 font-sans transition-colors duration-150 pb-[env(safe-area-inset-bottom,0px)] select-none">
  
  <!-- Title Bar: Frameless top header across the window -->
  <header
    class="min-h-[3rem] h-[calc(3rem+env(safe-area-inset-top,0px))] pt-[env(safe-area-inset-top,0px)] flex items-center gap-1 md:gap-2 pl-2 pr-1 md:pl-3 shrink-0 select-none cursor-default"
    data-tauri-drag-region
    onmousedown={startDragging}
    ondblclick={(e) => {
      const target = e.target as HTMLElement | null;
      if (!target?.closest('button, input, textarea, a, select, [role="button"], .no-drag')) {
        maximizeWindow();
      }
    }}
  >
    <!-- Left: Breadcrumb / Connection Status -->
    <div class="flex items-center gap-2 min-w-0 flex-1" data-tauri-drag-region>
      <button
        type="button"
        onclick={() => (mobileDrawerOpen = true)}
        class="md:hidden p-1 rounded text-neutral-600 dark:text-neutral-300 hover:text-neutral-900 dark:hover:text-white hover:bg-neutral-200/70 dark:hover:bg-neutral-800 transition-colors shrink-0"
        title="Buka Navigasi"
        aria-label="Buka Navigasi"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6h16M4 12h16M4 18h16" />
        </svg>
      </button>

      <div class="flex gap-2 text-xs items-center overflow-x-auto scrollbar-none min-w-0 flex-1" data-tauri-drag-region>
        <a href="/" class="flex items-center gap-1.5 font-bold text-neutral-900 dark:text-white shrink-0 hover:opacity-80 transition-opacity">
          <Logo size={18} mode="brand" />
          <span class="tracking-tight">{APP_NAME}</span>
        </a>
        <span class="text-neutral-400 dark:text-neutral-600">/</span>
        {#if dbStore.isConnected}
          <div class="flex items-center gap-1.5 font-medium text-emerald-600 dark:text-emerald-400 bg-emerald-50 dark:bg-emerald-950/40 px-2 py-0.5 rounded border border-emerald-200 dark:border-emerald-800/60 truncate">
            <span class="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse"></span>
            <span class="truncate">{dbStore.activeConnection.name}</span>
            <span class="text-[10px] font-mono uppercase opacity-75">({dbStore.activeConnection.driver})</span>
          </div>
          {#if dbStore.activeTable && page.url.pathname === '/table-browser'}
            <span class="text-neutral-400 dark:text-neutral-600">/</span>
            <span class="font-mono text-neutral-700 dark:text-neutral-300">{dbStore.activeTable}</span>
          {/if}
        {:else if page.url.pathname === '/settings'}
          <span class="font-medium text-neutral-800 dark:text-neutral-200">Settings</span>
        {:else}
          <span class="font-medium text-neutral-500 dark:text-neutral-400">Connection Manager (Antares Mode)</span>
        {/if}
      </div>
    </div>

    <!-- Center: Draggable blank area -->
    <div class="flex-1 h-full min-w-[20px]" data-tauri-drag-region></div>

    <!-- Right: Header Controls -->
    <div class="flex items-center gap-1.5 text-neutral-500 dark:text-neutral-400 shrink-0" data-tauri-drag-region>
      <!-- Cyberpunk AI Copilot Bot Button -->
      <button
        type="button"
        onclick={handleAiToggle}
        class="p-1.5 rounded-md transition-colors relative flex items-center justify-center cursor-pointer {aiChat.open ? 'bg-sky-500/20 text-sky-400 border border-sky-500/40 shadow-[0_0_10px_rgba(56,189,248,0.25)]' : 'hover:text-neutral-900 dark:hover:text-white hover:bg-neutral-200/70 dark:hover:bg-neutral-800'}"
        title="AI Copilot Studio"
        aria-label="AI Copilot Studio"
      >
        <!-- Cyberpunk Bot Icon -->
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M9.75 3.104v1.272c-2.45.312-4.43 2.128-4.71 4.544H4.25a1.25 1.25 0 000 2.5h.79c.28 2.416 2.26 4.232 4.71 4.544v1.272a2 2 0 002 2h.5a2 2 0 002-2v-1.272c2.45-.312 4.43-2.128 4.71-4.544h.79a1.25 1.25 0 000-2.5h-.79c-.28-2.416-2.26-4.232-4.71-4.544V3.104a.75.75 0 00-1.5 0v.932h-.5V3.104a.75.75 0 00-1.5 0z" />
          <circle cx="9" cy="10.5" r="1.2" fill="currentColor" />
          <circle cx="15" cy="10.5" r="1.2" fill="currentColor" />
          <path stroke-linecap="round" stroke-width="1.5" d="M9.5 13.5h5" />
        </svg>
        {#if aiChat.open}
          <span class="absolute top-1 right-1 w-1.5 h-1.5 rounded-full bg-sky-400"></span>
        {:else}
          <span class="absolute -top-0.5 -right-0.5 w-1.5 h-1.5 rounded-full bg-emerald-500"></span>
        {/if}
      </button>

      <HeaderQuickControls
        layout={1}
        showFiles={false}
        aiOpen={aiChat.open}
        sessionCount={1}
        splitOptions={[]}
        onSetLayout={() => {}}
        onToggleFiles={() => {}}
        onToggleAi={handleAiToggle}
      />

      <button
        type="button"
        onclick={() => showToast('Tidak ada notifikasi baru', 'info')}
        class="p-1.5 hover:text-neutral-900 dark:hover:text-white hover:bg-neutral-200/70 dark:hover:bg-neutral-800 rounded-md transition-colors relative"
        title="Notifikasi"
        aria-label="Notifikasi"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9" />
        </svg>
      </button>

      <div class="pl-1 border-l border-neutral-200 dark:border-neutral-800">
        <WindowControls />
      </div>
    </div>
  </header>

  <!-- App Body: Two-State Dynamic Sidebar + Main Content -->
  <div class="flex-1 flex min-h-0 relative overflow-visible">
    
    <!-- Desktop Sidebar (Antares / TablePlus Dynamic Mode) -->
    <aside
      class="hidden md:flex flex-col shrink-0 bg-transparent {isCollapsed ? 'w-14' : 'w-60'} transition-all duration-150"
      aria-label="Sidebar Navigasi Database"
    >
      {#if !dbStore.isConnected}
        <!-- ============================================== -->
        <!-- ANTARES SQL STYLE: CONNECTION MANAGER SIDEBAR -->
        <!-- ============================================== -->
        <div class="h-12 flex items-center justify-between px-3.5 shrink-0">
          <div class="flex items-center gap-2 min-w-0">
            <span class="text-xs font-bold uppercase tracking-wider text-neutral-400">
              {#if !isCollapsed}Connections{/if}
            </span>
            {#if !isCollapsed}
              <span class="text-[10px] font-mono px-1.5 py-0.5 rounded bg-neutral-800/80 text-neutral-400 border border-neutral-700/60 font-semibold">
                {dbStore.connections.length}
              </span>
            {/if}
          </div>
          <button
            type="button"
            onclick={toggleSidebar}
            class="p-1.5 rounded-lg text-neutral-400 hover:text-white hover:bg-neutral-800/80 transition-colors cursor-pointer shrink-0"
            title={isCollapsed ? "Expand Sidebar" : "Collapse Sidebar"}
            aria-label={isCollapsed ? "Expand Sidebar" : "Collapse Sidebar"}
          >
            {#if isCollapsed}»{:else}«{/if}
          </button>
        </div>

        <!-- Workspaces Navigation Item -->
        <div class="px-2.5 pb-2">
          <a
            href="/groups"
            class="flex items-center justify-between px-2.5 py-2 rounded-xl text-xs font-semibold transition-colors {page.url.pathname === '/groups' ? 'bg-blue-600 text-white shadow-xs' : 'text-neutral-400 hover:text-white hover:bg-neutral-800/60'}"
          >
            <div class="flex items-center gap-2.5">
              <svg class="w-4 h-4 text-blue-500 {page.url.pathname === '/groups' ? 'text-white' : ''}" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
              </svg>
              {#if !isCollapsed}<span>Workspaces</span>{/if}
            </div>
            {#if !isCollapsed}
              <span class="text-[10px] font-mono opacity-80 bg-neutral-800 px-1.5 py-0.5 rounded text-neutral-400">
                Manage →
              </span>
            {/if}
          </a>
        </div>

        <!-- Connection Search Filter -->
        <div class="px-2.5 pb-2">
          <div class="relative">
            <input
              type="text"
              bind:value={connectionSearch}
              placeholder="Filter connections..."
              class="w-full text-xs pl-7 pr-2 py-1.5 bg-white dark:bg-neutral-800/90 rounded-xl border border-neutral-200 dark:border-neutral-700/80 focus:outline-none focus:ring-1 focus:ring-blue-500 text-neutral-800 dark:text-neutral-200"
            />
            <svg class="w-3.5 h-3.5 absolute left-2 top-2 text-neutral-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
          </div>
        </div>

        <!-- Connection List -->
        <div class="flex-1 overflow-y-auto {isCollapsed ? 'px-1' : 'px-2.5'} py-1 space-y-1 scrollbar-thin">
          {#each filteredConnections as conn (conn.id)}
            {#if isCollapsed}
              <button
                type="button"
                onclick={() => selectConnection(conn)}
                ondblclick={() => connectToDb(conn)}
                class="w-full flex items-center justify-center p-2 rounded-xl transition-all border {dbStore.selectedConnectionId === conn.id && page.url.pathname === '/' ? 'bg-sky-500/15 border-sky-500/40 text-sky-400 shadow-[0_0_8px_rgba(56,189,248,0.2)]' : 'border-transparent hover:bg-neutral-800/60'}"
                title="{conn.name} ({conn.driver})"
              >
                <DatabaseIcon driver={conn.driver} size={20} />
              </button>
            {:else}
              <div
                role="button"
                tabindex="0"
                onclick={() => selectConnection(conn)}
                ondblclick={() => connectToDb(conn)}
                onkeydown={(e) => e.key === 'Enter' && selectConnection(conn)}
                class="group relative w-full text-left p-2.5 rounded-xl cursor-pointer transition-all border {dbStore.selectedConnectionId === conn.id && page.url.pathname === '/' ? 'bg-sky-500/10 border-sky-500/40 shadow-xs' : 'border-transparent hover:bg-neutral-800/50'}"
              >
                <div class="flex items-center justify-between gap-1 mb-1">
                  <div class="flex items-center gap-2 min-w-0">
                    <DatabaseIcon driver={conn.driver} size={15} />
                    <span class="font-semibold text-xs text-neutral-900 dark:text-white truncate">{conn.name}</span>
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    {#if conn.ssh_tunnel?.enabled}
                      <span class="px-1.5 py-0.2 rounded text-[9px] font-mono font-bold bg-sky-500/10 text-sky-400 border border-sky-500/30">SSH</span>
                    {/if}
                    <button
                      type="button"
                      onclick={(e) => {
                        e.stopPropagation();
                        void deleteConnFromSidebar(conn);
                      }}
                      class="opacity-0 group-hover:opacity-100 p-0.5 rounded hover:bg-rose-950/60 text-neutral-400 hover:text-rose-500 transition-opacity"
                      title="Delete Connection"
                    >
                      <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                      </svg>
                    </button>
                  </div>
                </div>
                <div class="flex items-center justify-between gap-1 text-[10px] text-neutral-500 dark:text-neutral-400 font-mono">
                  <span class="truncate">{conn.driver === 'sqlite' || conn.driver === 'duckdb' ? conn.database : `${conn.user}@${conn.host}:${conn.port}`}</span>
                  {#if conn.group_name}
                    <span class="text-neutral-400 truncate max-w-[65px]">[{conn.group_name}]</span>
                  {/if}
                </div>
              </div>
            {/if}
          {/each}
        </div>

      {:else}
        <!-- ============================================== -->
        <!-- TABLEPLUS STYLE: ACTIVE DATABASE WORKSPACE    -->
        <!-- ============================================== -->
        <div class="mx-2.5 my-2 p-2 rounded-xl border border-neutral-800 bg-neutral-900/60 flex items-center justify-between gap-2">
          <div class="flex items-center gap-2.5 min-w-0">
            <DatabaseIcon driver={dbStore.activeConnection.driver} size={18} withBadge={true} badgeSize="sm" />
            <div class="min-w-0">
              <div class="text-xs font-bold text-neutral-900 dark:text-white truncate">{dbStore.activeConnection.name}</div>
              <div class="text-[10px] text-neutral-400 uppercase font-mono">{dbStore.activeConnection.driver}</div>
            </div>
          </div>
          <button
            onclick={handleDisconnect}
            class="text-[11px] font-medium text-rose-500 hover:text-rose-400 hover:bg-rose-950/40 px-2 py-1 rounded transition-colors shrink-0 cursor-pointer"
            title="Disconnect & Return to Connections"
          >
            Switch
          </button>
        </div>

        <!-- Workspace Nav Tabs -->
        <div class="px-2.5 py-1 space-y-0.5 text-xs">
          <a
            href="/table-browser"
            class="flex items-center gap-2.5 px-2.5 py-1.5 rounded-lg transition-colors {page.url.pathname === '/table-browser' ? 'bg-blue-600 text-white font-medium' : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/50 dark:hover:bg-neutral-800'}"
          >
            <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 10h18M3 14h18m-9-4v8m-7 4h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" /></svg>
            <span>Table Browser</span>
          </a>
          <a
            href="/sql-studio"
            class="flex items-center gap-2.5 px-2.5 py-1.5 rounded-lg transition-colors {page.url.pathname === '/sql-studio' ? 'bg-blue-600 text-white font-medium' : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/50 dark:hover:bg-neutral-800'}"
          >
            <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" /></svg>
            <span>SQL Studio</span>
          </a>
          <a
            href="/schema-erd"
            class="flex items-center gap-2.5 px-2.5 py-1.5 rounded-lg transition-colors {page.url.pathname === '/schema-erd' ? 'bg-blue-600 text-white font-medium' : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/50 dark:hover:bg-neutral-800'}"
          >
            <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6z M14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6z M9 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z" /></svg>
            <span>Schema ERD</span>
          </a>
          <a
            href="/ai-copilot"
            class="flex items-center gap-2.5 px-2.5 py-1.5 rounded-lg transition-colors {page.url.pathname === '/ai-copilot' ? 'bg-blue-600 text-white font-medium' : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/50 dark:hover:bg-neutral-800'}"
          >
            <svg class="w-4 h-4 text-rose-500" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09z" /></svg>
            <span>AI Copilot</span>
          </a>
        </div>

        <!-- Table Search -->
        <div class="px-2 pt-2 pb-1 border-b border-neutral-200 dark:border-neutral-800/60">
          <div class="text-[10px] font-bold text-neutral-400 uppercase tracking-wider mb-1 px-1">Tables ({dbStore.tables.length})</div>
          <div class="relative">
            <input
              type="text"
              bind:value={tableSearch}
              placeholder="Filter tables..."
              class="w-full text-xs pl-7 pr-2 py-1 bg-white dark:bg-neutral-800/90 rounded-lg border border-neutral-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-blue-500 text-neutral-800 dark:text-neutral-200"
            />
            <svg class="w-3 h-3 absolute left-2.5 top-2 text-neutral-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
          </div>
        </div>

        <!-- Table List -->
        <div class="flex-1 overflow-y-auto p-1.5 space-y-0.5 text-xs scrollbar-thin">
          {#each filteredTables as tbl (tbl.name)}
            <button
              onclick={() => {
                dbStore.activeTable = tbl.name;
                void goto('/table-browser');
              }}
              class="w-full text-left px-2.5 py-1.5 rounded-lg flex items-center justify-between transition-colors {dbStore.activeTable === tbl.name && page.url.pathname === '/table-browser' ? 'bg-blue-50 dark:bg-blue-900/40 text-blue-600 dark:text-blue-400 font-medium' : 'text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/50 dark:hover:bg-neutral-800'}"
            >
              <div class="flex items-center gap-2 min-w-0">
                <svg class="w-3.5 h-3.5 shrink-0 opacity-70" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 10h18M3 14h18m-9-4v8m-7 4h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" /></svg>
                <span class="truncate">{tbl.name}</span>
              </div>
              {#if tbl.row_count !== undefined}
                <span class="text-[10px] font-mono text-neutral-400 shrink-0">{tbl.row_count}</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}

      <!-- Bottom User Profile Card (CATerm Standard) -->
      <div class="p-2 border-t border-neutral-200 dark:border-neutral-800/80">
        <ProfileMenu collapsed={isCollapsed} onLock={() => {}} onSignOut={() => {}} onOpenAbout={() => {}} />
      </div>
    </aside>

    <!-- Content panel with Outer Ambient Halo Underglow -->
    <div class="flex-1 min-w-0 flex relative overflow-visible">
      <AmbientGlow defaultAccent="#0ea5e9" />

      <main class="flex-1 min-w-0 flex overflow-hidden relative z-10 bg-white dark:bg-[#161616] {cardBorderClass} rounded-tl-2xl transition-colors duration-150">
        <div class="flex-1 min-w-0 overflow-y-auto overflow-x-hidden text-neutral-900 dark:text-neutral-100 relative px-4 py-6 md:px-8 md:py-8 bg-[linear-gradient(to_right,#0000000a_1px,transparent_1px),linear-gradient(to_bottom,#0000000a_1px,transparent_1px)] dark:bg-[linear-gradient(to_right,#ffffff08_1px,transparent_1px),linear-gradient(to_bottom,#ffffff08_1px,transparent_1px)] bg-[size:56px_56px] z-20">
          {@render children()}
        </div>

        {#if aiChat.open}
          <AiChatPanel onClose={closeAiChat} />
        {/if}
      </main>
    </div>
  </div>

  <BottomNav onOpenAll={() => (allMenusSheetOpen = true)} />
  <AllMenusSheet bind:open={allMenusSheetOpen} onClose={() => (allMenusSheetOpen = false)} />

  {#if pendingCrashReport}
    <CrashReportModal
      report={pendingCrashReport}
      onClose={() => (pendingCrashReport = null)}
    />
  {/if}

  {#if feedbackPrompt.show}
    <FeedbackModal
      onClose={() => feedbackPrompt.close()}
      onSubmitted={() => feedbackPrompt.markSubmitted()}
    />
  {/if}
</div>
