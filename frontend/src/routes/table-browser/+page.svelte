<script lang="ts">
  import { onMount } from 'svelte';
  import { dbStore, type QueryResult } from '$lib/stores/database.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';

  const PAGE_SIZE = 50;
  let offset = $state(0);
  let previewResult = $state<QueryResult | null>(null);
  let isPreviewLoading = $state(false);

  const activeTableInfo = $derived(
    dbStore.tables.find((t) => t.name === dbStore.activeTable)
  );
  const totalRows = $derived(activeTableInfo?.row_count ?? 0);
  const currentPage = $derived(Math.floor(offset / PAGE_SIZE) + 1);
  const totalPages = $derived(Math.max(1, Math.ceil(totalRows / PAGE_SIZE)));

  onMount(async () => {
    await dbStore.loadTables();
    if (dbStore.activeTable) {
      void fetchTableData(dbStore.activeTable, 0);
    }
  });

  async function fetchTableData(name: string, newOffset: number) {
    isPreviewLoading = true;
    offset = Math.max(0, newOffset);
    previewResult = null;
    try {
      const escapedName = `"${name.replace(/"/g, '""')}"`;
      const res = await dbStore.executeQuery(
        `SELECT * FROM ${escapedName} LIMIT ${PAGE_SIZE} OFFSET ${offset}`
      );
      previewResult = res;
    } finally {
      isPreviewLoading = false;
    }
  }

  function handleTableSelect(name: string) {
    dbStore.activeTable = name;
    offset = 0;
    void fetchTableData(name, 0);
  }

  function handlePrev() {
    if (offset > 0 && dbStore.activeTable) {
      void fetchTableData(dbStore.activeTable, Math.max(0, offset - PAGE_SIZE));
    }
  }

  function handleNext() {
    if (offset + PAGE_SIZE < totalRows && dbStore.activeTable) {
      void fetchTableData(dbStore.activeTable, offset + PAGE_SIZE);
    }
  }

  function handleFirst() {
    if (offset !== 0 && dbStore.activeTable) {
      void fetchTableData(dbStore.activeTable, 0);
    }
  }

  function handleLast() {
    if (dbStore.activeTable) {
      const lastOffset = Math.max(0, (totalPages - 1) * PAGE_SIZE);
      void fetchTableData(dbStore.activeTable, lastOffset);
    }
  }
</script>

