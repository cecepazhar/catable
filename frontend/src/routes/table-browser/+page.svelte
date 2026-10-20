<script lang="ts">
  import { onMount } from 'svelte';
  import { dbStore, type QueryResult } from '$lib/stores/database.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';

  let previewResult = $state<QueryResult | null>(null);
  let isPreviewLoading = $state(false);

  onMount(async () => {
    await dbStore.loadTables();
  });

  async function previewTable(name: string) {
    isPreviewLoading = true;
    previewResult = null;
    try {
      const res = await dbStore.executeQuery(`SELECT * FROM ${name} LIMIT 50`);
      previewResult = res;
    } finally {
      isPreviewLoading = false;
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
    </div>
  </div>

  <div class="flex-1 flex gap-4 overflow-hidden min-h-0">
    <!-- Sidebar -->
    <aside class="w-60 bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-200 dark:border-neutral-800 rounded-xl flex flex-col overflow-hidden shrink-0">
      <div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900/80 text-[11px] font-bold text-neutral-500 uppercase tracking-wider">
        Tables
      </div>
      <div class="flex-1 overflow-y-auto p-2 space-y-1 text-xs">
        {#each dbStore.tables as table (table.name)}
          <div
            class={`px-3 py-2.5 rounded-lg cursor-pointer transition-colors border ${table.name === dbStore.activeTable ? 'bg-blue-500/10 border-blue-500/30' : 'border-transparent hover:bg-neutral-100 dark:hover:bg-neutral-800/60'}`}
            onclick={() => {
              dbStore.activeTable = table.name;
              previewTable(table.name);
            }}
          >
            <div class="font-bold text-neutral-800 dark:text-neutral-200 flex items-center gap-1.5">
              <svg class="w-3.5 h-3.5 text-blue-500 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3 10h18M3 14h18M9 6v12M15 6v12" />
              </svg>
              {table.name}
            </div>
            <div class="text-[10px] text-neutral-500 font-mono mt-1">
              {table.table_type === 'table' ? 'TABLE' : 'VIEW'} · {table.row_count.toLocaleString()} rows
            </div>
          </div>
        {:else}
          <div class="text-neutral-400 p-3 text-[11px]">No tables found.</div>
        {/each}
      </div>
    </aside>

    <!-- Main Content -->
    <div class="flex-1 flex flex-col gap-4 min-w-0 overflow-y-auto">
      <!-- Structure Tab -->
      {#if dbStore.activeTable}
        {@const activeTableInfo = dbStore.tables.find((t) => t.name === dbStore.activeTable)}
        {#if activeTableInfo}
          <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-4 space-y-3 shrink-0">
            <div class="flex items-center justify-between">
              <span class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">Column Structure</span>
              <span class="text-[10px] font-mono text-neutral-400">{activeTableInfo.columns.length} columns</span>
            </div>
            <div class="overflow-x-auto">
              <table class="w-full text-left text-xs border-collapse">
                <thead class="bg-neutral-50 dark:bg-neutral-800/60 text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800">
                  <tr>
                    <th class="py-2 px-3">Name</th>
                    <th class="py-2 px-3">Type</th>
                    <th class="py-2 px-3">Flags</th>
                  </tr>
                </thead>
                <tbody class="divide-y divide-neutral-100 dark:divide-neutral-800/80">
                  {#each activeTableInfo.columns as col}
                    <tr>
                      <td class="py-2 px-3 font-mono font-semibold text-neutral-800 dark:text-neutral-200">{col.name}</td>
                      <td class="py-2 px-3 font-mono text-neutral-500">{col.data_type}</td>
                      <td class="py-2 px-3 space-x-2">
                        {#if col.is_pk}
                          <span class="text-amber-500 font-bold text-[10px] uppercase">PK</span>
                        {/if}
                        {#if col.not_null}
                          <span class="text-rose-500 font-bold text-[10px] uppercase">NOT NULL</span>
                        {/if}
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </div>
        {/if}
      {/if}

      <!-- Data Preview Grid -->
      <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl flex-1 min-h-[200px] flex flex-col">
        <div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between bg-neutral-50 dark:bg-neutral-800/40">
          <span class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">
            Data Preview {previewResult ? `(${previewResult.rows_affected} rows)` : ''}
          </span>
        </div>

        <div class="flex-1 overflow-auto">
          {#if isPreviewLoading}
            <LoadingState message="Loading table data..." />
          {:else if previewResult && previewResult.columns.length > 0}
            <table class="w-full text-left text-xs border-collapse">
              <thead class="sticky top-0 bg-neutral-50 dark:bg-neutral-800 text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800">
                <tr>
                  {#each previewResult.columns as col}
                    <th class="py-2.5 px-3 font-semibold min-w-[100px]">{col}</th>
                  {/each}
                </tr>
              </thead>
              <tbody class="divide-y divide-neutral-100 dark:divide-neutral-800/80">
                {#each previewResult.rows as row, i}
                  <tr class="hover:bg-neutral-50 dark:hover:bg-neutral-800/30 transition-colors">
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
            <div class="h-full flex items-center justify-center text-neutral-400 text-xs">Select a table to preview its contents.</div>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>
