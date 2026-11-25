<script lang="ts">
  import { showToast } from '$lib/stores/uiNotifications.svelte';
  import { NULL_CELL, cellText, type CellValue, type EditStaging } from '$lib/grid/staging.svelte';

  interface Props {
    title?: string;
    subtitle?: string;
    columns: string[];
    row: any[] | null;
    rowIndex?: number;
    columnTypes?: Record<string, string> | null;
    isOpen?: boolean;
    onClose?: () => void;
    onToggle?: () => void;
    /**
     * When given (with `stagingRow`), values are read from and written to the edit staging, so
     * edits made here show up in the grid as pending changes and are saved with Ctrl+S.
     */
    staging?: EditStaging | null;
    stagingRow?: number;
    editable?: boolean;
  }

  let {
    title = 'Row Details',
    subtitle = '',
    columns = [],
    row = null,
    rowIndex = 0,
    columnTypes = null,
    isOpen = false,
    onClose,
    staging = null,
    stagingRow = -1,
    editable = false,
  }: Props = $props();

  let filterText = $state('');

  const filteredColumns = $derived(
    filterText.trim()
      ? columns
          .map((c, i) => ({ c, i }))
          .filter(({ c }) => c.toLowerCase().includes(filterText.toLowerCase()))
      : columns.map((c, i) => ({ c, i }))
  );

  /** Editing goes through the staging only when it is wired up for a real row. */
  const live = $derived(!!staging && stagingRow >= 0 && stagingRow < (staging?.rows.length ?? 0));
  const canEdit = $derived(live && editable && !(staging?.isDeleted(stagingRow) ?? false));

  function cellOf(column: string, index: number): CellValue {
    if (live && staging) return staging.cell(stagingRow, column);
    const v = row?.[index];
    if (v === null || v === undefined) return NULL_CELL;
    return { kind: 'text', value: typeof v === 'object' ? JSON.stringify(v, null, 2) : String(v) };
  }

  function textOf(cell: CellValue): string {
    return cell.kind === 'null' ? 'NULL' : cellText(cell);
  }

  function setValue(column: string, value: string) {
    if (!canEdit || !staging) return;
    staging.setCell(stagingRow, column, { kind: 'text', value });
  }

  function setNull(column: string) {
    if (!canEdit || !staging) return;
    staging.setCell(stagingRow, column, NULL_CELL);
  }

  function revert(column: string) {
    staging?.revertCell(stagingRow, column);
  }

  function copyValue(column: string, index: number) {
    void navigator.clipboard.writeText(textOf(cellOf(column, index)));
    showToast(`${column} copied to clipboard.`, 'success');
  }

  function rowObject(): Record<string, string | null> {
    const obj: Record<string, string | null> = {};
    columns.forEach((col, idx) => {
      const cell = cellOf(col, idx);
      obj[col] = cell.kind === 'null' ? null : cellText(cell);
    });
    return obj;
  }

  function copyRowJson() {
    void navigator.clipboard.writeText(JSON.stringify(rowObject(), null, 2));
    showToast('Row copied as JSON.', 'success');
  }

  function copyRowTsv() {
    const obj = rowObject();
    void navigator.clipboard.writeText(`${columns.join('\t')}\n${columns.map((c) => obj[c] ?? '').join('\t')}`);
    showToast('Row copied as TSV.', 'success');
  }

  const hasRow = $derived(live || !!row);
</script>