<div class="flex flex-col h-full overflow-hidden p-4 max-w-7xl mx-auto w-full space-y-4">
  <!-- Header -->
  <div class="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3 shrink-0">
    <div class="flex items-center gap-3">
      <h2 class="text-sm font-bold text-neutral-900 dark:text-white">Table Browser & Data Grid</h2>
      <span class="text-[10px] font-mono bg-neutral-100 dark:bg-neutral-800 text-neutral-500 dark:text-neutral-400 px-2 py-0.5 rounded">
        Active: {dbStore.activeConnection.name}
      </span>
      {#if activeTableInfo}
        <span class="text-[10px] font-mono text-blue-600 dark:text-blue-400 bg-blue-50 dark:bg-blue-900/30 px-2 py-0.5 rounded border border-blue-200 dark:border-blue-800 font-semibold">
          {activeTableInfo.name} ({totalRows.toLocaleString()} rows)
        </span>
      {/if}
    </div>
  </div>

  <div class="flex-1 flex gap-4 overflow-hidden min-h-0">
    <!-- Sidebar -->
    <aside class="w-60 bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-200 dark:border-neutral-800 rounded-xl flex flex-col overflow-hidden shrink-0">
      <div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900/80 text-[11px] font-bold text-neutral-500 uppercase tracking-wider flex justify-between items-center">
        <span>Tables</span>
        <span class="text-[10px] font-mono text-neutral-400">{dbStore.tables.length}</span>
      </div>
      <div class="flex-1 overflow-y-auto p-2 space-y-1 text-xs">
        {#each dbStore.tables as table (table.name)}
          <div
            class={`px-3 py-2.5 rounded-lg cursor-pointer transition-colors border ${table.name === dbStore.activeTable ? 'bg-blue-500/10 border-blue-500/30' : 'border-transparent hover:bg-neutral-100 dark:hover:bg-neutral-800/60'}`}
            onclick={() => handleTableSelect(table.name)}
          >
            <div class="font-bold text-neutral-800 dark:text-neutral-200 flex items-center gap-1.5">
              <svg class="w-3.5 h-3.5 text-blue-500 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3 10h18M3 14h18M9 6v12M15 6v12" />
              </svg>
              <span class="truncate">{table.name}</span>
            </div>
            <div class="text-[10px] text-neutral-500 font-mono mt-1 flex justify-between">
              <span>{table.table_type === 'table' ? 'TABLE' : 'VIEW'}</span>
              <span>{table.row_count.toLocaleString()} rows</span>
            </div>
          </div>
        {:else}
          <div class="text-neutral-400 p-3 text-[11px]">No tables found.</div>
        {/each}
      </div>
    </aside>

    <!-- Main Content -->
    <div class="flex-1 flex flex-col gap-4 min-w-0 overflow-y-auto">
      <!-- Structure Card -->
      {#if activeTableInfo}
        <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-4 space-y-3 shrink-0">
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2">
              <span class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">Column Structure</span>
              <span class="text-[10px] font-mono bg-neutral-100 dark:bg-neutral-800 px-1.5 py-0.5 rounded text-neutral-600 dark:text-neutral-400">
                {activeTableInfo.columns.length} columns
              </span>
            </div>
            <span class="text-[10px] font-mono text-neutral-400 uppercase">{activeTableInfo.table_type}</span>
          </div>

          <div class="overflow-x-auto">
            <table class="w-full text-left text-xs border-collapse">
              <thead class="bg-neutral-50 dark:bg-neutral-800/60 text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800">
                <tr>
                  <th class="py-2 px-3">Column</th>
                  <th class="py-2 px-3">Data Type</th>
                  <th class="py-2 px-3">Constraints & Attributes</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-neutral-100 dark:divide-neutral-800/80">
                {#each activeTableInfo.columns as col}
                  <tr class="hover:bg-neutral-50 dark:hover:bg-neutral-800/20 transition-colors">
                    <td class="py-2 px-3 font-mono font-semibold text-neutral-800 dark:text-neutral-200 flex items-center gap-1.5">
                      {#if col.is_pk}
                        <span class="text-amber-500 text-xs" title="Primary Key">🔑</span>
                      {:else if col.name.endsWith('_id')}
                        <span class="text-blue-500 text-xs" title="Foreign Key Column">🔗</span>
                      {:else}
                        <span class="text-neutral-300 dark:text-neutral-600">·</span>
                      {/if}
                      {col.name}
                    </td>
                    <td class="py-2 px-3 font-mono text-neutral-500 dark:text-neutral-400">
                      <span class="px-1.5 py-0.5 rounded bg-neutral-100 dark:bg-neutral-800 text-[11px] font-medium text-neutral-700 dark:text-neutral-300">
                        {col.data_type || 'TEXT'}
                      </span>
                    </td>
                    <td class="py-2 px-3 space-x-1.5">
                      {#if col.is_pk}
                        <span class="bg-amber-500/10 border border-amber-500/30 text-amber-600 dark:text-amber-400 font-bold text-[10px] px-1.5 py-0.5 rounded uppercase">
                          PRIMARY KEY
                        </span>
                      {/if}
                      {#if col.not_null}
                        <span class="bg-rose-500/10 border border-rose-500/30 text-rose-600 dark:text-rose-400 font-bold text-[10px] px-1.5 py-0.5 rounded uppercase">
                          NOT NULL
                        </span>
                      {:else}
                        <span class="text-neutral-400 text-[10px] font-mono">NULLABLE</span>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        </div>
      {/if}

      <!-- Data Preview Grid & Pagination -->
      <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl flex-1 min-h-[220px] flex flex-col">
        <div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between bg-neutral-50 dark:bg-neutral-800/40">
          <div class="flex items-center gap-2">
            <span class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">Data Preview</span>
            {#if previewResult}
              <span class="text-[10px] font-mono text-neutral-400">
                · Showing {totalRows === 0 ? 0 : offset + 1}–{Math.min(offset + previewResult.rows.length, totalRows)} of {totalRows.toLocaleString()} rows
              </span>
            {/if}
          </div>

          <!-- Pagination Controls -->
          <div class="flex items-center gap-1.5">
            <span class="text-[10px] font-mono text-neutral-500 dark:text-neutral-400 mr-2">
              Page {currentPage} of {totalPages}
            </span>

            <button
              disabled={offset === 0 || isPreviewLoading}
              onclick={handleFirst}
              class="px-2 py-1 text-xs border border-neutral-200 dark:border-neutral-700 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors text-neutral-600 dark:text-neutral-300"
              title="First Page"
            >
              «
            </button>
            <button
              disabled={offset === 0 || isPreviewLoading}
              onclick={handlePrev}
              class="flex items-center gap-1 px-2.5 py-1 text-xs border border-neutral-200 dark:border-neutral-700 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors text-neutral-600 dark:text-neutral-300"
            >
              ‹ Prev
            </button>
            <button
              disabled={offset + PAGE_SIZE >= totalRows || isPreviewLoading}
              onclick={handleNext}
              class="flex items-center gap-1 px-2.5 py-1 text-xs border border-neutral-200 dark:border-neutral-700 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors text-neutral-600 dark:text-neutral-300"
            >
              Next ›
            </button>
            <button
              disabled={offset + PAGE_SIZE >= totalRows || isPreviewLoading}
              onclick={handleLast}
              class="px-2 py-1 text-xs border border-neutral-200 dark:border-neutral-700 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors text-neutral-600 dark:text-neutral-300"
              title="Last Page"
            >
              »
            </button>
          </div>
        </div>

        <div class="flex-1 overflow-auto">
          {#if isPreviewLoading}
            <LoadingState message="Loading page data..." />
          {:else if previewResult && previewResult.columns.length > 0}
            <table class="w-full text-left text-xs border-collapse">
              <thead class="sticky top-0 bg-neutral-50 dark:bg-neutral-800 text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800 z-10">
                <tr>
                  <th class="py-2.5 px-3 font-semibold w-12 bg-neutral-50 dark:bg-neutral-800 text-right select-none">#</th>
                  {#each previewResult.columns as col}
                    <th class="py-2.5 px-3 font-semibold min-w-[120px]">{col}</th>
                  {/each}
                </tr>
              </thead>
              <tbody class="divide-y divide-neutral-100 dark:divide-neutral-800/80">
                {#each previewResult.rows as row, i}
                  <tr class="hover:bg-neutral-50 dark:hover:bg-neutral-800/30 transition-colors">
                    <td class="py-2 px-3 font-mono text-neutral-400 text-[10px] bg-neutral-50/50 dark:bg-neutral-800/20 text-right select-none">
                      {offset + i + 1}
                    </td>
                    {#each row as val}
                      <td class="py-2 px-3 font-mono text-neutral-700 dark:text-neutral-300 whitespace-nowrap">
                        {#if val === null}
                          <span class="italic text-neutral-400">NULL</span>
                        {:else}
                          {String(val)}
                        {/if}
                      </td>
                    {/each}
                  </tr>
                {/each}
              </tbody>
            </table>
          {:else}
            <div class="h-full flex items-center justify-center text-neutral-400 text-xs">
              Select a table or change page to preview its contents.
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>
