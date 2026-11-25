<script lang="ts">
  // TablePlus-style filter rows: [on] column · operator · value  [Apply] [−] [+]
  // followed by one action row (search, SQL, export, columns, Clear, Apply All).
  import { OPERATORS, operatorInfo, type BrowseQuery, type ColumnRef } from '$lib/grid/browseQuery.svelte';
  import type { FilterOp } from '$lib/stores/database.svelte';

  interface Props {
    query: BrowseQuery;
    columns: ColumnRef[];
    onApply: () => void;
    onClear: () => void;
    /** Shows the SELECT the backend built for the current filters. */
    onShowSql?: () => void;
    /** Copies the loaded page as CSV. */
    onExport?: () => void;
    /** Columns hidden in the grid. */
    hidden?: string[];
    onHiddenChange?: (hidden: string[]) => void;
  }

  let { query, columns, onApply, onClear, onShowSql, onExport, hidden = [], onHiddenChange }: Props = $props();

  let columnsMenuOpen = $state(false);

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      onApply();
    }
  }

  function applyRow(id: number) {
    query.applyOnly(id);
    onApply();
  }

  function applyAll() {
    query.enableAll();
    onApply();
  }

  function removeRow(id: number) {
    const wasActive = query.filters.find((f) => f.id === id)?.enabled;
    query.removeFilter(id);
    if (wasActive) onApply();
  }

  function toggleColumn(name: string) {
    const set = new Set(hidden);
    if (set.has(name)) set.delete(name);
    else set.add(name);
    // Never hide every column — the grid would have nothing to show.
    if (set.size >= columns.length) return;
    onHiddenChange?.([...set]);
  }

  const field =
    'h-7 text-xs px-2 rounded-md border border-[var(--ca-border)] bg-white dark:bg-neutral-900 text-neutral-800 dark:text-neutral-200 focus:outline-none focus:ring-1 focus:ring-[var(--ca-brand)]';
  const btn =
    'h-7 px-2.5 text-xs rounded-md border border-[var(--ca-border)] text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer disabled:opacity-40';
  const iconBtn =
    'h-7 w-7 flex items-center justify-center text-sm rounded-md border border-[var(--ca-border)] text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer';
</script>

