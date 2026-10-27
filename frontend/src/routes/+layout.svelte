<script lang="ts">
  import "../app.css";
  import NotificationCenter from '$lib/components/NotificationCenter.svelte';
  import AiChatPanel from '$lib/components/AiChatPanel.svelte';
  import { getAiChatState, toggleAiChat, closeAiChat } from '$lib/stores/aiChat.svelte';
  import { page } from '$app/state';
  import { getTheme, initTheme } from '$lib/stores/theme.svelte';
  import { getToasts, showToast } from '$lib/stores/uiNotifications.svelte';
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
    if (!ambient.config.enabled || !ambient.config.cardGlowEnabled || !pro.isPro) {
      return 'border border-neutral-200 dark:border-neutral-800 md:border-b-0 md:border-r-0';
    }
    if (ambient.config.cardGlowStyle === 'neon-border') {
      return 'border border-sky-400 dark:border-sky-400 md:border-b-0 md:border-r-0 shadow-[inset_0_0_8px_rgba(56,189,248,0.15)]';
    }
    if (ambient.config.cardGlowStyle === 'chroma-beam') {
      return 'border border-sky-400/50 dark:border-sky-400/40 md:border-b-0 md:border-r-0';
    }
    return 'border border-sky-400/30 dark:border-sky-400/25 md:border-b-0 md:border-r-0';
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
<div class="flex flex-col h-screen bg-neutral-100 dark:bg-[#0e0e0e] text-neutral-800 dark:text-neutral-300 font-sans transition-colors duration-150 pb-[env(safe-area-inset-bottom,0px)] select-none">
  
  <!-- Title Bar: Frameless top header across the window -->
  <header
    class="min-h-[3rem] h-[calc(3rem+env(safe-area-inset-top,0px))] pt-[env(safe-area-inset-top,0px)] flex items-center gap-1 md:gap-2 pl-2 pr-1 md:pl-3 shrink-0 select-none cursor-default border-b border-neutral-200 dark:border-neutral-800/80"
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
  <div class="flex-1 flex overflow-hidden">
    
    <!-- Desktop Sidebar (Antares / TablePlus Dynamic Mode) -->
    <aside
      class="hidden md:flex flex-col shrink-0 border-r border-neutral-200 dark:border-neutral-800/80 bg-neutral-50/50 dark:bg-[#121212] {isCollapsed ? 'w-14' : 'w-64'} transition-all duration-150"
      aria-label="Sidebar Navigasi Database"
    >
      {#if !dbStore.isConnected}
        <!-- ============================================== -->
        <!-- ANTARES SQL STYLE: CONNECTION MANAGER SIDEBAR -->
        <!-- ============================================== -->
        <div class="p-2.5 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between gap-1">
          <span class="text-xs font-bold uppercase tracking-wider text-neutral-500 dark:text-neutral-400">Connections</span>
          <button
            onclick={handleNewConnection}
            class="flex items-center gap-1 px-2.5 py-1 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-colors cursor-pointer"
            title="Create New Connection"
          >
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4" />
            </svg>
            <span>New</span>
          </button>
        </div>

        <!-- Connection Search Filter -->
        <div class="p-2 border-b border-neutral-200 dark:border-neutral-800/60">
          <div class="relative">
            <input
              type="text"
              bind:value={connectionSearch}
              placeholder="Filter connections..."
              class="w-full text-xs pl-7 pr-2 py-1.5 bg-white dark:bg-neutral-800/90 rounded-lg border border-neutral-200 dark:border-neutral-700 focus:outline-none focus:ring-1 focus:ring-blue-500 text-neutral-800 dark:text-neutral-200"
            />
            <svg class="w-3.5 h-3.5 absolute left-2 top-2 text-neutral-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
          </div>
        </div>

        <!-- Connection List -->
        <div class="flex-1 overflow-y-auto p-2 space-y-1 scrollbar-thin">
          {#each filteredConnections as conn (conn.id)}
            <div
              role="button"
              tabindex="0"
              onclick={() => selectConnection(conn)}
              ondblclick={() => connectToDb(conn)}
              onkeydown={(e) => e.key === 'Enter' && selectConnection(conn)}
              class="w-full text-left p-2.5 rounded-xl cursor-pointer transition-all border {dbStore.selectedConnectionId === conn.id && page.url.pathname === '/' ? 'bg-blue-50 dark:bg-blue-950/40 border-blue-300 dark:border-blue-700 shadow-xs' : 'border-transparent hover:bg-neutral-200/50 dark:hover:bg-neutral-800/50'}"
            >
              <div class="flex items-center justify-between gap-1 mb-1">
                <span class="font-semibold text-xs text-neutral-900 dark:text-white truncate">{conn.name}</span>
                <div class="flex items-center gap-1 shrink-0">
                  {#if conn.group_name}
                    <span class="px-1.5 py-0.2 rounded text-[9px] font-mono bg-neutral-200/60 dark:bg-neutral-800 text-neutral-500 dark:text-neutral-400 truncate max-w-[70px]">{conn.group_name}</span>
                  {/if}
                  {#if conn.ssh_tunnel?.enabled}
                    <span class="px-1.5 py-0.2 rounded text-[9px] font-mono font-bold bg-sky-500/10 text-sky-500 border border-sky-500/30">SSH</span>
                  {/if}
                  <span class="px-1.5 py-0.2 rounded text-[9px] font-mono font-bold uppercase bg-neutral-200 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300">{conn.driver}</span>
                </div>
              </div>
              <div class="text-[10px] text-neutral-500 dark:text-neutral-400 truncate font-mono">
                {conn.driver === 'sqlite' || conn.driver === 'duckdb' ? conn.database : `${conn.user}@${conn.host}:${conn.port}`}
              </div>
            </div>
          {/each}
        </div>

      {:else}
        <!-- ============================================== -->
        <!-- TABLEPLUS STYLE: ACTIVE DATABASE WORKSPACE    -->
        <!-- ============================================== -->
        <div class="p-2.5 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-100/50 dark:bg-neutral-900/60 flex items-center justify-between gap-2">
          <div class="flex items-center gap-2 min-w-0">
            <span class="w-2 h-2 rounded-full bg-emerald-500 animate-pulse shrink-0"></span>
            <div class="min-w-0">
              <div class="text-xs font-bold text-neutral-900 dark:text-white truncate">{dbStore.activeConnection.name}</div>
              <div class="text-[10px] text-neutral-400 uppercase font-mono">{dbStore.activeConnection.driver}</div>
            </div>
          </div>
          <button
            onclick={handleDisconnect}
            class="text-[11px] font-medium text-rose-500 hover:text-rose-600 dark:hover:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950/40 px-2 py-1 rounded transition-colors shrink-0"
            title="Disconnect & Return to Connections"
          >
            Switch
          </button>
        </div>

        <!-- Workspace Nav Tabs -->
        <div class="p-2 border-b border-neutral-200 dark:border-neutral-800/80 space-y-0.5 text-xs">
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

      <!-- Bottom Settings Link -->
      <div class="p-2 border-t border-neutral-200 dark:border-neutral-800 flex items-center justify-between text-xs">
        <a
          href="/settings"
          class="flex items-center gap-2 text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-white px-2 py-1.5 rounded-lg hover:bg-neutral-200/50 dark:hover:bg-neutral-800 transition-colors w-full"
        >
          <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 001.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
          <span>Settings & License</span>
        </a>
      </div>
    </aside>

    <!-- Main Content Panel -->
    <div class="flex-1 min-w-0 flex relative overflow-hidden bg-white dark:bg-[#161616]">
      <AmbientGlow defaultAccent="#64748b" />
      
      <main class="flex-1 min-w-0 flex overflow-hidden relative z-10">
        <div class="flex-1 min-w-0 overflow-auto text-neutral-900 dark:text-neutral-100 relative">
          {@render children()}
        </div>

        {#if aiChat.open}
          <AiChatPanel onClose={closeAiChat} />
        {/if}
      </main>
    </div>
  </div>

  <!-- Floating Action Button: AI Copilot -->
  {#if !aiChat.open}
    <button
      type="button"
      onclick={() => handleAiToggle()}
      class="fixed bottom-20 right-4 md:bottom-5 md:right-5 z-40 flex items-center gap-2 px-3.5 py-2 rounded-full bg-neutral-900/90 hover:bg-neutral-800 text-neutral-100 border border-neutral-700/80 shadow-2xl shadow-black/60 hover:scale-105 active:scale-95 transition-all text-xs font-semibold cursor-pointer group"
      title="AI Copilot (CATable Assistant)"
      aria-label="Open AI Copilot"
    >
      <svg class="w-4 h-4 text-rose-500 group-hover:rotate-12 transition-transform shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09zM18.259 8.715L18 9.75l-.259-1.035a3.375 3.375 0 00-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 002.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 002.456 2.456L21.75 6l-1.035.259a3.375 3.375 0 00-2.456 2.456zM16.894 20.567L16.5 21.75l-.394-1.183a2.25 2.25 0 00-1.423-1.423L13.5 18.75l1.183-.394a2.25 2.25 0 001.423-1.423l.394-1.183.394 1.183a2.25 2.25 0 001.423 1.423l1.183.394-1.183.394a2.25 2.25 0 00-1.423 1.423z" />
      </svg>
      <span>AI Copilot</span>
      <span class="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse"></span>
    </button>
  {/if}

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
