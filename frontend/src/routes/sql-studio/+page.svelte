<script lang="ts">
  import { onMount } from 'svelte';
  import { dbStore, type QueryResult, type GuardVerdict, type TableInfo } from '$lib/stores/database.svelte';
  import { showToast, confirmModal } from '$lib/stores/uiNotifications.svelte';
  import { page } from '$app/state';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import RowDetailsInspector from '$lib/components/RowDetailsInspector.svelte';

  let sqlQuery = $state('SELECT * FROM users LIMIT 10;');
  let isExecuting = $state(false);
  let result = $state<QueryResult | null>(null);
  let selectedTab = $state<'results' | 'messages' | 'schema'>('results');
  let verdict = $state<GuardVerdict | null>(null);
  let verdictIsLoading = $state(false);
  let confirmationPending = $state<{ verdict: GuardVerdict; sql: string } | null>(null);
  let selectedRow = $state<any[] | null>(null);
  let selectedRowIndex = $state<number>(-1);
  let inspectorOpen = $state(false);
  let initialQueryRun = $state(false);

  const RISK_STYLES: Record<string, { badge: string; label: string }> = {
    safe: {
      badge: 'bg-emerald-500/10 border-emerald-500/30 text-emerald-600 dark:text-emerald-400',
      label: 'SAFE',
    },
    warning: {
      badge: 'bg-amber-500/10 border-amber-500/30 text-amber-600 dark:text-amber-400',
      label: 'WARNING',
    },
    destructive: {
      badge: 'bg-rose-500/10 border-rose-500/30 text-rose-600 dark:text-rose-400',
      label: 'DESTRUCTIVE',
    },
    blocked: {
      badge: 'bg-gray-500/10 border-gray-500/30 text-gray-500 dark:text-gray-400',
      label: 'BLOCKED',
    },
  };

  async function refreshSafety(sql: string) {
    if (!sql.trim()) {
      verdict = null;
      return;
    }
    verdictIsLoading = true;
    try {
      verdict = await dbStore.evaluateSafety(sql);
    } catch {
      verdict = null;
    } finally {
      verdictIsLoading = false;
    }
  }

  $effect(() => {
    const q = sqlQuery;
    // Debounce safety evaluation to prevent a flood of IPC calls on every keystroke
    const handle = setTimeout(() => {
      void refreshSafety(q);
    }, 350);
    return () => clearTimeout(handle);
  });

  onMount(async () => {
    await dbStore.loadTables();
    const queryParam = page.url.searchParams.get('query');
    if (queryParam) {
      sqlQuery = queryParam;
      void runQuery(queryParam);
    } else {
      void refreshSafety(sqlQuery);
    }
  });

  async function runQuery(sqlOverride?: string) {
    const sql = (sqlOverride ?? sqlQuery).trim();
    if (!sql) return;
    isExecuting = true;
    result = null;
    selectedRow = null;
    selectedRowIndex = -1;
    try {
      // First pass: guardrail check against the active connection
      const verdict = await dbStore.evaluateSafety(sql);
      if (verdict.risk === 'blocked') {
        showToast(`Blocked by Guardrail: ${verdict.reason || 'query is disallowed'}`, 'error');
        isExecuting = false;
        return;
      }

      // Non-dev environments require explicit confirmation for warning/destructive queries
      if (verdict.risk === 'warning' || verdict.risk === 'destructive') {
        if (dbStore.activeConnection.environment !== 'development') {
          confirmationPending = { verdict, sql };
          isExecuting = false;
          return;
        }
      }

      const res = await dbStore.executeQuery(sql);
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

  function handleConfirmation(accepted: boolean) {
    if (!confirmationPending) return;
    const { sql } = confirmationPending;
    confirmationPending = null;
    if (accepted) {
      void runQuery(sql);
    }
  }

  function handleTextareaKeydown(e: KeyboardEvent) {
    if (e.key === 'Tab') {
      e.preventDefault();
      const target = e.currentTarget as HTMLTextAreaElement;
      const start = target.selectionStart;
      const end = target.selectionEnd;
      const newQuery = sqlQuery.slice(0, start) + '    ' + sqlQuery.slice(end);
      sqlQuery = newQuery;
      requestAnimationFrame(() => {
        target.selectionStart = start + 4;
        target.selectionEnd = start + 4;
      });
    } else if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      void runQuery();
    }
  }

  function insertTemplate(template: string) {
    sqlQuery = template;
    void refreshSafety(sqlQuery);
    selectedTab = 'results';
  }

  function clearEditor() {
    sqlQuery = '';
    verdict = null;
    result = null;
  }

  function copyResults() {
    if (!result || result.rows.length === 0) return;
    const header = result.columns.join('\t');
    const rows = result.rows.map((r) => r.map((v) => (v === null ? '' : String(v))).join('\t'));
    const text = [header, ...rows].join('\n');
    void navigator.clipboard.writeText(text);
    showToast('Results copied to clipboard as TSV.', 'success');
  }

  function cellVal(v: any): string {
    if (v === null || v === undefined) return 'NULL';
    return String(v);
  }

  function handleRowClick(row: any[], index: number) {
    selectedRow = row;
    selectedRowIndex = index;
    inspectorOpen = true;
  }

  function closeInspector() {
    inspectorOpen = false;
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
      {#if verdict}
        <span class={`text-[10px] font-mono font-bold px-2 py-0.5 rounded border ${RISK_STYLES[verdict.risk]?.badge || ''}`}>
          {verdictIsLoading ? 'ANALYZING…' : RISK_STYLES[verdict.risk]?.label || verdict.risk.toUpperCase()}
        </span>
      {/if}
    </div>

    <div class="flex items-center gap-2">
      <button
        onclick={clearEditor}
        class="px-2.5 py-1.5 text-xs font-medium text-neutral-600 dark:text-neutral-400 hover:bg-neutral-100 dark:hover:bg-neutral-800 rounded-lg transition-colors cursor-pointer"
      >
        Clear
      </button>
      <button
        disabled={isExecuting}
        onclick={() => runQuery()}
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
              void refreshSafety(sqlQuery);
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
      <div class="p-2 border-t border-neutral-200 dark:border-neutral-800">
        <div class="text-[10px] font-bold text-neutral-400 uppercase tracking-wider mb-1.5 px-1">Quick Snippets</div>
        <div class="flex flex-col gap-1">
          <button
            class="text-left text-[11px] px-2 py-1 rounded font-mono bg-neutral-100 dark:bg-neutral-800/60 hover:bg-blue-500/10 hover:text-blue-600 dark:hover:text-blue-400 transition-colors"
            onclick={() => insertTemplate('SELECT count(*) AS total_rows, count(DISTINCT id) AS total_unique FROM users;')}
          >
            Count unique users
          </button>
          <button
            class="text-left text-[11px] px-2 py-1 rounded font-mono bg-neutral-100 dark:bg-neutral-800/60 hover:bg-blue-500/10 hover:text-blue-600 dark:hover:text-blue-400 transition-colors"
            onclick={() => insertTemplate('EXPLAIN QUERY PLAN SELECT * FROM orders WHERE user_id = 42;')}
          >
            Explain query plan
          </button>
        </div>
      </div>
    </aside>

    <!-- Main Content -->
    <div class="flex-1 flex flex-col gap-4 min-w-0 overflow-y-auto">
      <!-- Query Editor -->
      <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl overflow-hidden">
        <div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-800/40 flex justify-between items-center">
          <span class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">SQL Editor</span>
          <span class="text-[10px] font-mono text-neutral-400">Ctrl+Enter to Execute · Tab Indent</span>
        </div>
        <textarea
          bind:value={sqlQuery}
          rows={6}
          spellcheck="false"
          onkeydown={handleTextareaKeydown}
          class="w-full p-4 font-mono text-xs bg-transparent text-neutral-900 dark:text-neutral-100 focus:outline-none resize-none leading-relaxed"
        ></textarea>
      </div>

      <!-- Results -->
      <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl flex-1 min-h-[120px] flex flex-col">
        <div class="px-3 py-2 border-b border-neutral-200 dark:border-neutral-800 flex items-center justify-between bg-neutral-50 dark:bg-neutral-800/40">
          <span class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">
            {result ? `Results · ${result.rows_affected.toLocaleString()} row${result.rows_affected === 1 ? '' : 's'} · ${result.duration_ms} ms` : 'Results'}
          </span>
          <div class="flex items-center gap-2">
            {#if result?.error}
              <span class="text-rose-500 text-[10px] font-bold">ERROR</span>
            {/if}
            {#if result && result.rows.length > 0}
              <button
                onclick={copyResults}
                class="flex items-center gap-1 text-[10px] font-semibold text-neutral-500 hover:text-neutral-900 dark:hover:text-white transition-colors cursor-pointer"
              >
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" /></svg>
                Copy
              </button>
            {/if}
          </div>
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
              <thead class="sticky top-0 bg-neutral-50 dark:bg-neutral-800 text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800 z-10">
                <tr>
                  <th class="py-2.5 px-3 font-semibold w-10 bg-neutral-50 dark:bg-neutral-800">#</th>
                  {#each result.columns as col}
                    <th class="py-2.5 px-3 font-semibold min-w-[120px]">{col}</th>
                  {/each}
                </tr>
              </thead>
              <tbody class="divide-y divide-neutral-100 dark:divide-neutral-800/80">
                {#each result.rows as row, i}
                  <tr
                    class={`transition-colors cursor-pointer ${selectedRowIndex === i ? 'bg-blue-500/10 hover:bg-blue-500/15' : 'hover:bg-neutral-50 dark:hover:bg-neutral-800/30'}`}
                    onclick={() => handleRowClick(row, i)}
                  >
                    <td class="py-2 px-3 font-mono text-neutral-400 text-[10px] bg-neutral-50/50 dark:bg-neutral-800/20 text-right select-none">{i + 1}</td>
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
            <div class="h-full flex items-center justify-center text-neutral-400 text-xs">Run a query to see results here.</div>
          {/if}
        </div>
      </div>
    </div>

    <!-- Row Details Inspector -->
    <RowDetailsInspector
      title="Row Details"
      subtitle={sqlQuery ? `SELECT query · ${result?.rows.length ?? 0} rows` : 'No query'}
      columns={result?.columns ?? []}
      row={selectedRow}
      rowIndex={selectedRowIndex >= 0 ? selectedRowIndex + 1 : 0}
      isOpen={inspectorOpen}
      onToggle={() => (inspectorOpen = true)}
      onClose={closeInspector}
    />
  </div>

  <!-- Guardrail Confirmation Modal (non-dev environments) -->
  {#if confirmationPending}
    {@const modalVerdict = confirmationPending.verdict}
    {@const modalSql = confirmationPending.sql}
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 dark:bg-black/70 backdrop-blur-xs p-4">
      <div class="relative bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-2xl max-w-lg w-full p-5 space-y-4 shadow-2xl">
        <div class="flex items-center gap-3">
          <div class={`w-9 h-9 rounded-full flex items-center justify-center border ${modalVerdict.risk === 'destructive' ? 'bg-rose-500/10 border-rose-500/30 text-rose-500' : 'bg-amber-500/10 border-amber-500/30 text-amber-500'}`}>
            <svg class="w-4.5 h-4.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.5 0 2.692-1.152 2.496-2.636l-1.544-10.35c-.196-1.484-1.39-2.574-2.91-2.574H6.086c-1.52 0-2.714 1.09-2.91 2.574L1.634 16.364C1.438 17.848 2.63 19 4.13 19z" /></svg>
          </div>
          <div>
            <h3 class="text-sm font-bold text-neutral-900 dark:text-white">Guardrail Confirmation Required</h3>
            <p class="text-[11px] text-neutral-500 dark:text-neutral-400">Environment: <span class="font-semibold uppercase text-rose-500">{dbStore.activeConnection.environment}</span> · Connection: <span class="font-mono">{dbStore.activeConnection.name}</span></p>
          </div>
        </div>

        <p class="text-xs text-neutral-700 dark:text-neutral-300 leading-relaxed">
          {modalVerdict.reason || 'This query modifies or deletes data and requires explicit confirmation outside development environments.'}
        </p>

        <div class="bg-neutral-50 dark:bg-neutral-800/60 border border-neutral-200 dark:border-neutral-700 rounded-lg p-3">
          <div class="text-[10px] font-bold text-neutral-400 uppercase tracking-wider mb-1.5">Query Statement</div>
          <code class="font-mono text-[11px] text-neutral-800 dark:text-neutral-200 break-words block max-h-32 overflow-y-auto">{modalSql}</code>
        </div>

        <div class="flex items-center justify-end gap-3">
          <button
            onclick={() => handleConfirmation(false)}
            class="px-3 py-1.5 text-xs font-medium text-neutral-600 dark:text-neutral-300 hover:text-neutral-900 dark:hover:text-white transition-colors cursor-pointer"
          >
            Cancel
          </button>
          <button
            onclick={() => handleConfirmation(true)}
            class={`px-4 py-1.5 text-xs font-bold text-white rounded-lg shadow-sm transition-colors cursor-pointer ${modalVerdict.risk === 'destructive' ? 'bg-rose-600 hover:bg-rose-500' : 'bg-amber-600 hover:bg-amber-500'}`}
          >
            {modalVerdict.risk === 'destructive' ? 'I Understand — Execute Anyway' : 'Execute Query'}
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>
