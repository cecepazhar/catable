<script lang="ts">
  // CATable Pro sidebar card: dynamic feature list + a live Synced/Unsynced indicator.
  import { getPro } from '$lib/stores/pro.svelte';

  let { compact = false } = $props();

  const pro = getPro();

  const features = $derived([
    { id: 'cloud_sync', label: 'Cloud Sync', tip: 'Zero-Knowledge E2EE Sync', active: pro.isSyncConnected },
    { id: 'ssh_bastion', label: 'SSH Bastion', tip: '1-Click Bastion Tunnels', active: pro.isBastionAvailable },
    { id: 'blast_shield', label: 'Blast-Radius Shield', tip: 'Dry-Run Simulation', active: pro.blastRadiusShieldAvailable },
    { id: 'erd_diff', label: 'ERD Diff', tip: 'Visual Schema Migrations', active: pro.isPro },
    { id: 'ai_copilot', label: 'AI DBA Copilot', tip: 'Context-Aware SQL Assistant', active: pro.isPro },
    { id: 'pii_masking', label: 'PII Masking', tip: 'Automatic Sensitive Data Redaction', active: pro.isPiiMaskingEnabled }
  ]);

  const syncActive = $derived(pro.isSyncConnected);
</script>

<div
  class="rounded-xl border border-neutral-200/90 dark:border-neutral-800 bg-white/80 dark:bg-[#1a1a1a]/70 backdrop-blur p-3 space-y-2.5 select-none"
  aria-label="CATable Pro"
>
  <div class="flex items-center justify-between gap-2">
    <div class="flex items-center gap-2 min-w-0">
      <span class="w-6 h-6 rounded-lg bg-gradient-to-br from-sky-500 to-indigo-600 flex items-center justify-center text-white shadow-sm shrink-0" aria-hidden="true">
        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4M7.835 4.697a3.42 3.42 0 001.946-.806 3.42 3.42 0 014.438 0 3.42 3.42 0 001.946.806 3.42 3.42 0 013.138 3.138 3.42 3.42 0 00.806 1.946 3.42 3.42 0 010 4.438 3.42 3.42 0 00-.806 1.946 3.42 3.42 0 01-3.138 3.138 3.42 3.42 0 00-1.946.806 3.42 3.42 0 01-4.438 0 3.42 3.42 0 00-1.946-.806 3.42 3.42 0 01-3.138-3.138 3.42 3.42 0 00-.806-1.946 3.42 3.42 0 010-4.438 3.42 3.42 0 00.806-1.946 3.42 3.42 0 013.138-3.138z" />
        </svg>
      </span>
      <span class="text-[11px] font-bold uppercase tracking-wider text-neutral-900 dark:text-white">CATable Pro</span>
    </div>
    <!-- Visual 'Synced' / 'Unsynced' indicator -->
    <div class="flex items-center gap-1.5 shrink-0" title={syncActive ? 'Vault Synced' : 'Vault Unsynced'}>
      <span
        class="w-2 h-2 rounded-full {syncActive ? 'bg-emerald-500 shadow-emerald-500/50 animate-pulse' : 'bg-amber-500 shadow-amber-500/50'}"
        aria-hidden="true"
      ></span>
      <span class="text-[10px] font-semibold text-neutral-600 dark:text-neutral-400">{syncActive ? 'Synced' : 'Unsynced'}</span>
    </div>
  </div>

  {#if !compact}
    <div class="flex flex-col gap-1">
      {#each features as feat (feat.id)}
        <div class="flex items-center justify-between gap-2 group">
          <div class="flex items-center gap-2 min-w-0">
            <span
              class="w-1.5 h-1.5 rounded-full shrink-0 {feat.active ? 'bg-emerald-500' : 'bg-neutral-300 dark:bg-neutral-700'}"
              aria-hidden="true"
            ></span>
            <span class="text-[11px] font-medium truncate {feat.active ? 'text-neutral-800 dark:text-neutral-200' : 'text-neutral-500 dark:text-neutral-500'}" title={feat.tip}>
              {feat.label}
            </span>
          </div>
          {#if feat.active}
            <svg class="w-3.5 h-3.5 text-emerald-500/80 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" /></svg>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
