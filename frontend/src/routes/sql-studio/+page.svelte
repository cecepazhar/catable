<script lang="ts">
  import { onMount } from 'svelte';
  import { dbStore, type QueryResult } from '$lib/stores/database.svelte';
  import { showToast } from '$lib/stores/uiNotifications.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';

  let sqlQuery = $state('SELECT * FROM users LIMIT 10;');
  let isExecuting = $state(false);
  let result = $state<QueryResult | null>(null);
  let selectedTab = $state<'results' | 'messages' | 'schema'>('results');

  onMount(async () => {
    await dbStore.loadTables();
  });

  async function runQuery() {
    if (!sqlQuery.trim()) return;
    isExecuting = true;
    result = null;
    try {
      // First pass: guardrail check
      const verdict = await dbStore.evaluateSafety(sqlQuery);
      if (verdict.risk === 'blocked') {
        showToast(`Blocked by Guardrail: ${verdict.reason}`, 'error');
        isExecuting = false;
        return;
      }

      const res = await dbStore.executeQuery(sqlQuery);
      result = res;
      if (res.error) {
        showToast(`Query failed: ${res.error}`, 'error');
      }
    } catch (e: any) {
      showToast(e?.message || 'Execution failed.', 'error');
    } finally {
      isExecuting = false;
    }
  }

  function cellVal(v: any): string {
    if (v === null || v === undefined) return 'NULL';
    return String(v);
  }
</script>

<div class="flex flex-col h-full overflow-hidden p-4 max-w-7xl mx-auto w-full space-y-4">
  <!-- Header -->
  <div class="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3 shrink-0">
    <div class="flex items-center gap-3">
      <h2 class="text-sm font-bold text-neutral-900 dark:text-white">SQL Query Studio</h2>
      <span class="text-[10px] font-mono bg-neutral-100 dark:bg-neutral-800 text-neutral-500 dark:text-neutral-400 px-2 py-0.5 rounded">
        Active: {dbStore.activeConnection.name} ({dbStore.activeConnection.driver})
      </span>
    </div>

    <div class="flex items-center gap-2">
      <button
        disabled={isExecuting}
        onclick={runQuery}
        class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold text-white bg-emerald-600 hover:bg-emerald-500 rounded-lg shadow-sm transition-colors disabled:opacity-50 cursor-pointer"
      >
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832L14.752 12.83a1 1 0 000-1.662z" /><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 12a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>
        Run Query
      </button>
    </div>
  </div>

  <div class="flex-1 flex gap-4 overflow-hidden min-h-0">
    <!-- Schema Sidebar -->
    <aside class="w-56 bg-neutral-50 dark:bg-neutral-900/50 border border-neutral-200 dark:border-neutral-800 rounded-xl flex flex-col overflow-hidden shrink-0">
      <div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900/80 text-[11px] font-bold text-neutral-500 uppercase tracking-wider">
        Object Explorer
      </div>
      <div class="flex-1 overflow-y-auto p-2 space-y-0.5 text-xs">
        {#each dbStore.tables as table (table.name)}
          <button
            onclick={() => {
              sqlQuery = `SELECT * FROM ${table.name} LIMIT 50;`;
              selectedTab = 'results';
            }}
            class="w-full text-left px-2 py-1.5 rounded-lg flex items-center justify-between hover:bg-blue-500/10 hover:text-blue-600 dark:hover:text-blue-400 transition-colors font-medium"
          >
            <span class="flex items-center gap-1.5 truncate">
              <svg class="w-3.5 h-3.5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M3 10h18M3 14h18M9 6v12M15 6v12" />
              </svg>
              {table.name}
            </span>
            <span class="text-[10px] font-mono text-neutral-400">({table.row_count.toLocaleString()})</span>
          </button>
        {:else}
          <div class="text-neutral-400 p-2 text-[11px]">No tables found for this database. Ensure the connection has seeded data or is correctly linked.</div>
        {/each}
      </div>
    </aside>

    <!-- Main Content -->
    <div class="flex-1 flex flex-col gap-4 min-w-0 overflow-y-auto">
      <!-- Query Editor -->
      <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl overflow-hidden">
        <div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-800/40 flex justify-between items-center">
          <span class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">SQL Editor</span>
          <span class="text-[10px] font-mono text-neutral-400">Ctrl+Enter to Execute</span>
        </div>
        <textarea
          bind:value={sqlQuery}
          rows={6}
          spellcheck="false"
          onkeydown={(e) => {
            if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
              e.preventDefault();
              runQuery();
            }
          }}
          class="w-full p-4 font-mono text-xs bg-transparent text-neutral-900 dark:text-neutral-100 focus:outline-none resize-none"
        ></textarea>
      </div>

      <!-- Results -->
      <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl flex-1 min-h-[120px] flex flex-col">
        <div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between bg-neutral-50 dark:bg-neutral-800/40">
          <span class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">
            {result ? `Results (${result.rows_affected} ${result.rows_affected === 1 ? 'row' : 'rows'}) · ${result.duration_ms} ms` : 'Results'}
          </span>
          {#if result?.error}
            <span class="text-rose-500 text-[10px] font-bold">ERROR</span>
          {/if}
        </div>

        <div class="flex-1 overflow-auto">
          {#if isExecuting}
            <LoadingState message="Executing query..." />
          {:else if result?.error}
            <div class="p-4">
              <div class="bg-rose-50 dark:bg-rose-900/30 border border-rose-200 dark:border-rose-800 text-rose-600 dark:text-rose-400 p-3 rounded-lg text-xs font-mono">
                {result.error}
              </div>
            </div>
          {:else if result && result.columns.length > 0}
            <table class="w-full text-left text-xs border-collapse">
              <thead class="sticky top-0 bg-neutral-50 dark:bg-neutral-800 text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800">
                <tr>
                  {#each result.columns as col}
                    <th class="py-2.5 px-3 font-semibold">{col}</th>
                  {/each}
                </tr>
              </thead>
              <tbody class="divide-y divide-neutral-100 dark:divide-neutral-800/80">
                {#each result.rows as row, i}
                  <tr class="hover:bg-neutral-50 dark:hover:bg-neutral-800/30 transition-colors">
                    <td class="py-2 px-3 font-mono text-neutral-400 text-[10px]">{i + 1}</td>
                    {#each row as val}
                      <td class="py-2 px-3 font-mono text-neutral-700 dark:text-neutral-300">
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
            <div class="h-full flex items-center justify-center text-neutral-400 text-xs">Run a query to see results here.</div>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>
