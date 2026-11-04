<script lang="ts">
  import { onMount } from 'svelte';
  import { dbStore, type QueryResult, type TableStructure } from '$lib/stores/database.svelte';
  import { showToast, confirmModal } from '$lib/stores/uiNotifications.svelte';
  import { goto } from '$app/navigation';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import { errorText } from '$lib/errors';
  import RowDetailsInspector from '$lib/components/RowDetailsInspector.svelte';
  import EditableGrid from '$lib/grid/EditableGrid.svelte';
  import { EditStaging } from '$lib/grid/staging.svelte';
  import { BrowseQuery } from '$lib/grid/browseQuery.svelte';
  import FilterBar from '$lib/grid/FilterBar.svelte';
  import StructurePanel from '$lib/grid/StructurePanel.svelte';
  import { portal } from '$lib/actions/portal';

  const PAGE_SIZE = 50;
  const staging = new EditStaging();
  const query = new BrowseQuery();
  let viewTab = $state<'data' | 'structure' | 'ddl'>('data');
  let structure = $state<TableStructure | null>(null);
  let structureLoading = $state(false);
  let structureError = $state<string | null>(null);
  const structureCache = new Map<string, TableStructure>();

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
      if (dbStore.activeTable !== name) return; // the user moved on while this was loading
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
  let isPreviewLoading = $state(false);
  let selectedRow = $state<any[] | null>(null);
  let selectedRowIndex = $state<number>(-1);
  let inspectorOpen = $state(false);

  const activeTableInfo = $derived(
    dbStore.tables.find((t) => t.name === dbStore.activeTable)
  );
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

  onMount(async () => {
    await dbStore.loadTables();
    if (dbStore.activeTable) {
      void fetchTableData(dbStore.activeTable, 0);
    } else if (dbStore.tables.length > 0) {
      handleTableSelect(dbStore.tables[0].name);
    }
  });

  async function fetchTableData(name: string, newOffset: number) {
    isPreviewLoading = true;
    offset = Math.max(0, newOffset);
    previewResult = null;
    try {
      // Count on the first page / after a table switch only; paging keeps the known total.
      const res = await dbStore.browseTable({
        table: name,
        limit: PAGE_SIZE,
        offset,
        with_total: offset === 0 || browseTotal === null,
        ...query.toRequestParts(activeTableInfo?.columns ?? []),
      });
      previewResult = res.result;
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

  async function handleTableSelect(name: string) {
    if (name !== dbStore.activeTable && !(await confirmDiscard())) return;
    dbStore.activeTable = name;
    offset = 0;
    browseTotal = null;
    query.reset();
    void fetchTableData(name, 0);
  }

  /** Filters, search and sort are applied by the backend over the whole table, from page 1. */
  async function applyQuery() {
    if (!dbStore.activeTable || !(await confirmDiscard())) return;
    offset = 0;
    browseTotal = null; // the filtered total differs
    void fetchTableData(dbStore.activeTable, 0);
  }

  async function clearQuery() {
    query.clearFilters();
    await applyQuery();
  }

  /** Opens another table, optionally filtered to the one row a foreign key points at. */
  async function openTable(table: string, column?: string, value?: string) {
    if (!dbStore.tables.some((t) => t.name === table)) {
      showToast(`Table "${table}" was not found in this database.`, 'error');
      return;
    }
    if (!(await confirmDiscard())) return;
    dbStore.activeTable = table;
    offset = 0;
    browseTotal = null;
    query.reset();
    if (column !== undefined && value !== undefined) {
      query.addFilter(column);
      query.filters[0].value = value;
    }
    viewTab = 'data';
    void fetchTableData(table, 0);
  }

  function followFk(column: string, value: string) {
    const target = fkTargets.get(column);
    if (target) void openTable(target.table, target.column, value);
  }

  async function sortBy(column: string, additive: boolean) {
    query.toggleSort(column, additive);
    await applyQuery();
  }

  async function handlePrev() {
    if (offset > 0 && dbStore.activeTable && (await confirmDiscard())) {
      void fetchTableData(dbStore.activeTable, Math.max(0, offset - PAGE_SIZE));
    }
  }

  async function handleNext() {
    if (offset + PAGE_SIZE < totalRows && dbStore.activeTable && (await confirmDiscard())) {
      void fetchTableData(dbStore.activeTable, offset + PAGE_SIZE);
    }
  }

  async function handleFirst() {
    if (offset !== 0 && dbStore.activeTable && (await confirmDiscard())) {
      void fetchTableData(dbStore.activeTable, 0);
    }
  }

  async function handleLast() {
    if (dbStore.activeTable && (await confirmDiscard())) {
      const lastOffset = Math.max(0, (totalPages - 1) * PAGE_SIZE);
      void fetchTableData(dbStore.activeTable, lastOffset);
    }
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
      if (dbStore.activeTable) {
        browseTotal = null; // row count may have changed
        await fetchTableData(dbStore.activeTable, offset);
      }
    } catch (e) {
      // Nothing was saved (the backend rolled back); keep the staged edits so they can be fixed.
      commitError = errorText(e);
    } finally {
      isCommitting = false;
    }
  }

  function discardChanges() {
    staging.discard();
  }

  function onWindowKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's' && staging.hasPending) {
      e.preventDefault();
      if (commitSql === null) void openCommit();
    }
  }

  function handleRowClick(row: any[], index: number) {
    selectedRow = row;
    selectedRowIndex = index;
    inspectorOpen = true;
  }

  function closeInspector() {
    inspectorOpen = false;
  }

  async function openInSqlStudio() {
    const tableName = dbStore.activeTable;
    if (!tableName) return;
    try {
      // The backend quotes identifiers for the active driver.
      const sql = await dbStore.buildSelect({ table: tableName, limit: 50, with_total: false });
      void goto(`/sql-studio?query=${encodeURIComponent(sql + ';')}`);
    } catch (e) {
      showToast(errorText(e), 'error');
    }
  }

  const columnTypes = $derived(
    activeTableInfo
      ? Object.fromEntries(activeTableInfo.columns.map((c) => [c.name, c.data_type || 'TEXT']))
      : null
  );
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

    <!-- Quick Actions -->
    <div class="flex items-center gap-2">
      {#if activeTableInfo}
        <button
          onclick={openInSqlStudio}
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-colors cursor-pointer"
          title="Open this table in SQL Studio"
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 20l4-16m4 4l4 4-4 4M6 16l-4-4 4-4" />
          </svg>
          <span>Query in SQL Studio</span>
        </button>
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
      <!-- View tabs: Data | Structure | DDL -->
      {#if activeTableInfo}
        <div class="flex items-center gap-1 shrink-0" role="tablist">
          {#each [['data', 'Data'], ['structure', 'Structure'], ['ddl', 'DDL']] as [id, label]}
            <button
              type="button"
              role="tab"
              aria-selected={viewTab === id}
              onclick={() => (viewTab = id as 'data' | 'structure' | 'ddl')}
              class={`px-3 py-1.5 text-xs font-semibold rounded-lg border cursor-pointer transition-colors ${viewTab === id ? 'bg-blue-500/10 border-blue-500/30 text-blue-600 dark:text-blue-400' : 'border-transparent text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800'}`}
            >
              {label}
            </button>
          {/each}
          <span class="ml-auto text-[10px] font-mono text-neutral-400 uppercase">{activeTableInfo.table_type}</span>
        </div>
      {/if}

      {#if viewTab !== 'data'}
        <StructurePanel
          {structure}
          loading={structureLoading}
          error={structureError}
          mode={viewTab === 'ddl' ? 'ddl' : 'structure'}
          onOpenTable={(t) => void openTable(t)}
        />
      {/if}

      <!-- Data Preview Grid & Pagination -->
      {#if viewTab === 'data'}
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

          <!-- Edit toolbar -->
          <div class="flex items-center gap-1.5 mr-3">
            {#if editBlockedReason && previewResult && !previewResult.error}
              <span class="text-[10px] text-neutral-400" title={editBlockedReason}>Read-only</span>
            {/if}
            {#if canEdit}
              <button
                onclick={() => staging.addRow()}
                class="px-2 py-1 text-xs border border-neutral-200 dark:border-neutral-700 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-600 dark:text-neutral-300 cursor-pointer"
                title="Add a new row"
              >
                + Row
              </button>
            {/if}
            {#if staging.hasPending}
              <button
                onclick={discardChanges}
                class="px-2 py-1 text-xs border border-neutral-200 dark:border-neutral-700 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-600 dark:text-neutral-300 cursor-pointer"
              >
                Discard
              </button>
              <button
                onclick={() => void openCommit()}
                class="px-2.5 py-1 text-xs font-semibold text-white bg-emerald-600 hover:bg-emerald-500 rounded cursor-pointer"
                title="Review the SQL, then save (Ctrl+S)"
              >
                Commit Changes ({staging.pendingCount})
              </button>
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

        <FilterBar
          {query}
          columns={activeTableInfo?.columns ?? []}
          onApply={() => void applyQuery()}
          onClear={() => void clearQuery()}
        />

        <div class="flex-1 overflow-auto">
          {#if isPreviewLoading}
            <LoadingState message="Loading page data..." />
          {:else if previewResult && previewResult.columns.length > 0}
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
            />
          {:else if previewResult?.error}
            <div class="h-full flex items-center justify-center p-6">
              <div class="max-w-xl p-3 rounded-lg border border-rose-300 dark:border-rose-800 bg-rose-50 dark:bg-rose-950/40 text-rose-700 dark:text-rose-300 text-xs font-mono">
                {previewResult.error}
              </div>
            </div>
          {:else}
            <div class="h-full flex items-center justify-center text-neutral-400 text-xs">
              Select a table or change page to preview its contents.
            </div>
          {/if}
        </div>
      </div>
      {/if}
    </div>

    <!-- Row Details Inspector -->
    <RowDetailsInspector
      title="Row Details"
      subtitle={activeTableInfo ? `${activeTableInfo.name} (${totalRows.toLocaleString()} rows)` : 'No table selected'}
      columns={previewResult?.columns ?? []}
      row={selectedRow}
      rowIndex={selectedRowIndex >= 0 ? offset + selectedRowIndex + 1 : 0}
      columnTypes={columnTypes}
      isOpen={inspectorOpen}
      onToggle={() => (inspectorOpen = true)}
      onClose={closeInspector}
    />
  </div>
</div>

<svelte:window onkeydown={onWindowKey} />

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
