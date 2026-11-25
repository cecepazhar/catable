<script lang="ts">
  import { needsConfirmation } from '$lib/guardPolicy';
  import { onMount } from 'svelte';
  import { dbStore, type QueryResult, type GuardVerdict, type TableInfo } from '$lib/stores/database.svelte';
  import { showToast, confirmModal } from '$lib/stores/uiNotifications.svelte';
  import { page } from '$app/state';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import RowDetailsInspector from '$lib/components/RowDetailsInspector.svelte';

  let sqlQuery = $state('SELECT 1;');
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
      // Start from the table the user is looking at, not a hard-coded sample.
      const table = dbStore.activeTable ?? dbStore.tables[0]?.name;
      if (table) {
        try {
          sqlQuery = (await dbStore.buildSelect({ table, limit: 100, with_total: false })) + ';';
        } catch {
          /* keep the default text */
        }
      }
      void refreshSafety(sqlQuery);
    }
  });

  async function runQuery(sqlOverride?: string, confirmed = false) {
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

      // Destructive queries are confirmed in every environment, warnings outside Development.
      // A query the user already confirmed must not ask again (it used to loop forever).
      if (!confirmed && needsConfirmation(verdict.risk, dbStore.activeConnection.environment)) {
        confirmationPending = { verdict, sql };
        isExecuting = false;
        return;
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
      void runQuery(sql, true);
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

<div class="flex flex-col h-full overflow-hidden">
  <!-- Toolbar -->
  <div class="h-10 px-3 flex items-center gap-2 border-b border-[var(--ca-border)] bg-neutral-50/80 dark:bg-neutral-900/70 shrink-0">
    <span class="text-xs font-semibold text-neutral-800 dark:text-neutral-100">SQL Query</span>
    {#if verdict}
      <span class={`text-[10px] font-mono font-bold px-2 py-0.5 rounded border ${RISK_STYLES[verdict.risk]?.badge || ''}`}>
        {verdictIsLoading ? 'ANALYZING…' : RISK_STYLES[verdict.risk]?.label || verdict.risk.toUpperCase()}
      </span>
    {/if}
    <span class="ml-auto text-[10px] font-mono text-neutral-400 hidden md:inline">
      <kbd class="px-1 rounded border border-[var(--ca-border)]">Ctrl</kbd>+<kbd class="px-1 rounded border border-[var(--ca-border)]">Enter</kbd> run · Tab indent
    </span>
    <button
      onclick={clearEditor}
      class="px-2.5 py-1 text-xs font-medium text-neutral-600 dark:text-neutral-400 hover:bg-neutral-200/70 dark:hover:bg-neutral-800 rounded-md transition-colors cursor-pointer"
    >
      Clear
    </button>
    <button
      disabled={isExecuting}
      onclick={() => runQuery()}
      class="flex items-center gap-1.5 px-3 py-1 text-xs font-semibold text-white bg-[var(--ca-brand)] hover:opacity-90 rounded-md transition-opacity disabled:opacity-50 cursor-pointer"
      title="Run (Ctrl+Enter)"
    >
      <svg class="w-3.5 h-3.5" fill="currentColor" viewBox="0 0 24 24"><path d="M8 5v14l11-7z" /></svg>
      Run
    </button>
  </div>

  <div class="flex-1 flex overflow-hidden min-h-0">
    <div class="flex-1 flex flex-col min-w-0">
      <!-- Query Editor -->
      <textarea
        bind:value={sqlQuery}
        rows={8}
        spellcheck="false"
        onkeydown={handleTextareaKeydown}
        aria-label="SQL editor"
        class="w-full shrink-0 h-[34%] min-h-[110px] p-4 font-mono text-[13px] bg-white dark:bg-[#121214] text-neutral-900 dark:text-neutral-100 focus:outline-none resize-y leading-relaxed border-b border-[var(--ca-border)]"
      ></textarea>

      <!-- Results -->
      <div class="flex-1 min-h-0 overflow-auto bg-white dark:bg-[#141416]">
        {#if isExecuting}
          <LoadingState message="Executing query..." />
        {:else if result?.error}
          <div class="p-4">
            <div class="bg-rose-50 dark:bg-rose-900/30 border border-rose-200 dark:border-rose-800 text-rose-600 dark:text-rose-400 p-3 rounded-lg text-xs font-mono whitespace-pre-wrap">
              {result.error}
            </div>
          </div>
        {:else if result && result.columns.length > 0}
          <table class="w-full text-left text-xs border-collapse">
            <thead class="sticky top-0 bg-neutral-100 dark:bg-neutral-800 text-neutral-600 dark:text-neutral-300 text-[11px] border-b border-[var(--ca-border)] z-10">
              <tr>
                <th class="py-1.5 px-2 font-semibold w-10 text-right text-neutral-400">#</th>
                {#each result.columns as col}
                  <th class="py-1.5 px-2.5 font-semibold min-w-[100px] border-l border-[var(--ca-border)]">{col}</th>
                {/each}
              </tr>
            </thead>
            <tbody>
              {#each result.rows as row, i}
                <tr
                  class={`cursor-pointer ${selectedRowIndex === i ? 'bg-[var(--ca-brand-glow)]' : i % 2 ? 'bg-neutral-50/70 dark:bg-white/[0.02]' : ''} hover:bg-neutral-100 dark:hover:bg-white/[0.05]`}
                  onclick={() => handleRowClick(row, i)}
                >
                  <td class="py-1 px-2 font-mono text-neutral-400 text-[10px] text-right select-none">{i + 1}</td>
                  {#each row as val}
                    <td class="py-1 px-2.5 font-mono text-[12px] text-neutral-700 dark:text-neutral-300 whitespace-nowrap max-w-[420px] truncate">
                      {#if val === null}
                        <span class="italic text-neutral-400">NULL</span>
                      {:else}
                        {typeof val === 'object' ? JSON.stringify(val) : String(val)}
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

      <!-- Status bar -->
      <div class="h-8 px-3 flex items-center gap-3 border-t border-[var(--ca-border)] bg-neutral-50/80 dark:bg-neutral-900/70 text-[11px] text-neutral-500 shrink-0">
        {#if result && !result.error}
          <span>{result.rows.length.toLocaleString()} row{result.rows.length === 1 ? '' : 's'}</span>
          {#if result.rows_affected && result.columns.length === 0}
            <span>{result.rows_affected.toLocaleString()} affected</span>
          {/if}
          <span>{result.duration_ms} ms</span>
          {#if result.truncated}
            <span
              class="text-amber-600 dark:text-amber-400 font-semibold"
              title="The query returned more rows than the 10,000-row display cap. Add a LIMIT or a WHERE clause to see the rest."
            >
              first {result.rows.length.toLocaleString()} rows only
            </span>
          {/if}
        {:else if result?.error}
          <span class="text-rose-500 font-semibold">Error</span>
        {:else}
          <span>Ready</span>
        {/if}
        <span class="ml-auto flex items-center gap-3">
          {#if result && result.rows.length > 0}
            <button onclick={copyResults} class="hover:text-neutral-900 dark:hover:text-white cursor-pointer">Copy</button>
          {/if}
          <button
            onclick={() => (inspectorOpen = !inspectorOpen)}
            class="hover:text-neutral-900 dark:hover:text-white cursor-pointer {inspectorOpen ? 'text-[var(--ca-brand)]' : ''}"
          >
            Row details
          </button>
        </span>
      </div>
    </div>

    {#if inspectorOpen}
      <RowDetailsInspector
        title="Row Details"
        subtitle={`${result?.rows.length ?? 0} rows`}
        columns={result?.columns ?? []}
        row={selectedRow}
        rowIndex={selectedRowIndex >= 0 ? selectedRowIndex + 1 : 0}
        isOpen={true}
        onClose={closeInspector}
      />
    {/if}
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