{#if isOpen}
  <aside
    class="w-80 h-full shrink-0 flex flex-col overflow-hidden border-l border-[var(--ca-border)] bg-neutral-50 dark:bg-[#111113]"
    aria-label="Row details"
  >
    <!-- Header -->
    <div class="px-3 py-2 border-b border-[var(--ca-border)] space-y-2 shrink-0">
      <div class="flex items-center justify-between gap-2">
        <div class="flex items-center gap-1.5 min-w-0">
          <span class="text-xs font-bold text-neutral-800 dark:text-neutral-100">{title}</span>
          {#if rowIndex > 0}
            <span class="text-[10px] font-mono font-semibold bg-[var(--ca-brand-glow)] text-[var(--ca-brand)] px-1.5 py-0.5 rounded">
              #{rowIndex}
            </span>
          {/if}
          {#if live && staging?.isDeleted(stagingRow)}
            <span class="text-[10px] font-semibold text-rose-500">marked for delete</span>
          {/if}
        </div>
        <button
          onclick={onClose}
          class="p-1 rounded text-neutral-400 hover:text-neutral-700 dark:hover:text-white hover:bg-neutral-200/70 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
          title="Close (Ctrl+I)"
          aria-label="Close row details"
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" /></svg>
        </button>
      </div>
      {#if subtitle}
        <div class="text-[10px] font-mono text-neutral-500 dark:text-neutral-400 truncate">{subtitle}</div>
      {/if}
      <input
        bind:value={filterText}
        placeholder="Filter columns…"
        aria-label="Filter columns"
        class="w-full px-2.5 py-1 text-xs bg-white dark:bg-neutral-800 border border-[var(--ca-border)] rounded-md focus:outline-none focus:ring-1 focus:ring-[var(--ca-brand)] text-neutral-800 dark:text-neutral-200"
      />
    </div>

    <!-- Column values -->
    <div class="flex-1 overflow-y-auto px-3 py-2 space-y-2.5 text-xs">
      {#if !hasRow}
        <div class="h-full flex flex-col items-center justify-center text-center p-4 text-neutral-400">
          <p class="text-xs font-medium">No row selected</p>
          <p class="text-[10px] text-neutral-500 mt-1">Click a row in the grid to see every column here.</p>
        </div>
      {:else}
        {#each filteredColumns as { c, i } (c)}
          {@const cell = cellOf(c, i)}
          {@const dirty = live && !!staging?.isDirty(stagingRow, c)}
          <div class="space-y-1">
            <div class="flex items-center justify-between gap-1.5">
              <div class="flex items-center gap-1.5 min-w-0">
                <span class="font-mono font-semibold text-[11px] text-neutral-700 dark:text-neutral-200 truncate" title={c}>{c}</span>
                {#if columnTypes?.[c]}
                  <span class="text-[9px] font-mono text-neutral-400 truncate">{columnTypes[c]}</span>
                {/if}
                {#if live && staging?.keyColumns.includes(c)}
                  <span class="text-[9px] font-bold text-amber-500" title="Primary key">PK</span>
                {/if}
              </div>
              <div class="flex items-center gap-0.5 shrink-0">
                {#if dirty}
                  <button onclick={() => revert(c)} class="px-1 text-[10px] text-amber-600 hover:underline cursor-pointer" title="Undo this change">undo</button>
                {/if}
                {#if canEdit && cell.kind !== 'null'}
                  <button onclick={() => setNull(c)} class="px-1 text-[9px] font-semibold uppercase text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 cursor-pointer" title="Set to NULL">null</button>
                {/if}
                <button
                  onclick={() => copyValue(c, i)}
                  class="p-1 rounded text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 cursor-pointer"
                  title={`Copy ${c}`}
                  aria-label={`Copy ${c}`}
                >
                  <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" /></svg>
                </button>
              </div>
            </div>

            {#if canEdit}
              <textarea
                value={cell.kind === 'null' ? '' : cellText(cell)}
                placeholder={cell.kind === 'null' ? 'NULL' : ''}
                oninput={(e) => setValue(c, (e.currentTarget as HTMLTextAreaElement).value)}
                rows={cellText(cell).length > 60 || cellText(cell).includes('\n') ? 4 : 1}
                spellcheck="false"
                class="w-full px-2 py-1 font-mono text-[11px] rounded-md border resize-y bg-white dark:bg-neutral-900 text-neutral-800 dark:text-neutral-200 focus:outline-none focus:ring-1 focus:ring-[var(--ca-brand)] placeholder:italic {dirty ? 'border-amber-500 bg-amber-500/10' : 'border-[var(--ca-border)]'}"
                aria-label={`Value of ${c}`}
              ></textarea>
            {:else}
              <div class="px-2 py-1 rounded-md border border-[var(--ca-border)] bg-white dark:bg-neutral-900 {dirty ? 'border-amber-500' : ''}">
                {#if cell.kind === 'null'}
                  <span class="italic text-neutral-400 font-mono text-[11px]">NULL</span>
                {:else if cellText(cell) === ''}
                  <span class="italic text-neutral-400 font-mono text-[11px]">&lt;empty&gt;</span>
                {:else}
                  <pre class="font-mono text-[11px] text-neutral-800 dark:text-neutral-200 break-words whitespace-pre-wrap max-h-36 overflow-y-auto leading-relaxed select-text">{cellText(cell)}</pre>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>

    {#if hasRow}
      <div class="px-3 py-2 border-t border-[var(--ca-border)] flex items-center justify-between shrink-0">
        <span class="text-[10px] font-mono text-neutral-400">
          {columns.length} columns{canEdit ? ' · Ctrl+S to save' : ''}
        </span>
        <div class="flex items-center gap-1.5">
          <button onclick={copyRowJson} class="px-2 py-0.5 text-[10px] font-medium text-neutral-600 dark:text-neutral-300 border border-[var(--ca-border)] rounded hover:bg-neutral-200/60 dark:hover:bg-neutral-800 cursor-pointer">JSON</button>
          <button onclick={copyRowTsv} class="px-2 py-0.5 text-[10px] font-medium text-neutral-600 dark:text-neutral-300 border border-[var(--ca-border)] rounded hover:bg-neutral-200/60 dark:hover:bg-neutral-800 cursor-pointer">TSV</button>
        </div>
      </div>
    {/if}
  </aside>
{/if}
