<script lang="ts">
  import { showToast } from '$lib/stores/uiNotifications.svelte';

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
    onToggle,
  }: Props = $props();

  let filterText = $state('');

  const filteredColumns = $derived(
    filterText.trim()
      ? columns
          .map((c, i) => ({ c, i }))
          .filter(({ c }) => c.toLowerCase().includes(filterText.toLowerCase()))
      : columns.map((c, i) => ({ c, i }))
  );

  function formatValue(val: any): string {
    if (val === null || val === undefined) return 'NULL';
    if (typeof val === 'object') {
      try {
        return JSON.stringify(val, null, 2);
      } catch {
        return String(val);
      }
    }
    const s = String(val);
    return s;
  }

  function copyValue(column: string, val: any) {
    const text = val === null || val === undefined ? 'NULL' : (typeof val === 'object' ? JSON.stringify(val, null, 2) : String(val));
    void navigator.clipboard.writeText(text);
    showToast(`${column} copied to clipboard.`, 'success');
  }

  function copyRowJson() {
    if (!row) return;
    const obj: Record<string, any> = {};
    columns.forEach((col, idx) => {
      obj[col] = row[idx];
    });
    void navigator.clipboard.writeText(JSON.stringify(obj, null, 2));
    showToast('Row copied as JSON.', 'success');
  }

  function copyRowTsv() {
    if (!row) return;
    const header = columns.join('\t');
    const values = row.map((v) => (v === null || v === undefined ? '' : String(v))).join('\t');
    void navigator.clipboard.writeText(`${header}\n${values}`);
    showToast('Row copied as TSV.', 'success');
  }
</script>

