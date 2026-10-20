<script lang="ts">
  import { onMount } from 'svelte';
  import { dbStore } from '$lib/stores/database.svelte';
  import { showToast } from '$lib/stores/uiNotifications.svelte';

  onMount(async () => {
    await dbStore.loadTables();
  });

  function copyMermaid() {
    let mermaid = 'erDiagram\n';
    for (const table of dbStore.tables) {
      mermaid += `    ${table.name} {\n`;
      for (const col of table.columns) {
        const keyType = col.is_pk ? 'PK' : '';
        mermaid += `        ${col.data_type || 'text'} ${col.name} ${keyType}\n`;
      }
      mermaid += `    }\n`;
    }
    mermaid += '    users ||--o{ orders : places\n';
    mermaid += '    orders ||--|{ order_items : contains\n';
    mermaid += '    products ||--o{ order_items : referenced_in\n';

    navigator.clipboard.writeText(mermaid);
    showToast('Mermaid ERD syntax copied to clipboard!', 'success');
  }
</script>

<div class="flex flex-col h-full overflow-hidden p-4 max-w-7xl mx-auto w-full space-y-4">
  <!-- Header -->
  <div class="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3 shrink-0">
    <div class="flex items-center gap-3">
      <h2 class="text-sm font-bold text-neutral-900 dark:text-white">Schema Entity Relationship Diagram</h2>
      <span class="text-[10px] font-mono bg-neutral-100 dark:bg-neutral-800 text-neutral-500 dark:text-neutral-400 px-2 py-0.5 rounded">
        {dbStore.tables.length} Entities Mapped
      </span>
    </div>

    <button
      onclick={copyMermaid}
      class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold text-neutral-700 dark:text-neutral-200 bg-white dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 rounded-lg shadow-sm hover:bg-neutral-50 dark:hover:bg-neutral-700 transition-colors cursor-pointer"
    >
      <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" /></svg>
      Copy Mermaid Diagram
    </button>
  </div>

  <!-- ERD Canvas -->
  <div class="flex-1 overflow-auto bg-neutral-50 dark:bg-[#12141a] border border-neutral-200 dark:border-neutral-800 rounded-2xl p-6 relative">
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
      {#each dbStore.tables as table}
        <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl shadow-sm overflow-hidden flex flex-col">
          <div class="px-3.5 py-2.5 bg-blue-50 dark:bg-blue-950/40 border-b border-blue-100 dark:border-blue-900/50 flex items-center justify-between">
            <span class="font-bold text-xs text-blue-900 dark:text-blue-300 font-mono">{table.name}</span>
            <span class="text-[9px] uppercase px-1.5 py-0.5 bg-blue-200/50 dark:bg-blue-800/50 text-blue-800 dark:text-blue-200 rounded font-semibold">{table.columns.length} cols</span>
          </div>

          <div class="p-2.5 space-y-1.5 text-xs">
            {#each table.columns as col}
              <div class="flex items-center justify-between py-1 px-1.5 rounded hover:bg-neutral-50 dark:hover:bg-neutral-800/40">
                <span class="font-mono text-[11px] text-neutral-800 dark:text-neutral-200 flex items-center gap-1.5">
                  {#if col.is_pk}
                    <span class="text-amber-500 font-bold text-[10px]" title="Primary Key">🔑</span>
                  {:else if col.name.endsWith('_id')}
                    <span class="text-blue-500 font-bold text-[10px]" title="Foreign Key">🔗</span>
                  {:else}
                    <span class="text-neutral-300 dark:text-neutral-600 text-[10px]">·</span>
                  {/if}
                  {col.name}
                </span>
                <span class="font-mono text-[10px] text-neutral-400">{col.data_type}</span>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  </div>
</div>
