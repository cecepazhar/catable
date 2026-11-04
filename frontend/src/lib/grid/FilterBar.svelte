<script lang="ts">
  import { OPERATORS, operatorInfo, type BrowseQuery, type ColumnRef } from '$lib/grid/browseQuery.svelte';
  import type { FilterOp } from '$lib/stores/database.svelte';

  interface Props {
    query: BrowseQuery;
    columns: ColumnRef[];
    onApply: () => void;
    onClear: () => void;
  }

  let { query, columns, onApply, onClear }: Props = $props();

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      onApply();
    }
  }

  const field =
    'text-xs px-2 py-1 rounded border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-neutral-800 dark:text-neutral-200 focus:outline-none focus:ring-1 focus:ring-blue-500';
</script>

<div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 space-y-1.5 bg-white dark:bg-neutral-900">
  <div class="flex items-center gap-2">
    <input
      type="search"
      bind:value={query.search}
      onkeydown={onKey}
      placeholder="Search all columns…"
      aria-label="Search all columns"
      class={`${field} w-56`}
    />
    <button
      type="button"
      onclick={() => query.addFilter(columns[0]?.name ?? '')}
      class="px-2 py-1 text-xs rounded border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer"
    >
      + Filter
    </button>
    {#if query.filters.length > 1}
      <select bind:value={query.mode} class={field} aria-label="Match all or any filter">
        <option value="and">Match all</option>
        <option value="or">Match any</option>
      </select>
    {/if}
    <button
      type="button"
      onclick={onApply}
      class="px-2.5 py-1 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded cursor-pointer"
    >
      Apply
    </button>
    {#if query.isFiltering || query.filters.length > 0}
      <button
        type="button"
        onclick={onClear}
        class="px-2 py-1 text-xs text-neutral-500 hover:text-neutral-800 dark:hover:text-white cursor-pointer"
      >
        Clear
      </button>
    {/if}
  </div>

  {#each query.filters as f (f.id)}
    {@const info = operatorInfo(f.op)}
    <div class="flex items-center gap-1.5">
      <select bind:value={f.column} class={`${field} min-w-[120px]`} aria-label="Column">
        {#each columns as c}
          <option value={c.name}>{c.name}</option>
        {/each}
      </select>
      <select
        value={f.op}
        onchange={(e) => (f.op = (e.currentTarget as HTMLSelectElement).value as FilterOp)}
        class={field}
        aria-label="Operator"
      >
        {#each OPERATORS as o}
          <option value={o.op}>{o.label}</option>
        {/each}
      </select>
      {#if info.operands === 1 || info.operands === 'list' || info.operands === 2}
        <input
          bind:value={f.value}
          onkeydown={onKey}
          placeholder={info.operands === 'list' ? 'a, b, c' : 'value'}
          aria-label="Value"
          class={`${field} w-44`}
        />
      {/if}
      {#if info.operands === 2}
        <span class="text-[11px] text-neutral-400">and</span>
        <input bind:value={f.value2} onkeydown={onKey} placeholder="value" aria-label="Second value" class={`${field} w-44`} />
      {/if}
      <button
        type="button"
        onclick={() => query.removeFilter(f.id)}
        class="text-[11px] text-neutral-400 hover:text-rose-500 cursor-pointer"
        aria-label="Remove filter"
        title="Remove filter"
      >
        ✕
      </button>
    </div>
  {/each}
</div>
