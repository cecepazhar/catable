<script lang="ts">
  import type { TableStructure } from '$lib/stores/database.svelte';

  interface Props {
    structure: TableStructure | null;
    loading: boolean;
    error: string | null;
    mode: 'structure' | 'ddl';
    /** Open another table (optionally filtered to one row of it). */
    onOpenTable: (table: string, column?: string, value?: string) => void;
  }

  let { structure, loading, error, mode, onOpenTable }: Props = $props();

  let copied = $state(false);

  async function copyDdl() {
    if (!structure?.ddl) return;
    try {
      await navigator.clipboard.writeText(structure.ddl);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // Clipboard access can be denied; the text stays selectable in the box.
    }
  }

  const th = 'py-2 px-3 text-left font-semibold';
  const badge = 'px-1.5 py-0.5 rounded text-[9px] font-bold uppercase border';
</script>

<div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl flex-1 min-h-[220px] overflow-auto">
  {#if loading}
    <div class="p-6 text-xs text-neutral-400">Reading the table definition…</div>
  {:else if error}
    <div class="m-4 p-3 rounded-lg border border-rose-300 dark:border-rose-800 bg-rose-50 dark:bg-rose-950/40 text-rose-700 dark:text-rose-300 text-xs font-mono whitespace-pre-wrap">
      {error}
    </div>
  {:else if !structure}
    <div class="p-6 text-xs text-neutral-400">Select a table to see its structure.</div>
  {:else if mode === 'ddl'}
    <div class="flex items-center justify-between px-4 py-2 border-b border-neutral-200 dark:border-neutral-800">
      <span class="text-[10px] font-bold uppercase tracking-wider text-neutral-500">CREATE TABLE</span>
      {#if structure.ddl}
        <button
          type="button"
          onclick={() => void copyDdl()}
          class="px-2 py-1 text-xs rounded border border-neutral-200 dark:border-neutral-700 text-neutral-600 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800 cursor-pointer"
        >
          {copied ? 'Copied' : 'Copy'}
        </button>
      {/if}
    </div>
    {#if structure.ddl}
      <pre class="p-4 text-[11px] font-mono leading-relaxed text-neutral-800 dark:text-neutral-200 whitespace-pre-wrap select-text">{structure.ddl}</pre>
    {:else}
      <div class="p-6 text-xs text-neutral-400">This database cannot produce a CREATE statement for this object (for example a view).</div>
    {/if}
  {:else}
    <section>
      <h4 class="px-4 pt-3 pb-1 text-[10px] font-bold uppercase tracking-wider text-neutral-500">
        Columns <span class="font-mono text-neutral-400">({structure.columns.length})</span>
      </h4>
      <table class="w-full text-xs border-collapse">
        <thead class="text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800">
          <tr>
            <th class={th}>#</th>
            <th class={th}>Name</th>
            <th class={th}>Type</th>
            <th class={th}>Null</th>
            <th class={th}>Default</th>
            <th class={th}>Extra</th>
            <th class={th}>Comment</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-neutral-100 dark:divide-neutral-800/80 font-mono">
          {#each structure.columns as c}
            <tr class="hover:bg-neutral-50 dark:hover:bg-neutral-800/20">
              <td class="py-1.5 px-3 text-neutral-400">{c.ordinal}</td>
              <td class="py-1.5 px-3 font-semibold text-neutral-800 dark:text-neutral-200">
                {#if c.is_pk}<span title="Primary key">🔑</span>{/if}
                {c.name}
              </td>
              <td class="py-1.5 px-3 text-neutral-600 dark:text-neutral-400">{c.data_type}</td>
              <td class="py-1.5 px-3">{c.nullable ? 'YES' : 'NO'}</td>
              <td class="py-1.5 px-3 text-neutral-600 dark:text-neutral-400">
                {#if c.default === null}<span class="text-neutral-400 italic">—</span>{:else}{c.default}{/if}
              </td>
              <td class="py-1.5 px-3">
                {#if c.is_auto_increment}
                  <span class={`${badge} border-sky-500/30 text-sky-500 bg-sky-500/10`}>auto</span>
                {/if}
              </td>
              <td class="py-1.5 px-3 text-neutral-500 font-sans">{c.comment ?? ''}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </section>

    <section class="mt-3">
      <h4 class="px-4 pt-2 pb-1 text-[10px] font-bold uppercase tracking-wider text-neutral-500">
        Foreign keys <span class="font-mono text-neutral-400">({structure.foreign_keys.length})</span>
      </h4>
      {#if structure.foreign_keys.length === 0}
        <p class="px-4 pb-2 text-xs text-neutral-400">None.</p>
      {:else}
        <ul class="px-4 pb-2 space-y-1.5 text-xs font-mono">
          {#each structure.foreign_keys as fk}
            <li class="flex flex-wrap items-center gap-2">
              {#if fk.name}<span class="text-neutral-400">{fk.name}</span>{/if}
              <span>({fk.columns.join(', ')})</span>
              <span class="text-neutral-400">→</span>
              <button
                type="button"
                onclick={() => onOpenTable(fk.ref_table)}
                class="text-blue-500 hover:text-blue-400 underline underline-offset-2 cursor-pointer"
                title="Open the referenced table"
              >
                {fk.ref_table}
              </button>
              <span>({fk.ref_columns.length ? fk.ref_columns.join(', ') : 'primary key'})</span>
              {#if fk.on_delete}<span class={`${badge} border-neutral-300 dark:border-neutral-600 text-neutral-500`}>delete: {fk.on_delete}</span>{/if}
              {#if fk.on_update}<span class={`${badge} border-neutral-300 dark:border-neutral-600 text-neutral-500`}>update: {fk.on_update}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section class="mt-1 pb-3">
      <h4 class="px-4 pt-2 pb-1 text-[10px] font-bold uppercase tracking-wider text-neutral-500">
        Indexes <span class="font-mono text-neutral-400">({structure.indexes.length})</span>
      </h4>
      {#if structure.indexes.length === 0}
        <p class="px-4 text-xs text-neutral-400">None.</p>
      {:else}
        <ul class="px-4 space-y-1.5 text-xs font-mono">
          {#each structure.indexes as ix}
            <li class="flex flex-wrap items-center gap-2">
              <span class="font-semibold text-neutral-800 dark:text-neutral-200">{ix.name}</span>
              <span>({ix.columns.join(', ')})</span>
              {#if ix.primary}<span class={`${badge} border-amber-500/30 text-amber-500 bg-amber-500/10`}>primary</span>
              {:else if ix.unique}<span class={`${badge} border-emerald-500/30 text-emerald-500 bg-emerald-500/10`}>unique</span>{/if}
              {#if ix.method}<span class="text-neutral-400">{ix.method}</span>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}
</div>