<div class="px-3 py-2 border-b border-[var(--ca-border)] space-y-1.5 bg-neutral-50/80 dark:bg-neutral-900/60">
  {#each query.filters as f (f.id)}
    {@const info = operatorInfo(f.op)}
    <div class="flex items-center gap-1.5">
      <input
        type="checkbox"
        bind:checked={f.enabled}
        class="w-3.5 h-3.5 accent-[var(--ca-brand)] cursor-pointer"
        aria-label="Use this filter"
        title="Use this filter"
      />
      <select bind:value={f.column} class={`${field} w-40`} aria-label="Column">
        {#each columns as c}
          <option value={c.name}>{c.name}</option>
        {/each}
      </select>
      <select
        value={f.op}
        onchange={(e) => (f.op = (e.currentTarget as HTMLSelectElement).value as FilterOp)}
        class={`${field} w-32`}
        aria-label="Operator"
      >
        {#each OPERATORS as o}
          <option value={o.op}>{o.label}</option>
        {/each}
      </select>
      {#if info.operands === 0}
        <div class="flex-1"></div>
      {:else}
        <input
          bind:value={f.value}
          onkeydown={onKey}
          placeholder={info.operands === 'list' ? 'a, b, c' : 'value'}
          aria-label="Value"
          class={`${field} flex-1 min-w-0 font-mono`}
        />
      {/if}
      {#if info.operands === 2}
        <span class="text-[11px] text-neutral-400">and</span>
        <input bind:value={f.value2} onkeydown={onKey} placeholder="value" aria-label="Second value" class={`${field} flex-1 min-w-0 font-mono`} />
      {/if}
      <button type="button" class={btn} onclick={() => applyRow(f.id)} title="Apply only this filter">Apply</button>
      <button type="button" class={iconBtn} onclick={() => removeRow(f.id)} title="Remove filter" aria-label="Remove filter">−</button>
      <button
        type="button"
        class={iconBtn}
        onclick={() => query.insertAfter(f.id, columns[0]?.name ?? '')}
        title="Add filter"
        aria-label="Add filter"
      >+</button>
    </div>
  {/each}

  <!-- Action row -->
  <div class="flex items-center gap-1.5 flex-wrap">
    <div class="relative">
      <input
        type="search"
        bind:value={query.search}
        onkeydown={onKey}
        placeholder="Search all columns…"
        aria-label="Search all columns"
        class={`${field} w-52 pl-6`}
      />
      <svg class="w-3 h-3 absolute left-2 top-2 text-neutral-400 pointer-events-none" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" /></svg>
    </div>
    {#if query.filters.length === 0}
      <button type="button" class={btn} onclick={() => query.addFilter(columns[0]?.name ?? '')}>+ Filter</button>
    {/if}
    {#if query.filters.length > 1}
      <select bind:value={query.mode} class={field} aria-label="Match all or any filter">
        <option value="and">Match all (AND)</option>
        <option value="or">Match any (OR)</option>
      </select>
    {/if}
    {#if onShowSql}
      <button type="button" class={btn} onclick={onShowSql} title="Show the generated SELECT">SQL</button>
    {/if}
    {#if onExport}
      <button type="button" class={btn} onclick={onExport} title="Copy this page as CSV">Export</button>
    {/if}
    {#if onHiddenChange}
      <div class="relative">
        <button
          type="button"
          class={`${btn} ${hidden.length ? 'border-[var(--ca-brand)] text-[var(--ca-brand)]' : ''}`}
          onclick={() => (columnsMenuOpen = !columnsMenuOpen)}
          aria-expanded={columnsMenuOpen}
        >
          Columns{hidden.length ? ` (${columns.length - hidden.length}/${columns.length})` : ''}
        </button>
        {#if columnsMenuOpen}
          <button type="button" class="fixed inset-0 z-30 cursor-default" aria-label="Close column menu" onclick={() => (columnsMenuOpen = false)}></button>
          <div class="absolute left-0 top-8 z-40 w-60 max-h-80 overflow-y-auto rounded-lg border border-[var(--ca-border)] bg-white dark:bg-neutral-900 shadow-xl p-1.5">
            <div class="flex items-center justify-between px-1.5 pb-1.5 mb-1 border-b border-[var(--ca-border)]">
              <span class="text-[11px] font-semibold text-neutral-500">Visible columns</span>
              <button type="button" class="text-[11px] text-[var(--ca-brand)] hover:underline cursor-pointer" onclick={() => onHiddenChange?.([])}>Show all</button>
            </div>
            {#each columns as c}
              <label class="flex items-center gap-2 px-1.5 py-1 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer text-xs">
                <input type="checkbox" class="accent-[var(--ca-brand)]" checked={!hidden.includes(c.name)} onchange={() => toggleColumn(c.name)} />
                <span class="font-mono truncate flex-1">{c.name}</span>
                <span class="text-[9px] font-mono text-neutral-400 truncate max-w-[70px]">{c.data_type}</span>
              </label>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <span class="hidden xl:flex items-center gap-2 ml-2 text-[10px] text-neutral-400 font-mono">
      <span>Filters: Ctrl+F</span>
      <span>Refresh: Ctrl+R</span>
      <span>Save: Ctrl+S</span>
    </span>

    <div class="ml-auto flex items-center gap-1.5">
      {#if query.isFiltering || query.filters.length > 0}
        <button type="button" class={btn} onclick={onClear}>Clear</button>
      {/if}
      <button
        type="button"
        onclick={applyAll}
        class="h-7 px-3 text-xs font-semibold text-white bg-[var(--ca-brand)] hover:opacity-90 rounded-md cursor-pointer"
        title="Apply all filters (Enter)"
      >
        Apply All
      </button>
    </div>
  </div>
</div>
