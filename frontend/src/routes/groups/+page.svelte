<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { dbStore, type ConnectionConfig } from '$lib/stores/database.svelte';
  import { showToast, confirmModal } from '$lib/stores/uiNotifications.svelte';
  import { goto } from '$app/navigation';
  import EmptyState from '$lib/components/EmptyState.svelte';

  export interface WorkspaceRecord {
    id: string;
    name: string;
    color: string;
    description: Option<string>;
    created_at: number;
  }

  type Option<T> = T | null | undefined;

  let workspaces = $state<WorkspaceRecord[]>([]);
  let searchQuery = $state('');
  let isLoading = $state(false);

  // Add/Edit Modal
  let isModalOpen = $state(false);
  let editingId = $state<string | null>(null);
  let formName = $state('');
  let formColor = $state('#3b82f6');
  let formDesc = $state('');

  const PALETTE = ['#ef4444', '#f97316', '#f59e0b', '#10b981', '#06b6d4', '#3b82f6', '#8b5cf6', '#ec4899'];

  const filteredWorkspaces = $derived.by(() => {
    if (!searchQuery.trim()) return workspaces;
    const q = searchQuery.toLowerCase().trim();
    return workspaces.filter(
      (w) => w.name.toLowerCase().includes(q) || (w.description || '').toLowerCase().includes(q)
    );
  });

  function getConnectionsForWorkspace(name: string): ConnectionConfig[] {
    return dbStore.connections.filter((c) => (c.group_name || '').trim() === name.trim());
  }

  const ungroupedConnections = $derived.by(() => {
    return dbStore.connections.filter((c) => !c.group_name || !c.group_name.trim());
  });

  async function loadWorkspaces() {
    isLoading = true;
    try {
      const list = await invoke<WorkspaceRecord[]>('list_workspaces');
      workspaces = list || [];
    } catch (e: any) {
      console.warn('Failed to load workspaces from vault:', e);
      // Fallback: derive from existing connection group_names if table empty
      const derived = new Set<string>();
      for (const c of dbStore.connections) {
        if (c.group_name?.trim()) derived.add(c.group_name.trim());
      }
      workspaces = Array.from(derived).map((name, i) => ({
        id: `derived-${i}`,
        name,
        color: PALETTE[i % PALETTE.length],
        description: 'Existing connection cluster',
        created_at: Date.now(),
      }));
    } finally {
      isLoading = false;
    }
  }

  onMount(async () => {
    await dbStore.loadConnections();
    await loadWorkspaces();
  });

  function openAdd() {
    editingId = null;
    formName = '';
    formColor = PALETTE[Math.floor(Math.random() * PALETTE.length)];
    formDesc = '';
    isModalOpen = true;
  }

  function openEdit(w: WorkspaceRecord) {
    editingId = w.id;
    formName = w.name;
    formColor = w.color || '#3b82f6';
    formDesc = w.description || '';
    isModalOpen = true;
  }

  async function handleSave() {
    if (!formName.trim()) {
      showToast('Workspace name is required', 'error');
      return;
    }

    try {
      await invoke('save_workspace', {
        input: {
          id: editingId,
          name: formName.trim(),
          color: formColor,
          description: formDesc.trim() || null,
        },
      });
      showToast(editingId ? 'Workspace updated' : 'Workspace created', 'success');
      isModalOpen = false;
      await loadWorkspaces();
      await dbStore.loadConnections();
    } catch (e: any) {
      showToast(e?.message || 'Failed to save workspace', 'error');
    }
  }

  async function handleDelete(w: WorkspaceRecord) {
    const memberCount = getConnectionsForWorkspace(w.name).length;
    const ok = await confirmModal(
      `Delete workspace "${w.name}"? ${memberCount > 0 ? `${memberCount} connections will become ungrouped.` : ''}`,
      'Delete Workspace',
      true,
      'Delete',
      'Cancel'
    );
    if (!ok) return;

    try {
      await invoke('delete_workspace', { id: w.id });
      showToast(`Workspace "${w.name}" deleted`, 'success');
      await loadWorkspaces();
      await dbStore.loadConnections();
    } catch (e: any) {
      showToast(e?.message || 'Failed to delete workspace', 'error');
    }
  }

  async function handleConnect(c: ConnectionConfig) {
    await dbStore.connect(c);
    showToast(`Connected to "${c.name}"`, 'success');
    void goto('/table-browser');
  }

  async function handleExportJson() {
    try {
      const jsonStr = await invoke<string>('export_json_backup');
      const blob = new Blob([jsonStr], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `catable-workspaces-${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 2000);
      showToast('Workspaces and vault backup exported', 'success');
    } catch (e: any) {
      showToast(e?.message || 'Failed to export backup', 'error');
    }
  }

  function handleImportJson() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.json';
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      try {
        const text = await file.text();
        const count = await invoke<number>('import_json_backup', { jsonStr: text });
        showToast(`Restored ${count} items from backup`, 'success');
        await loadWorkspaces();
        await dbStore.loadConnections();
      } catch (e: any) {
        showToast(e?.message || 'Failed to restore backup', 'error');
      }
    };
    input.click();
  }
</script>

<div class="max-w-6xl mx-auto space-y-6 pb-12">
  <!-- Header -->
  <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-4 border-b border-neutral-200 dark:border-neutral-800">
    <div class="flex items-center gap-3">
      <div class="w-10 h-10 rounded-xl bg-blue-500/10 text-blue-500 border border-blue-500/20 flex items-center justify-center font-bold">
        <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
        </svg>
      </div>
      <div>
        <h1 class="text-xl font-bold tracking-tight text-neutral-900 dark:text-white">Workspaces & Groups</h1>
        <p class="text-xs text-neutral-500 dark:text-neutral-400">
          Organize database connections into clusters, client environments, or teams ({workspaces.length} workspaces)
        </p>
      </div>
    </div>

    <!-- Actions toolbar -->
    <div class="flex items-center gap-2 flex-wrap">
      <button
        type="button"
        onclick={handleImportJson}
        class="px-3 py-1.5 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-700 dark:text-neutral-300 transition-colors flex items-center gap-1.5 cursor-pointer"
        title="Import Workspaces / Connections from JSON"
      >
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-8l-4-4m0 0L8 8m4-4v12" />
        </svg>
        <span>Import</span>
      </button>

      <button
        type="button"
        onclick={handleExportJson}
        class="px-3 py-1.5 text-xs font-semibold rounded-lg border border-neutral-300 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-700 dark:text-neutral-300 transition-colors flex items-center gap-1.5 cursor-pointer"
        title="Export All to JSON Backup"
      >
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
        </svg>
        <span>Export</span>
      </button>

      <button
        type="button"
        onclick={openAdd}
        class="px-3.5 py-1.5 text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 rounded-lg shadow-sm transition-colors flex items-center gap-1.5 cursor-pointer"
      >
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.5" d="M12 4v16m8-8H4" />
        </svg>
        <span>New Workspace</span>
      </button>
    </div>
  </div>

  <!-- Search Filter -->
  <div class="flex items-center gap-2">
    <div class="relative flex-1 max-w-md">
      <input
        type="text"
        bind:value={searchQuery}
        placeholder="Filter workspaces..."
        class="w-full text-xs pl-8 pr-3 py-2 bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-800 dark:text-neutral-200"
      />
      <svg class="w-4 h-4 absolute left-2.5 top-2.5 text-neutral-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
      </svg>
    </div>
  </div>

  <!-- Workspaces Grid -->
  {#if filteredWorkspaces.length === 0 && ungroupedConnections.length === 0}
    <EmptyState
      title="No Workspaces Yet"
      description="Create workspaces to group and segment your database connections."
      icon="connections"
      actionLabel="Create Workspace"
      onaction={openAdd}
    />
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
      {#each filteredWorkspaces as ws (ws.id)}
        {@const members = getConnectionsForWorkspace(ws.name)}
        <div class="p-5 rounded-2xl border border-neutral-200/90 dark:border-neutral-800 bg-white dark:bg-neutral-900/90 shadow-2xs hover:shadow-md transition-all duration-150 flex flex-col justify-between space-y-4 text-left">
          <!-- Top Row -->
          <div class="flex items-start justify-between gap-3">
            <div class="flex items-center gap-3 min-w-0">
              <div
                class="w-10 h-10 rounded-xl flex items-center justify-center shrink-0 shadow-xs text-white font-bold"
                style="background-color: {ws.color || '#3b82f6'};"
              >
                <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
                </svg>
              </div>

              <div class="min-w-0">
                <h3 class="font-bold text-sm text-neutral-900 dark:text-neutral-100 truncate">{ws.name}</h3>
                {#if ws.description}
                  <p class="text-xs text-neutral-500 dark:text-neutral-400 truncate mt-0.5">{ws.description}</p>
                {/if}
              </div>
            </div>

            <!-- Workspace Actions -->
            <div class="flex items-center gap-1 shrink-0">
              <button
                type="button"
                onclick={() => openEdit(ws)}
                class="p-1.5 rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-400 hover:text-blue-500 transition-colors"
                title="Edit Workspace"
              >
                <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z" />
                </svg>
              </button>

              <button
                type="button"
                onclick={() => handleDelete(ws)}
                class="p-1.5 rounded-lg hover:bg-rose-50 dark:hover:bg-rose-950/40 text-neutral-400 hover:text-rose-500 transition-colors"
                title="Delete Workspace"
              >
                <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                </svg>
              </button>
            </div>
          </div>

          <!-- Member Connections Preview -->
          <div class="space-y-1.5 pt-2 border-t border-neutral-100 dark:border-neutral-800/80">
            <div class="text-[10px] uppercase font-mono font-bold text-neutral-400 flex justify-between items-center">
              <span>Databases</span>
              <span>{members.length} active</span>
            </div>

            {#if members.length === 0}
              <p class="text-xs text-neutral-400 italic py-1">No connections assigned to this workspace</p>
            {:else}
              <div class="space-y-1 max-h-32 overflow-y-auto scrollbar-none">
                {#each members as conn (conn.id)}
                  <div class="flex items-center justify-between p-1.5 rounded-lg bg-neutral-50 dark:bg-neutral-800/50 text-xs">
                    <span class="font-medium text-neutral-800 dark:text-neutral-200 truncate">{conn.name}</span>
                    <button
                      type="button"
                      onclick={() => handleConnect(conn)}
                      class="text-[10px] text-blue-500 hover:underline font-mono"
                    >
                      Connect →
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>

          <!-- Bottom Footer -->
          <div class="pt-2 border-t border-neutral-100 dark:border-neutral-800/80 flex items-center justify-between text-[11px] text-neutral-500">
            <a
              href="/?workspace={encodeURIComponent(ws.name)}"
              class="text-blue-500 hover:text-blue-600 font-medium"
            >
              Filter in Workbench →
            </a>
            <span class="font-mono text-[10px] opacity-75">{new Date(ws.created_at * 1000).toLocaleDateString()}</span>
          </div>
        </div>
      {/each}

      <!-- Ungrouped Card -->
      {#if ungroupedConnections.length > 0}
        <div class="p-5 rounded-2xl border border-dashed border-neutral-300 dark:border-neutral-700 bg-neutral-50/50 dark:bg-neutral-900/40 flex flex-col justify-between space-y-4 text-left">
          <div class="flex items-start justify-between gap-3">
            <div class="flex items-center gap-3">
              <div class="w-10 h-10 rounded-xl bg-neutral-200 dark:bg-neutral-800 flex items-center justify-center text-neutral-500 font-bold">
                <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4" />
                </svg>
              </div>
              <div>
                <h3 class="font-bold text-sm text-neutral-900 dark:text-neutral-100">Ungrouped Databases</h3>
                <p class="text-xs text-neutral-500">{ungroupedConnections.length} connections without a workspace</p>
              </div>
            </div>
          </div>

          <div class="space-y-1.5 pt-2 border-t border-neutral-200 dark:border-neutral-800">
            {#each ungroupedConnections.slice(0, 4) as conn (conn.id)}
              <div class="flex items-center justify-between p-1.5 rounded-lg bg-white dark:bg-neutral-800 text-xs">
                <span class="truncate">{conn.name}</span>
                <button
                  type="button"
                  onclick={() => handleConnect(conn)}
                  class="text-[10px] text-blue-500 hover:underline font-mono"
                >
                  Connect →
                </button>
              </div>
            {/each}
          </div>

          <div class="pt-2 border-t border-neutral-200 dark:border-neutral-800 text-xs text-blue-500">
            <a href="/?workspace=ungrouped" class="hover:underline">View all ungrouped →</a>
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<!-- Add / Edit Workspace Modal -->
{#if isModalOpen}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-xs p-4">
    <div class="w-full max-w-md bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-2xl p-6 shadow-2xl space-y-5 text-neutral-800 dark:text-neutral-200">
      <div class="flex items-center justify-between pb-3 border-b border-neutral-200 dark:border-neutral-800">
        <h3 class="text-base font-bold text-neutral-900 dark:text-white">
          {editingId ? 'Edit Workspace' : 'Create New Workspace'}
        </h3>
        <button
          type="button"
          onclick={() => (isModalOpen = false)}
          class="p-1 rounded text-neutral-400 hover:text-neutral-900 dark:hover:text-white"
        >
          ✕
        </button>
      </div>

      <div class="space-y-4 text-xs">
        <div>
          <label for="ws-name" class="block font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-1">
            Workspace Name
          </label>
          <input
            id="ws-name"
            type="text"
            bind:value={formName}
            placeholder="e.g. Production Cluster, Analytics, Dev Sandbox"
            class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
          />
        </div>

        <div>
          <span class="block font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-2">
            Badge Accent Color
          </span>
          <div class="flex items-center gap-2 flex-wrap">
            {#each PALETTE as color}
              <button
                type="button"
                onclick={() => (formColor = color)}
                class="w-7 h-7 rounded-full transition-transform cursor-pointer {formColor === color ? 'scale-125 ring-2 ring-white ring-offset-2 ring-offset-neutral-900' : 'hover:scale-110'}"
                style="background-color: {color};"
                aria-label="Color {color}"
              ></button>
            {/each}
          </div>
        </div>

        <div>
          <label for="ws-desc" class="block font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-1">
            Description (Optional)
          </label>
          <textarea
            id="ws-desc"
            bind:value={formDesc}
            rows="3"
            placeholder="Purpose, environment notes, or team assignment..."
            class="w-full px-3 py-2 bg-neutral-50 dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 text-neutral-900 dark:text-white"
          ></textarea>
        </div>
      </div>

      <div class="flex items-center justify-end gap-2 pt-3 border-t border-neutral-200 dark:border-neutral-800">
        <button
          type="button"
          onclick={() => (isModalOpen = false)}
          class="px-4 py-2 text-xs font-semibold rounded-lg hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-600 dark:text-neutral-300"
        >
          Cancel
        </button>
        <button
          type="button"
          onclick={handleSave}
          class="px-4 py-2 text-xs font-semibold rounded-lg bg-blue-600 hover:bg-blue-500 text-white shadow-sm"
        >
          {editingId ? 'Save Changes' : 'Create Workspace'}
        </button>
      </div>
    </div>
  </div>
{/if}
