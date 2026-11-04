<script lang="ts">
  import { NULL_CELL, cellText, type CellValue, type EditStaging } from '$lib/grid/staging.svelte';

  interface Props {
    staging: EditStaging;
    /** Offset of the first loaded row, for the row-number column. */
    offset: number;
    selectedRowIndex: number;
    editable: boolean;
    onRowClick: (row: unknown[], index: number) => void;
    /** Sort state of a column; clicking a header calls `onSort` (Shift = add to the sort). */
    sortOf?: (column: string) => 'asc' | 'desc' | null;
    onSort?: (column: string, additive: boolean) => void;
    /** Columns that are (part of) a foreign key; their cells get a link to the referenced row. */
    fkColumns?: ReadonlySet<string>;
    onFollowFk?: (column: string, value: string) => void;
  }

  let { staging, offset, selectedRowIndex, editable, onRowClick, sortOf, onSort, fkColumns, onFollowFk }: Props = $props();

  type Target =
    | { kind: 'row'; row: number; column: string }
    | { kind: 'insert'; index: number; column: string };

  let editing = $state<Target | null>(null);
  let draft = $state('');
  let draftIsNull = $state(false);

  function isEditing(t: Target): boolean {
    const e = editing;
    if (!e || e.kind !== t.kind || e.column !== t.column) return false;
    return e.kind === 'row' && t.kind === 'row' ? e.row === t.row : e.kind === 'insert' && t.kind === 'insert' && e.index === t.index;
  }

  function currentCell(t: Target): CellValue | undefined {
    return t.kind === 'row' ? staging.cell(t.row, t.column) : staging.inserts[t.index]?.[t.column];
  }

  function startEdit(t: Target) {
    if (!editable) return;
    const cell = currentCell(t) ?? NULL_CELL;
    draftIsNull = cell.kind === 'null';
    draft = cellText(cell);
    editing = t;
  }

  function commit() {
    const t = editing;
    if (!t) return;
    const value: CellValue = draftIsNull ? NULL_CELL : { kind: 'text', value: draft };
    if (t.kind === 'row') staging.setCell(t.row, t.column, value);
    else staging.setInsertCell(t.index, t.column, value);
    editing = null;
  }

  function cancel() {
    editing = null;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commit();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    }
  }

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  function setNull() {
    draftIsNull = true;
    draft = '';
    commit();
  }
</script>