{#if isOpen}
  <aside class="w-80 bg-neutral-50 dark:bg-neutral-900/60 border border-neutral-200 dark:border-neutral-800 rounded-xl flex flex-col overflow-hidden shrink-0 shadow-sm transition-all duration-200">
    <!-- Header -->
    <div class="px-3 py-2.5 border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900/80 space-y-2 shrink-0">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-1.5">
          <div class="w-5 h-5 rounded bg-blue-500/10 text-blue-500 flex items-center justify-center">
            <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 6h16M4 10h16M4 14h16M4 18h16" />
            </svg>
          </div>
          <span class="text-xs font-bold text-neutral-800 dark:text-neutral-100">{title}</span>
          {#if rowIndex > 0}
            <span class="text-[10px] font-mono font-semibold bg-blue-50 dark:bg-blue-900/40 text-blue-600 dark:text-blue-400 px-1.5 py-0.5 rounded">
              Row #{rowIndex}
            </span>
          {/if}
        </div>
        <button
          onclick={onClose}
          class="p-1 rounded text-neutral-400 hover:text-neutral-700 dark:hover:text-white hover:bg-neutral-100 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
          title="Close inspector"
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" /></svg>
        </button>
      </div>

      {#if subtitle}
        <div class="text-[10px] font-mono text-neutral-500 dark:text-neutral-400 truncate">{subtitle}</div>
      {/if}

      <div class="relative">
        <input
          bind:value={filterText}
          placeholder="Filter columns…"
          class="w-full px-2.5 py-1.5 text-xs bg-white dark:bg-neutral-800 border border-neutral-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-1 focus:ring-blue-500 text-neutral-800 dark:text-neutral-200"
        />
        {#if filterText}
          <button
            onclick={() => (filterText = '')}
            class="absolute right-2 top-1/2 -translate-y-1/2 text-neutral-400 hover:text-neutral-600 text-xs"
          >
            ×
          </button>
        {/if}
      </div>
    </div>

    <!-- Column Values List -->
    <div class="flex-1 overflow-y-auto p-2 space-y-2 text-xs">
      {#if !row}
        <div class="h-full flex flex-col items-center justify-center text-center p-4 text-neutral-400">
          <svg class="w-8 h-8 mb-2 opacity-50" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M15 15l-2 5L9 9l11 4-5 2zm0 0l5 5M7.188 2.239l.777 2.897M5.136 7.965l-2.898-.777M13.95 4.05l-2.122 2.122m-5.657 5.656l-2.12 2.122" />
          </svg>
          <p class="text-xs font-medium">No row selected</p>
          <p class="text-[10px] text-neutral-500 mt-1">Click any row in the data grid to inspect all column values.</p>
        </div>
      {:else}
        {#each filteredColumns as { c, i } (c)}
          <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-lg p-2.5 space-y-1.5 hover:border-neutral-300 dark:hover:border-neutral-700 transition-colors">
            <div class="flex items-center justify-between gap-1.5">
              <div class="flex items-center gap-1.5 min-w-0">
                <span class="font-mono font-semibold text-[11px] text-neutral-800 dark:text-neutral-200 truncate" title={c}>{c}</span>
                {#if columnTypes?.[c]}
                  <span class="text-[9px] font-mono text-neutral-500 dark:text-neutral-400 bg-neutral-100 dark:bg-neutral-800 px-1.5 py-0.5 rounded shrink-0">
                    {columnTypes[c]}
                  </span>
                {/if}
              </div>
              <button
                onclick={() => copyValue(c, row[i])}
                class="p-1 rounded hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200 transition-colors shrink-0"
                title={`Copy ${c} value`}
              >
                <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" /></svg>
              </button>
            </div>

            <!-- Value Display with Multi-line Support -->
            <div class="bg-neutral-50 dark:bg-neutral-800/50 rounded p-1.5 border border-neutral-100 dark:border-neutral-800">
              {#if row[i] === null || row[i] === undefined}
                <span class="italic text-neutral-400 font-mono text-[11px]">NULL</span>
              {:else if String(row[i]) === ''}
                <span class="italic text-neutral-400 font-mono text-[11px]">&lt;empty&gt;</span>
              {:else}
                <pre class="font-mono text-[11px] text-neutral-800 dark:text-neutral-200 break-words whitespace-pre-wrap max-h-36 overflow-y-auto leading-relaxed select-text">{formatValue(row[i])}</pre>
              {/if}
            </div>
          </div>
        {/each}
      {/if}
    </div>

    <!-- Inspector Footer Quick Actions -->
    {#if row}
      <div class="px-3 py-2 border-t border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900/90 flex items-center justify-between shrink-0">
        <span class="text-[10px] font-mono text-neutral-400">{columns.length} columns</span>
        <div class="flex items-center gap-1.5">
          <button
            onclick={copyRowJson}
            class="flex items-center gap-1 px-2 py-1 text-[10px] font-medium text-neutral-700 dark:text-neutral-300 border border-neutral-200 dark:border-neutral-700 rounded hover:bg-neutral-50 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
            title="Copy as JSON"
          >
            Copy JSON
          </button>
          <button
            onclick={copyRowTsv}
            class="flex items-center gap-1 px-2 py-1 text-[10px] font-medium text-neutral-700 dark:text-neutral-300 border border-neutral-200 dark:border-neutral-700 rounded hover:bg-neutral-50 dark:hover:bg-neutral-800 transition-colors cursor-pointer"
            title="Copy as TSV"
          >
            Copy TSV
          </button>
        </div>
      </div>
    {/if}
  </aside>
{:else}
  <!-- Collapsed Inspector Strip -->
  <button
    onclick={onToggle}
    class="w-7 shrink-0 rounded-xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 flex flex-col items-center justify-center py-4 text-neutral-400 hover:text-blue-500 hover:border-blue-300 dark:hover:border-blue-700/60 transition-colors cursor-pointer group shadow-2xs"
    title="Open Row Details inspector"
  >
    <svg class="w-3.5 h-3.5 mb-2 group-hover:text-blue-500 transition-colors" fill="none" viewBox="0 0 24 24" stroke="currentColor">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 19l-7-7 7-7" />
    </svg>
    <span class="text-[9px] font-bold uppercase tracking-widest [writing-mode:vertical-rl] rotate-180 text-neutral-400 group-hover:text-neutral-600 dark:group-hover:text-neutral-300">
      Row Details
    </span>
  </button>
{/if}
