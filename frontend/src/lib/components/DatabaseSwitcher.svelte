<script lang="ts">
  // TablePlus-style "open database": pick another database on the same server, or create one.
  import { onMount, tick } from 'svelte';
  import { goto } from '$app/navigation';
  import { dbStore } from '$lib/stores/database.svelte';
  import { showToast } from '$lib/stores/uiNotifications.svelte';
  import { errorText } from '$lib/errors';
  import { confirmLeave } from '$lib/stores/unsavedGuard';

  let { compact = false }: { compact?: boolean } = $props();

  let open = $state(false);
  let list = $state<string[] | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let query = $state('');
  let busy = $state<string | null>(null);
  let inputEl = $state<HTMLInputElement | null>(null);

  const current = $derived(dbStore.activeConnection.database);
  const typed = $derived(query.trim());
  const exists = $derived(!!list && list.some((d) => d === typed));
  const shown = $derived(list ? list.filter((d) => !typed || d.toLowerCase().includes(typed.toLowerCase())) : []);
  const preview = $derived(
    typed
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '')
  );

  export async function show(createMode = false) {
    open = true;
    query = '';
    await load();
    await tick();
    inputEl?.focus();
    if (createMode) inputEl?.setAttribute('placeholder', 'Name of the new database…');
  }

  async function load() {
    loading = true;
    error = null;
    try {
      list = await dbStore.listDatabases();
    } catch (e) {
      list = null;
      error = errorText(e) || 'Could not list databases.';
    } finally {
      loading = false;
    }
  }

  async function pick(db: string) {
    if (db === current) {
      open = false;
      return;
    }
    if (!(await confirmLeave())) return;
    busy = db;
    try {
      await dbStore.switchDatabase(db);
      open = false;
      showToast(`Switched to database "${db}"`, 'success');
      void goto('/table-browser');
    } catch (e) {
      showToast(errorText(e) || `Could not open "${db}"`, 'error');
    } finally {
      busy = null;
    }
  }

  async function create() {
    if (!typed || exists || busy) return;
    busy = '+create';
    try {
      const name = await dbStore.createDatabase(typed);
      showToast(`Database "${name}" created`, 'success');
      busy = null;
      await pick(name);
    } catch (e) {
      showToast(errorText(e) || 'Could not create the database.', 'error');
    } finally {
      busy = null;
    }
  }

  // The command palette opens this switcher through a window event.
  onMount(() => {
    const handler = (e: Event) => void show((e as CustomEvent<{ create?: boolean }>).detail?.create ?? false);
    window.addEventListener('catable:database-switcher', handler);
    return () => window.removeEventListener('catable:database-switcher', handler);
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      open = false;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (exists) void pick(typed);
      else if (shown.length === 1 && typed) void pick(shown[0]);
      else if (typed) void create();
    }
  }
</script>

<div class="relative">
  <button
    type="button"
    onclick={() => (open ? (open = false) : void show())}
    class="w-full flex items-center gap-1.5 px-2 py-1 rounded-md text-[11px] font-mono text-neutral-600 dark:text-neutral-300 hover:bg-neutral-200/60 dark:hover:bg-neutral-800/80 cursor-pointer {compact ? 'justify-center' : ''}"
    title="Switch or create a database (Ctrl+P → database)"
    aria-expanded={open}
  >
    <svg class="w-3.5 h-3.5 shrink-0 text-sky-500" fill="none" viewBox="0 0 24 24" stroke="currentColor"><ellipse cx="12" cy="5" rx="8" ry="3" stroke-width="2" /><path stroke-width="2" d="M4 5v14c0 1.66 3.58 3 8 3s8-1.34 8-3V5M4 12c0 1.66 3.58 3 8 3s8-1.34 8-3" /></svg>
    {#if !compact}
      <span class="truncate flex-1 text-left">{current || '(default)'}</span>
      <span class="text-neutral-400">▾</span>
    {/if}
  </button>

  {#if open}
    <button type="button" class="fixed inset-0 z-40 cursor-default" aria-label="Close database switcher" onclick={() => (open = false)}></button>
    <div class="absolute left-0 top-8 z-50 w-64 rounded-lg border border-[var(--ca-border)] bg-white dark:bg-neutral-900 shadow-2xl overflow-hidden">
      <div class="p-2 border-b border-[var(--ca-border)]">
        <input
          bind:this={inputEl}
          bind:value={query}
          onkeydown={onKey}
          placeholder="Find or create a database…"
          aria-label="Find or create a database"
          class="w-full h-7 px-2 text-xs font-mono rounded-md border border-[var(--ca-border)] bg-neutral-50 dark:bg-neutral-800 text-neutral-800 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-[var(--ca-brand)]"
        />
      </div>
      {#if loading && !list}
        <div class="px-3 py-3 text-xs text-neutral-500">Loading databases…</div>
      {:else if error}
        <div class="px-3 py-3 text-xs text-rose-600 dark:text-rose-400 whitespace-pre-wrap">{error}</div>
      {:else}
        <ul class="max-h-60 overflow-y-auto py-1">
          {#each shown as d (d)}
            <li>
              <button
                type="button"
                onclick={() => pick(d)}
                disabled={busy !== null}
                class="w-full flex items-center gap-2 text-left px-3 py-1.5 text-xs font-mono hover:bg-[var(--ca-brand-glow)] disabled:opacity-60 cursor-pointer {d === current ? 'text-[var(--ca-brand)] font-semibold' : 'text-neutral-700 dark:text-neutral-200'}"
              >
                <span class="truncate flex-1">{d}</span>
                {#if d === current}<span class="text-[9px] uppercase">current</span>{/if}
                {#if busy === d}<span class="text-[10px] text-neutral-400">opening…</span>{/if}
              </button>
            </li>
          {:else}
            <li class="px-3 py-2 text-xs text-neutral-500">{typed ? 'No database matches.' : 'No databases.'}</li>
          {/each}
        </ul>
      {/if}
      {#if !error && typed && !exists}
        <button
          type="button"
          onclick={create}
          disabled={busy !== null || dbStore.activeConnection.read_only}
          class="w-full text-left px-3 py-2 text-xs font-semibold border-t border-[var(--ca-border)] text-emerald-600 dark:text-emerald-400 hover:bg-emerald-500/10 disabled:opacity-50 cursor-pointer"
          title={dbStore.activeConnection.read_only ? 'This connection is read-only' : 'Create and open it'}
        >
          {busy === '+create' ? 'Creating…' : `+ Create "${preview || typed}" & open`}
        </button>
      {:else if !error}
        <div class="px-3 py-1.5 text-[10px] text-neutral-400 border-t border-[var(--ca-border)]">Type a new name to create a database · Enter</div>
      {/if}
    </div>
  {/if}
</div>
