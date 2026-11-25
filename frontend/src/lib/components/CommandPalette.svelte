<script lang="ts">
  // Ctrl+P / Ctrl+K "Open anything": tables, views, connections and app commands.
  import { goto } from '$app/navigation';
  import { tick } from 'svelte';
  import { getPalette, closePalette } from '$lib/stores/commandPalette.svelte';
  import { dbStore } from '$lib/stores/database.svelte';
  import { workspace } from '$lib/stores/workspace.svelte';
  import { showToast } from '$lib/stores/uiNotifications.svelte';
  import { errorText } from '$lib/errors';

  const palette = getPalette();
  let { onLock }: { onLock?: () => void } = $props();
  let query = $state('');
  let selectedIndex = $state(0);
  let inputEl = $state<HTMLInputElement | null>(null);
  let listEl = $state<HTMLDivElement | null>(null);

  type Kind = 'table' | 'view' | 'connection' | 'command' | 'go';
  interface Item {
    id: string;
    title: string;
    detail?: string;
    kind: Kind;
    hint?: string;
    run: () => void | Promise<void>;
  }

  const KIND_LABEL: Record<Kind, string> = {
    table: 'Table',
    view: 'View',
    connection: 'Connect',
    command: 'Command',
    go: 'Go to',
  };

  function openTable(name: string) {
    workspace.open(name);
    void goto('/table-browser');
  }

  async function connect(id: string) {
    const c = dbStore.connections.find((x) => x.id === id);
    if (!c) return;
    if (dbStore.openConnections.some((x) => x.id === id)) {
      await dbStore.switchConnection(id);
      void goto('/table-browser');
      return;
    }
    try {
      await dbStore.connect(c);
      showToast(`Connected to "${c.name}"`, 'success');
      void goto('/table-browser');
    } catch (e) {
      showToast(`Could not connect to "${c.name}": ${errorText(e)}`, 'error');
    }
  }

  const items = $derived.by((): Item[] => {
    const out: Item[] = [];
    if (dbStore.isConnected) {
      for (const t of dbStore.tables) {
        const view = /view/i.test(t.table_type);
        out.push({
          id: `t:${t.name}`,
          title: t.name,
          detail: `${t.row_count.toLocaleString()} rows · ${t.columns.length} columns`,
          kind: view ? 'view' : 'table',
          run: () => openTable(t.name),
        });
      }
      out.push(
        { id: 'c:sql', title: 'New SQL query', kind: 'command', hint: 'Ctrl+T', run: () => goto('/sql-studio') },
        { id: 'c:refresh', title: 'Refresh table', kind: 'command', hint: 'Ctrl+R', run: () => workspace.refresh() },
        {
          id: 'c:filters',
          title: 'Toggle filters',
          kind: 'command',
          hint: 'Ctrl+F',
          run: () => {
            workspace.filtersOpen = !workspace.filtersOpen;
            void goto('/table-browser');
          },
        },
        {
          id: 'c:inspector',
          title: 'Toggle row details panel',
          kind: 'command',
          hint: 'Ctrl+I',
          run: () => {
            workspace.inspectorOpen = !workspace.inspectorOpen;
          },
        },
        { id: 'c:erd', title: 'Schema diagram (ERD)', kind: 'go', run: () => goto('/schema-erd') },
        { id: 'c:reload-schema', title: 'Reload schema', kind: 'command', run: () => dbStore.loadTables(true) },
        {
          id: 'c:disconnect',
          title: `Disconnect from ${dbStore.activeConnection.name}`,
          kind: 'command',
          run: () => {
            dbStore.disconnect();
            void goto(dbStore.openConnections.length > 0 ? '/table-browser' : '/');
          },
        }
      );
    }
    for (const c of dbStore.connections) {
      if (dbStore.isConnected && c.id === dbStore.activeConnection.id) continue;
      const isOpen = dbStore.openConnections.some((x) => x.id === c.id);
      out.push({
        id: `conn:${c.id}`,
        title: c.name,
        detail: `${c.driver} · ${c.environment}${isOpen ? ' · open' : ''}`,
        kind: 'connection',
        run: () => connect(c.id),
      });
    }
    out.push(
      { id: 'g:connections', title: 'Connections', kind: 'go', run: () => goto('/') },
      { id: 'g:services', title: 'Local database services', kind: 'go', run: () => goto('/services') },
      { id: 'g:workspaces', title: 'Workspaces', kind: 'go', run: () => goto('/groups') },
      { id: 'g:settings', title: 'Settings', kind: 'go', run: () => goto('/settings') }
    );
    return out;
  });

  /** Subsequence match with a score: prefix > word start > contiguous > scattered. */
  function score(text: string, q: string): number {
    const t = text.toLowerCase();
    if (!q) return 1;
    if (t.startsWith(q)) return 1000 - t.length;
    const idx = t.indexOf(q);
    if (idx >= 0) return (/[\s_.\-]/.test(t[idx - 1] ?? '') ? 800 : 600) - t.length;
    let ti = 0;
    for (const ch of q) {
      ti = t.indexOf(ch, ti);
      if (ti < 0) return -1;
      ti++;
    }
    return 200 - t.length;
  }

  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items.slice(0, 60);
    return items
      .map((it) => ({ it, s: Math.max(score(it.title, q), score(`${KIND_LABEL[it.kind]} ${it.title}`, q) - 300) }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 60)
      .map((x) => x.it);
  });

  $effect(() => {
    if (palette.open) {
      query = '';
      selectedIndex = 0;
      void tick().then(() => inputEl?.focus());
    }
  });

  $effect(() => {
    void results;
    selectedIndex = 0;
  });

  async function choose(item: Item | undefined) {
    if (!item) return;
    closePalette();
    await item.run();
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      closePalette();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIndex = (selectedIndex + 1) % Math.max(1, results.length);
      scrollIntoView();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIndex = (selectedIndex - 1 + results.length) % Math.max(1, results.length);
      scrollIntoView();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      void choose(results[selectedIndex]);
    }
  }

  function scrollIntoView() {
    void tick().then(() => listEl?.querySelector('[data-selected="true"]')?.scrollIntoView({ block: 'nearest' }));
  }

  void onLock;