{#snippet display(cell: CellValue | undefined)}
  {#if cell === undefined}
    <span class="text-neutral-400 dark:text-neutral-600 text-[10px] uppercase tracking-wide">default</span>
  {:else if cell.kind === 'null'}
    <span class="italic text-neutral-400 text-[11px]" title="NULL">NULL</span>
  {:else if cell.kind === 'default'}
    <span class="text-neutral-400 text-[10px] uppercase tracking-wide">default</span>
  {:else if cell.value === ''}
    <span
      class="px-1 rounded border border-neutral-300 dark:border-neutral-600 text-neutral-400 text-[9px] uppercase tracking-wide"
      title="Empty string (not NULL)">empty</span>
  {:else}
    {cell.value}
  {/if}
{/snippet}

{#snippet editor()}
  <div class="flex items-center gap-1">
    <input
      use:focusOnMount
      bind:value={draft}
      oninput={() => (draftIsNull = false)}
      onkeydown={onKey}
      onblur={commit}
      class="w-full min-w-[110px] px-1.5 py-0.5 text-xs font-mono bg-white dark:bg-neutral-900 border border-blue-500 rounded outline-none"
      placeholder={draftIsNull ? 'NULL' : ''}
      aria-label="Edit cell"
    />
    <button
      type="button"
      onmousedown={(e) => e.preventDefault()}
      onclick={setNull}
      class="px-1 py-0.5 text-[9px] font-semibold uppercase rounded border border-neutral-300 dark:border-neutral-600 text-neutral-500 hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer"
      title="Set this cell to NULL"
    >
      null
    </button>
  </div>
{/snippet}

<table class="w-full text-left text-xs border-collapse">
  <thead
    class="sticky top-0 bg-neutral-50 dark:bg-neutral-800 text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800 z-10"
  >
    <tr>
      <th class="py-2.5 px-3 font-semibold w-12 bg-neutral-50 dark:bg-neutral-800 text-right select-none">#</th>
      {#each staging.columns as col}
        {@const sorted = sortOf?.(col) ?? null}
        <th
          class="py-2.5 px-3 font-semibold min-w-[120px]"
          aria-sort={sorted === 'asc' ? 'ascending' : sorted === 'desc' ? 'descending' : 'none'}
        >
          <button
            type="button"
            class={`inline-flex items-center gap-1 uppercase tracking-wider ${onSort ? 'cursor-pointer hover:text-neutral-900 dark:hover:text-white' : 'cursor-default'}`}
            onclick={(e) => onSort?.(col, e.shiftKey)}
            title={onSort ? 'Click to sort (Shift+click to add a column)' : undefined}
          >
            {col}
            {#if sorted}
              <span class="text-blue-500 normal-case">{sorted === 'asc' ? '▲' : '▼'}</span>
            {/if}
          </button>
          {#if staging.keyColumns.includes(col)}
            <span class="text-amber-500 normal-case" title="Primary key">🔑</span>
          {/if}
        </th>
      {/each}
      {#if editable}
        <th class="w-8 bg-neutral-50 dark:bg-neutral-800"></th>
      {/if}
    </tr>
  </thead>
  <tbody class="divide-y divide-neutral-100 dark:divide-neutral-800/80">
    {#each staging.rows as row, i}
      {@const deleted = staging.isDeleted(i)}
      <tr
        class={`transition-colors ${deleted ? 'bg-rose-500/10 line-through text-neutral-400' : selectedRowIndex === i ? 'bg-blue-500/10' : 'hover:bg-neutral-50 dark:hover:bg-neutral-800/30'}`}
        onclick={() => onRowClick(row, i)}
      >
        <td class="py-2 px-3 font-mono text-neutral-400 text-[10px] bg-neutral-50/50 dark:bg-neutral-800/20 text-right select-none">
          {offset + i + 1}
        </td>
        {#each staging.columns as col}
          {@const dirty = staging.isDirty(i, col)}
          <td
            class={`py-1.5 px-3 font-mono whitespace-nowrap ${dirty ? 'bg-amber-500/15 border-l-2 border-amber-500 text-neutral-900 dark:text-neutral-100' : 'text-neutral-700 dark:text-neutral-300'} ${editable && !deleted ? 'cursor-text' : ''}`}
            ondblclick={() => !deleted && startEdit({ kind: 'row', row: i, column: col })}
            title={dirty ? 'Changed — not saved yet' : undefined}
          >
            {#if isEditing({ kind: 'row', row: i, column: col })}
              {@render editor()}
            {:else}
              {@render display(staging.cell(i, col))}
              {#if fkColumns?.has(col) && staging.cell(i, col).kind === 'text' && cellText(staging.cell(i, col)) !== ''}
                <button
                  type="button"
                  onclick={(e) => {
                    e.stopPropagation();
                    onFollowFk?.(col, cellText(staging.cell(i, col)));
                  }}
                  class="ml-1 text-blue-500 hover:text-blue-400 cursor-pointer no-underline"
                  title="Open the referenced row"
                  aria-label="Open the referenced row"
                >
                  ↗
                </button>
              {/if}
            {/if}
          </td>
        {/each}
        {#if editable}
          <td class="px-1 text-center no-underline">
            <button
              type="button"
              onclick={(e) => {
                e.stopPropagation();
                staging.toggleDelete(i);
              }}
              class="text-[11px] text-neutral-400 hover:text-rose-500 cursor-pointer"
              title={deleted ? 'Keep this row' : 'Delete this row'}
              aria-label={deleted ? 'Keep this row' : 'Delete this row'}
            >
              {deleted ? '↩' : '✕'}
            </button>
          </td>
        {/if}
      </tr>
    {/each}

    {#each staging.inserts as ins, n}
      <tr class="bg-emerald-500/10">
        <td class="py-2 px-3 font-mono text-emerald-600 dark:text-emerald-400 text-[10px] text-right select-none">new</td>
        {#each staging.columns as col}
          <td
            class="py-1.5 px-3 font-mono whitespace-nowrap border-l-2 border-emerald-500/40 cursor-text"
            ondblclick={() => startEdit({ kind: 'insert', index: n, column: col })}
          >
            {#if isEditing({ kind: 'insert', index: n, column: col })}
              {@render editor()}
            {:else}
              {@render display(ins[col])}
            {/if}
          </td>
        {/each}
        <td class="px-1 text-center">
          <button
            type="button"
            onclick={() => staging.removeInsert(n)}
            class="text-[11px] text-neutral-400 hover:text-rose-500 cursor-pointer"
            title="Remove this new row"
            aria-label="Remove this new row"
          >
            ✕
          </button>
        </td>
      </tr>
    {/each}
  </tbody>
</table>
