<script lang="ts">
  // TablePlus-style table workspace: tab strip · filter rows · grid · row panel · status footer.
  // Which table is open lives in `workspace` (sidebar, palette and shortcuts open tabs there);
  // this page loads whatever `workspace.active` points at.
  import { onMount, untrack } from 'svelte';
  import { SegmentedControl } from '@cecepazhar/caui';
  import { dbStore, type QueryResult, type TableStructure } from '$lib/stores/database.svelte';
  import { workspace } from '$lib/stores/workspace.svelte';
  import { showToast, confirmModal } from '$lib/stores/uiNotifications.svelte';
  import { goto } from '$app/navigation';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import { errorText } from '$lib/errors';
  import RowDetailsInspector from '$lib/components/RowDetailsInspector.svelte';
  import EditableGrid from '$lib/grid/EditableGrid.svelte';
  import { EditStaging, cellText } from '$lib/grid/staging.svelte';
  import { BrowseQuery } from '$lib/grid/browseQuery.svelte';
  import FilterBar from '$lib/grid/FilterBar.svelte';
  import StructurePanel from '$lib/grid/StructurePanel.svelte';
  import { portal } from '$lib/actions/portal';

  const PAGE_SIZE = 100;
  const staging = new EditStaging();
  const query = new BrowseQuery();
  let viewTab = $state<'data' | 'structure' | 'ddl'>('data');
  let structure = $state<TableStructure | null>(null);
  let structureLoading = $state(false);
  let structureError = $state<string | null>(null);
  const structureCache = new Map<string, TableStructure>();

  /** The table whose rows are loaded (may briefly lag `workspace.active`). */
  let shown = $state<string | null>(null);
  /** Connection + schema the loaded rows belong to (tabs can share names across them). */
  let shownScope = '';
  /** A filter to apply once a table opened from a foreign key has switched in. */
  let pendingFk: { column: string; value: string } | null = null;

  /** Single-column foreign keys: column -> the row it points at. */
  const fkTargets = $derived.by(() => {
    const map = new Map<string, { table: string; column: string }>();
    for (const fk of structure?.foreign_keys ?? []) {
      if (fk.columns.length === 1 && fk.ref_columns.length === 1) {
        map.set(fk.columns[0], { table: fk.ref_table, column: fk.ref_columns[0] });
      }
    }
    return map;
  });
  const fkColumns = $derived(new Set(fkTargets.keys()));

  async function loadStructure(name: string, force = false) {
    const key = `${dbStore.activeConnection.id}:${name}`;
    if (!force && structureCache.has(key)) {
      structure = structureCache.get(key)!;
      structureError = null;
      return;
    }
    structureLoading = true;
    structureError = null;
    try {
      const loaded = await dbStore.describeTable(name);
      if (shown !== name) return; // the user moved on while this was loading
      structureCache.set(key, loaded);
      structure = loaded;
    } catch (e) {
      structure = null;
      structureError = errorText(e);
    } finally {
      structureLoading = false;
    }
  }

  let commitSql = $state<string[] | null>(null);
  let commitError = $state<string | null>(null);
  let isCommitting = $state(false);
  let offset = $state(0);
  let previewResult = $state<QueryResult | null>(null);
  let lastSql = $state('');
  let showSql = $state(false);
  let isPreviewLoading = $state(false);
  let selectedRowIndex = $state<number>(-1);

  const activeTableInfo = $derived(dbStore.tables.find((t) => t.name === shown));
  const EDIT_DRIVERS = ['sqlite', 'postgres', 'mysql', 'mariadb'];
  /** Why the grid is read-only, or null when rows can be edited. */
  const editBlockedReason = $derived.by((): string | null => {
    const conn = dbStore.activeConnection;
    if (!EDIT_DRIVERS.includes(conn.driver)) return 'Editing rows is not supported for this database type yet.';
    if (conn.read_only) return 'This connection is read-only.';
    if (activeTableInfo && /view/i.test(activeTableInfo.table_type)) return 'Views cannot be edited.';
    return staging.readOnlyReason;
  });
  const canEdit = $derived(editBlockedReason === null && !!previewResult && !previewResult.error);

  /** Exact filtered total from the backend; falls back to the (possibly estimated) schema count. */
  let browseTotal = $state<number | null>(null);
  const totalRows = $derived(browseTotal ?? activeTableInfo?.row_count ?? 0);
  const currentPage = $derived(Math.floor(offset / PAGE_SIZE) + 1);
  const totalPages = $derived(Math.max(1, Math.ceil(totalRows / PAGE_SIZE)));
  const hiddenCols = $derived(shown ? (workspace.hiddenColumns[shown] ?? []) : []);
  const hiddenSet = $derived(new Set(hiddenCols));

  onMount(async () => {
    await dbStore.loadTables();
    if (!workspace.active) {
      const first = dbStore.activeTable ?? dbStore.tables[0]?.name;
      if (first) workspace.open(first);
    }
  });

  // Load whatever tab became active (sidebar click, palette, Ctrl+Tab, back/forward…).
  $effect(() => {
    const target = workspace.active;
    const scope = `${dbStore.activeConnection.id}|${dbStore.activeSchema ?? ''}`;
    untrack(() => void switchTo(target, scope));
  });

  // Ctrl+R / refresh button.
  $effect(() => {
    if (workspace.refreshTick === 0) return;
    untrack(() => void refresh());
  });

  async function switchTo(target: string | null, scope: string) {
    if (target === shown && scope === shownScope) return;
    if (shown && staging.hasPending && !(await confirmDiscard())) {
      if (scope !== shownScope) {
        // The user left with unsaved edits by switching connection; go back to them.
        void dbStore.switchConnection(shownScope.split('|')[0]);
      } else {
        workspace.revertTo(shown);
      }
      return;
    }
    shown = target;
    shownScope = scope;
    dbStore.activeTable = target;
    offset = 0;
    browseTotal = null;
    selectedRowIndex = -1;
    structure = null;
    query.reset();
    if (!target) {
      previewResult = null;
      staging.load({ table: '', columns: [], rows: [], keyColumns: [] });
      return;
    }
    if (pendingFk) {
      query.addFilter(pendingFk.column);
      query.filters[0].value = pendingFk.value;
      pendingFk = null;
      workspace.filtersOpen = true;
      viewTab = 'data';
    }
    void fetchTableData(target, 0);
  }

  async function fetchTableData(name: string, newOffset: number) {
    isPreviewLoading = true;
    offset = Math.max(0, newOffset);
    selectedRowIndex = -1;
    try {
      // Count on the first page / after a table switch only; paging keeps the known total.
      const res = await dbStore.browseTable({
        table: name,
        limit: PAGE_SIZE,
        offset,
        with_total: offset === 0 || browseTotal === null,
        ...query.toRequestParts(activeTableInfo?.columns ?? []),
      });
      if (shown !== name) return;
      previewResult = res.result;
      lastSql = res.sql;
      if (res.total !== null) browseTotal = res.total;
      if (res.result.error) showToast(res.result.error, 'error');
      void loadStructure(name);
      staging.load({
        table: name,
        columns: res.result.columns,
        rows: res.result.rows,
        keyColumns: (activeTableInfo?.columns ?? []).filter((c) => c.is_pk).map((c) => c.name),
        columnTypes: Object.fromEntries((activeTableInfo?.columns ?? []).map((c) => [c.name, c.data_type])),
      });
    } catch (e) {
      previewResult = { columns: [], rows: [], rows_affected: 0, duration_ms: 0, error: errorText(e) };
    } finally {
      isPreviewLoading = false;
    }
  }

  /** Staged edits are lost when the page changes, so ask first. */
  async function confirmDiscard(): Promise<boolean> {
    if (!staging.hasPending) return true;
    return confirmModal(
      `You have ${staging.pendingCount} uncommitted change${staging.pendingCount === 1 ? '' : 's'}. Discard them?`,
      'Discard changes?',
      true,
      'Discard'
    );
  }

  async function refresh() {
    if (!shown || !(await confirmDiscard())) return;
    browseTotal = null;
    if (structure) void loadStructure(shown, true);
    void fetchTableData(shown, offset);
  }

  /** Filters, search and sort are applied by the backend over the whole table, from page 1. */
  async function applyQuery() {
    if (!shown || !(await confirmDiscard())) return;
    browseTotal = null; // the filtered total differs
    void fetchTableData(shown, 0);
  }

  async function clearQuery() {
    query.clearFilters();
    await applyQuery();
  }

  /** Opens another table in a tab, optionally filtered to the row a foreign key points at. */
  function openTable(table: string, column?: string, value?: string) {
    if (!dbStore.tables.some((t) => t.name === table)) {
      showToast(`Table "${table}" was not found in this database.`, 'error');
      return;
    }
    pendingFk = column !== undefined && value !== undefined ? { column, value } : null;
    workspace.open(table);
  }

  function followFk(column: string, value: string) {
    const target = fkTargets.get(column);
    if (target) openTable(target.table, target.column, value);
  }

  async function sortBy(column: string, additive: boolean) {
    query.toggleSort(column, additive);
    await applyQuery();
  }

  async function goToOffset(next: number) {
    if (!shown || next === offset || !(await confirmDiscard())) return;
    void fetchTableData(shown, next);
  }

  // ---- Commit flow: preview the real SQL first, then apply in one transaction --------------------
  async function openCommit() {
    if (!staging.hasPending) return;
    commitError = null;
    try {
      commitSql = await dbStore.previewChanges(staging.toChangeSet());
    } catch (e) {
      showToast(errorText(e), 'error');
    }
  }

  async function applyCommit() {
    if (!commitSql || isCommitting) return;
    isCommitting = true;
    commitError = null;
    try {
      const res = await dbStore.applyChanges(staging.toChangeSet());
      commitSql = null;
      showToast(`Saved ${res.statements} change${res.statements === 1 ? '' : 's'}.`, 'success');
      if (shown) {
        browseTotal = null; // row count may have changed
        await fetchTableData(shown, offset);
      }
    } catch (e) {
      // Nothing was saved (the backend rolled back); keep the staged edits so they can be fixed.
      commitError = errorText(e);
    } finally {
      isCommitting = false;
    }
  }

  function addRow() {
    if (!canEdit) return;
    staging.addRow();
    viewTab = 'data';
  }

  function onWindowKey(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    if (e.key.toLowerCase() === 's' && staging.hasPending) {
      e.preventDefault();
      if (commitSql === null) void openCommit();
    }
  }

  function handleRowClick(_row: unknown[], index: number) {
    selectedRowIndex = index;
  }

  function closeTab(table: string, e?: MouseEvent) {
    e?.stopPropagation();
    workspace.close(table);
  }

  async function openInSqlStudio() {
    if (!shown) return;
    void goto(`/sql-studio?query=${encodeURIComponent((lastSql || `SELECT * FROM ${shown}`) + ';')}`);
  }

  function csvField(v: string | null): string {
    if (v === null) return '';
    return /[",\n\r]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v;
  }

  function exportCsv() {
    const cols = staging.columns.filter((c) => !hiddenSet.has(c));
    const lines = [cols.map(csvField).join(',')];
    staging.rows.forEach((_, r) => {
      lines.push(
        cols
          .map((c) => {
            const cell = staging.cell(r, c);
            return csvField(cell.kind === 'null' ? null : cellText(cell));
          })
          .join(',')
      );
    });
    void navigator.clipboard.writeText(lines.join('\n'));
    showToast(`Copied ${staging.rows.length} row${staging.rows.length === 1 ? '' : 's'} as CSV.`, 'success');
  }

  function toggleFilters() {
    workspace.filtersOpen = !workspace.filtersOpen;
    if (workspace.filtersOpen && query.filters.length === 0 && activeTableInfo?.columns[0]) {
      query.addFilter(activeTableInfo.columns[0].name);
    }
  }

  const columnTypes = $derived(
    activeTableInfo ? Object.fromEntries(activeTableInfo.columns.map((c) => [c.name, c.data_type || 'TEXT'])) : null
  );
  const selectedRow = $derived(selectedRowIndex >= 0 ? (staging.rows[selectedRowIndex] ?? null) : null);
  const rangeLabel = $derived.by(() => {
    if (!previewResult || previewResult.error) return '';
    const n = previewResult.rows.length;
    if (n === 0) return totalRows === 0 ? 'No rows' : `0 of ${totalRows.toLocaleString()} rows`;
    return `${(offset + 1).toLocaleString()}–${(offset + n).toLocaleString()} of ${totalRows.toLocaleString()} rows`;
  });

  const tool =
    'h-7 px-2 flex items-center gap-1 text-xs rounded-md text-neutral-600 dark:text-neutral-300 hover:bg-neutral-200/70 dark:hover:bg-neutral-800 disabled:opacity-35 disabled:hover:bg-transparent cursor-pointer';
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="flex flex-col h-full overflow-hidden bg-white dark:bg-[#141416]">
  <!-- Tab strip -->
  <div class="h-9 flex items-stretch border-b border-[var(--ca-border)] bg-neutral-50 dark:bg-neutral-900/70 shrink-0">
    <div class="flex items-center gap-0.5 px-1.5 border-r border-[var(--ca-border)]">
      <button class={tool} disabled={!workspace.canBack} onclick={() => workspace.goBack()} title="Back (Ctrl+[)" aria-label="Back">
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.2" d="M15 19l-7-7 7-7" /></svg>
      </button>
      <button class={tool} disabled={!workspace.canForward} onclick={() => workspace.goForward()} title="Forward (Ctrl+])" aria-label="Forward">
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.2" d="M9 5l7 7-7 7" /></svg>
      </button>
    </div>

    <div class="flex-1 flex items-stretch overflow-x-auto scrollbar-none min-w-0" role="tablist" aria-label="Open tables">
      {#each workspace.tabs as tab (tab)}
        {@const on = tab === workspace.active}
        <div
          role="tab"
          tabindex="0"
          aria-selected={on}
          onclick={() => workspace.open(tab)}
          onkeydown={(e) => e.key === 'Enter' && workspace.open(tab)}
          onauxclick={(e) => e.button === 1 && closeTab(tab, e)}
          title={tab}
          class="group relative flex items-center gap-1.5 pl-3 pr-1.5 min-w-[110px] max-w-[200px] text-xs font-mono border-r border-[var(--ca-border)] cursor-pointer select-none {on ? 'bg-white dark:bg-[#141416] text-neutral-900 dark:text-white font-semibold' : 'text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 hover:bg-neutral-100 dark:hover:bg-neutral-800/60'}"
        >
          {#if on}<span class="absolute inset-x-0 top-0 h-[2px] bg-[var(--ca-brand)]"></span>{/if}
          <svg class="w-3 h-3 shrink-0 {on ? 'text-[var(--ca-brand)]' : 'opacity-60'}" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 10h18M3 14h18m-9-4v8m-7 4h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" /></svg>
          <span class="truncate flex-1">{tab}</span>
          {#if on && staging.hasPending}
            <span class="w-1.5 h-1.5 rounded-full bg-amber-500 shrink-0" title="Unsaved changes"></span>
          {/if}
          <button
            type="button"
            onclick={(e) => closeTab(tab, e)}
            class="w-4 h-4 flex items-center justify-center rounded text-[11px] shrink-0 text-neutral-400 hover:text-neutral-900 dark:hover:text-white hover:bg-neutral-200 dark:hover:bg-neutral-700 {on ? '' : 'opacity-0 group-hover:opacity-100'}"
            title="Close tab (Ctrl+W)"
            aria-label="Close {tab}"
          >✕</button>
        </div>
      {/each}
    </div>

    <div class="flex items-center gap-0.5 px-1.5 border-l border-[var(--ca-border)]">
      <button class={tool} disabled={!shown || isPreviewLoading} onclick={() => workspace.refresh()} title="Refresh (Ctrl+R)" aria-label="Refresh">
        <svg class="w-3.5 h-3.5 {isPreviewLoading ? 'animate-spin' : ''}" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" /></svg>
      </button>
      <button class={tool} disabled={!shown} onclick={openInSqlStudio} title="Open this query in the SQL editor" aria-label="Open in SQL editor">
        <span class="font-mono text-[10px] font-bold">SQL</span>
      </button>
      <button
        class="{tool} {workspace.inspectorOpen ? 'text-[var(--ca-brand)] bg-[var(--ca-brand-glow)]' : ''}"
        onclick={() => (workspace.inspectorOpen = !workspace.inspectorOpen)}
        title="Row details panel (Ctrl+I)"
        aria-label="Toggle row details panel"
        aria-pressed={workspace.inspectorOpen}
      >
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><rect x="3" y="4" width="18" height="16" rx="2" stroke-width="2" /><path stroke-width="2" d="M15 4v16" /></svg>
      </button>
    </div>
  </div>

  {#if !shown}
    <div class="flex-1 flex flex-col items-center justify-center gap-2 text-center p-6">
      <div class="text-sm font-semibold text-neutral-700 dark:text-neutral-200">No table open</div>
      <p class="text-xs text-neutral-500 max-w-sm">
        Pick a table in the sidebar, or press <kbd class="px-1 rounded border border-[var(--ca-border)] font-mono">Ctrl</kbd>+<kbd class="px-1 rounded border border-[var(--ca-border)] font-mono">P</kbd> to jump to one.
      </p>
    </div>
  {:else}
    <div class="flex-1 flex min-h-0">
      <div class="flex-1 flex flex-col min-w-0">
        {#if viewTab === 'data' && workspace.filtersOpen}
          <FilterBar
            {query}
            columns={activeTableInfo?.columns ?? []}
            onApply={() => void applyQuery()}
            onClear={() => void clearQuery()}
            onShowSql={() => (showSql = true)}
            onExport={exportCsv}
            hidden={hiddenCols}
            onHiddenChange={(cols) => shown && workspace.setHidden(shown, cols)}
          />
        {/if}

        <div class="flex-1 min-h-0 overflow-auto">
          {#if viewTab !== 'data'}
            <div class="p-4">
              <StructurePanel
                {structure}
                loading={structureLoading}
                error={structureError}
                mode={viewTab === 'ddl' ? 'ddl' : 'structure'}
                onOpenTable={(t) => openTable(t)}
              />
            </div>
          {:else if isPreviewLoading && !previewResult}
            <LoadingState message="Loading rows…" />
          {:else if previewResult?.error}
            <div class="h-full flex items-center justify-center p-6">
              <div class="max-w-xl p-3 rounded-lg border border-rose-300 dark:border-rose-800 bg-rose-50 dark:bg-rose-950/40 text-rose-700 dark:text-rose-300 text-xs font-mono whitespace-pre-wrap">
                {previewResult.error}
              </div>
            </div>
          {:else if previewResult && previewResult.columns.length > 0}
            <div class="transition-opacity {isPreviewLoading ? 'opacity-50 pointer-events-none' : ''}">
              <EditableGrid
                {staging}
                {offset}
                {selectedRowIndex}
                editable={canEdit}
                onRowClick={handleRowClick}
                sortOf={(c) => query.sortState(c)}
                onSort={sortBy}
                {fkColumns}
                onFollowFk={followFk}
                hidden={hiddenSet}
              />
            </div>
            {#if previewResult.rows.length === 0 && staging.inserts.length === 0}
              <div class="py-10 text-center text-xs text-neutral-400">
                {query.isFiltering ? 'No rows match the current filters.' : 'This table is empty.'}
              </div>
            {/if}
          {:else}
            <LoadingState message="Loading rows…" />
          {/if}
        </div>
      </div>

      <RowDetailsInspector
        title="Row"
        subtitle={shown ?? ''}
        columns={staging.columns}
        row={selectedRow as any[] | null}
        rowIndex={selectedRowIndex >= 0 ? offset + selectedRowIndex + 1 : 0}
        {columnTypes}
        isOpen={workspace.inspectorOpen && viewTab === 'data'}
        onClose={() => (workspace.inspectorOpen = false)}
        {staging}
        stagingRow={selectedRowIndex}
        editable={canEdit}
      />
    </div>

    <!-- Footer: view switch · row actions · status · filters/columns · paging -->
    <div class="h-10 px-2 flex items-center gap-2 border-t border-[var(--ca-border)] bg-neutral-50 dark:bg-neutral-900/70 shrink-0 text-xs">
      <SegmentedControl
        size="sm"
        bind:value={viewTab}
        options={[
          { value: 'data', label: 'Data' },
          { value: 'structure', label: 'Structure' },
          { value: 'ddl', label: 'DDL' },
        ]}
      />

      {#if viewTab === 'data'}
        <button
          class="{tool} border border-[var(--ca-border)]"
          disabled={!canEdit}
          onclick={addRow}
          title={canEdit ? 'Add a new row' : (editBlockedReason ?? 'Read-only')}
        >
          <span class="text-sm leading-none">+</span> Row
        </button>
        {#if staging.hasPending}
          <button class="{tool} border border-[var(--ca-border)]" onclick={() => staging.discard()}>Discard</button>
          <button
            class="h-7 px-3 text-xs font-semibold text-white bg-emerald-600 hover:bg-emerald-500 rounded-md cursor-pointer"
            onclick={() => void openCommit()}
            title="Review the SQL, then save (Ctrl+S)"
          >
            Commit {staging.pendingCount}
          </button>
        {/if}
      {/if}

      <div class="flex-1 min-w-0 text-center text-[11px] text-neutral-500 truncate">
        {#if viewTab === 'data'}
          {#if editBlockedReason && previewResult && !previewResult.error}
            <span class="mr-2 px-1.5 py-0.5 rounded bg-neutral-200/70 dark:bg-neutral-800 text-[10px]" title={editBlockedReason}>read-only</span>
          {/if}
          {rangeLabel}
          {#if selectedRowIndex >= 0}<span class="ml-1">· row {offset + selectedRowIndex + 1} selected</span>{/if}
          {#if previewResult && !previewResult.error}<span class="ml-1 text-neutral-400">· {previewResult.duration_ms} ms</span>{/if}
        {:else if activeTableInfo}
          {activeTableInfo.table_type} · {activeTableInfo.columns.length} columns
        {/if}
      </div>

      {#if viewTab === 'data'}
        <button
          class="h-7 px-2.5 flex items-center gap-1.5 text-xs rounded-md font-medium cursor-pointer {workspace.filtersOpen || query.isFiltering ? 'bg-[var(--ca-brand)] text-white' : 'border border-[var(--ca-border)] text-neutral-600 dark:text-neutral-300 hover:bg-neutral-200/70 dark:hover:bg-neutral-800'}"
          onclick={toggleFilters}
          title="Filters (Ctrl+F)"
          aria-pressed={workspace.filtersOpen}
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" /></svg>
          Filters{query.isFiltering ? ' •' : ''}
        </button>

        <div class="flex items-center gap-0.5 pl-1 border-l border-[var(--ca-border)]">
          <button class={tool} disabled={offset === 0 || isPreviewLoading} onclick={() => goToOffset(0)} title="First page" aria-label="First page">«</button>
          <button class={tool} disabled={offset === 0 || isPreviewLoading} onclick={() => goToOffset(Math.max(0, offset - PAGE_SIZE))} title="Previous page" aria-label="Previous page">‹</button>
          <span class="px-1 text-[11px] font-mono text-neutral-500 whitespace-nowrap">{currentPage} / {totalPages}</span>
          <button class={tool} disabled={offset + PAGE_SIZE >= totalRows || isPreviewLoading} onclick={() => goToOffset(offset + PAGE_SIZE)} title="Next page" aria-label="Next page">›</button>
          <button class={tool} disabled={offset + PAGE_SIZE >= totalRows || isPreviewLoading} onclick={() => goToOffset(Math.max(0, (totalPages - 1) * PAGE_SIZE))} title="Last page" aria-label="Last page">»</button>
        </div>
      {/if}
    </div>
  {/if}
</div>

{#if showSql}
  <div
    use:portal
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
    role="dialog"
    aria-modal="true"
    aria-label="Generated SQL"
    tabindex="-1"
    onclick={(e) => e.target === e.currentTarget && (showSql = false)}
    onkeydown={(e) => e.key === 'Escape' && (showSql = false)}
  >
    <div class="w-full max-w-2xl flex flex-col bg-white dark:bg-[#121212] border border-[var(--ca-border)] rounded-2xl shadow-2xl overflow-hidden">
      <div class="px-4 py-3 border-b border-[var(--ca-border)]">
        <h3 class="text-sm font-bold text-neutral-900 dark:text-white">Generated SQL</h3>
        <p class="text-[11px] text-neutral-500 mt-0.5">The query CATable ran for this page, with the current filters and sort.</p>
      </div>
      <pre class="p-4 text-[12px] font-mono leading-relaxed bg-neutral-50 dark:bg-neutral-900/60 text-neutral-800 dark:text-neutral-200 whitespace-pre-wrap break-all max-h-[50vh] overflow-auto select-text">{lastSql || '—'};</pre>
      <div class="p-3 flex items-center justify-end gap-2">
        <button
          onclick={() => {
            void navigator.clipboard.writeText(lastSql + ';');
            showToast('SQL copied.', 'success');
          }}
          class="px-3 py-1.5 text-xs font-semibold rounded-lg border border-[var(--ca-border)] text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer"
        >Copy</button>
        <button
          onclick={() => {
            showSql = false;
            void openInSqlStudio();
          }}
          class="px-3 py-1.5 text-xs font-semibold rounded-lg border border-[var(--ca-border)] text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer"
        >Open in SQL editor</button>
        <button onclick={() => (showSql = false)} class="px-3 py-1.5 text-xs font-semibold text-white bg-[var(--ca-brand)] rounded-lg cursor-pointer">Close</button>
      </div>
    </div>
  </div>
{/if}

{#if commitSql}
  <div
    use:portal
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-xs p-4"
    role="dialog"
    aria-modal="true"
    aria-label="Review changes"
  >
    <div class="w-full max-w-2xl max-h-[80vh] flex flex-col bg-white dark:bg-[#121212] border border-neutral-200 dark:border-neutral-800 rounded-2xl shadow-2xl overflow-hidden">
      <div class="px-4 py-3 border-b border-neutral-200 dark:border-neutral-800">
        <h3 class="text-sm font-bold text-neutral-900 dark:text-white">Review changes</h3>
        <p class="text-[11px] text-neutral-500 mt-0.5">
          {commitSql.length} statement{commitSql.length === 1 ? '' : 's'} on
          <span class="font-mono">{staging.table}</span>, applied together in one transaction.
        </p>
      </div>
      <pre class="flex-1 overflow-auto p-4 text-[11px] font-mono leading-relaxed bg-neutral-50 dark:bg-neutral-900/60 text-neutral-800 dark:text-neutral-200 whitespace-pre-wrap break-all">{commitSql.join(';\n\n')};</pre>
      {#if commitError}
        <div class="mx-4 mt-3 p-3 rounded-lg border border-rose-300 dark:border-rose-800 bg-rose-50 dark:bg-rose-950/40 text-rose-700 dark:text-rose-300 text-[11px] font-mono whitespace-pre-wrap break-words">
          {commitError}
        </div>
      {/if}
      <div class="p-4 flex items-center justify-end gap-2">
        <button
          onclick={() => (commitSql = null)}
          disabled={isCommitting}
          class="px-3.5 py-2 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 disabled:opacity-50 cursor-pointer"
        >
          {commitError ? 'Back to editing' : 'Cancel'}
        </button>
        <button
          onclick={() => void applyCommit()}
          disabled={isCommitting}
          class="px-4 py-2 text-xs font-semibold text-white bg-emerald-600 hover:bg-emerald-500 rounded-lg disabled:opacity-50 cursor-pointer"
        >
          {isCommitting ? 'Saving…' : commitError ? 'Try again' : 'Apply to database'}
        </button>
      </div>
    </div>
  </div>
{/if}