</script>

{#if palette.open}
  <div
    class="fixed inset-0 z-50 flex items-start justify-center pt-[12vh] bg-black/50 p-4"
    role="dialog"
    aria-modal="true"
    aria-label="Open anything"
    tabindex="-1"
    onclick={(e) => {
      if (e.target === e.currentTarget) closePalette();
    }}
    onkeydown={(e) => {
      if (e.key === 'Escape') closePalette();
    }}
  >
    <div class="w-full max-w-xl bg-white dark:bg-[#141416] border border-[var(--ca-border)] rounded-xl shadow-2xl overflow-hidden">
      <div class="flex items-center px-3.5 border-b border-[var(--ca-border)]">
        <svg class="w-4 h-4 text-neutral-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
        </svg>
        <input
          bind:this={inputEl}
          type="text"
          bind:value={query}
          onkeydown={handleKeyDown}
          placeholder={dbStore.isConnected ? 'Open a table, view, connection or command…' : 'Open a connection or command…'}
          aria-label="Search"
          class="w-full px-3 py-3 bg-transparent text-sm text-neutral-900 dark:text-white placeholder-neutral-400 focus:outline-none"
        />
        <kbd class="text-[10px] font-mono px-1.5 py-0.5 rounded border border-[var(--ca-border)] text-neutral-400">Esc</kbd>
      </div>

      <div bind:this={listEl} class="max-h-[50vh] overflow-y-auto p-1.5">
        {#if results.length === 0}
          <div class="p-4 text-center text-sm text-neutral-500">Nothing matches "{query}".</div>
        {:else}
          {#each results as item, idx (item.id)}
            {@const on = selectedIndex === idx}
            <button
              data-selected={on}
              onclick={() => void choose(item)}
              onmousemove={() => (selectedIndex = idx)}
              class="w-full flex items-center gap-2.5 px-2.5 py-1.5 rounded-lg text-left text-sm {on ? 'bg-[var(--ca-brand)] text-white' : 'text-neutral-700 dark:text-neutral-300'}"
            >
              <span class="w-14 shrink-0 text-[10px] font-semibold uppercase tracking-wide {on ? 'text-white/80' : item.kind === 'table' ? 'text-sky-500' : item.kind === 'view' ? 'text-violet-500' : item.kind === 'connection' ? 'text-emerald-500' : 'text-neutral-400'}">
                {KIND_LABEL[item.kind]}
              </span>
              <span class="truncate {item.kind === 'table' || item.kind === 'view' ? 'font-mono text-[13px]' : ''}">{item.title}</span>
              {#if item.detail}
                <span class="truncate text-[11px] {on ? 'text-white/70' : 'text-neutral-400'}">{item.detail}</span>
              {/if}
              {#if item.hint}
                <span class="ml-auto shrink-0 text-[10px] font-mono {on ? 'text-white/80' : 'text-neutral-400'}">{item.hint}</span>
              {/if}
            </button>
          {/each}
        {/if}
      </div>

      <div class="px-3 py-1.5 border-t border-[var(--ca-border)] flex items-center gap-3 text-[10px] font-mono text-neutral-400">
        <span>↑↓ move</span><span>Enter open</span><span>Ctrl+P / Ctrl+K</span>
      </div>
    </div>
  </div>
{/if}
