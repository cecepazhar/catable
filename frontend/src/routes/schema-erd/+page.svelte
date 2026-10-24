<script lang="ts">
  import { onMount } from 'svelte';
  import { dbStore } from '$lib/stores/database.svelte';
  import { showToast } from '$lib/stores/uiNotifications.svelte';

  interface ForeignKeyRelation {
    fromTable: string;
    fromColumn: string;
    toTable: string;
    toColumn: string;
  }

  let relationships = $state<ForeignKeyRelation[]>([]);
  let isErdLoading = $state(false);

  onMount(async () => {
    await dbStore.loadTables();
    await extractForeignKeys();
  });

  function buildRelationSql(tableName: string): string | null {
    const driver = dbStore.activeConnection.driver;
    const escapedName = `"${tableName.replace(/"/g, '""')}"`;

    switch (driver) {
      case 'sqlite':
        // PRAGMA foreign_key_list returns: id, seq, table, from, to, on_update, on_delete, match
        return `SELECT "table" AS to_table, "from" AS from_col, "to" AS to_col FROM pragma_foreign_key_list(${escapedName})`;
      case 'postgres':
      case 'duckdb':
        return `
          SELECT DISTINCT tc.table_name AS from_table, kcu.column_name AS from_column,
                 ccu.table_name AS to_table, ccu.column_name AS to_column
          FROM information_schema.table_constraints tc
          JOIN information_schema.key_column_usage kcu
            ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema
          JOIN information_schema.constraint_column_usage ccu
            ON tc.constraint_name = ccu.constraint_name AND tc.table_schema = ccu.table_schema
          WHERE tc.constraint_type = 'FOREIGN KEY' AND tc.table_name = '${tableName}'
        `;
      case 'mysql':
      case 'mariadb':
        return `
          SELECT DISTINCT TABLE_NAME AS from_table, COLUMN_NAME AS from_column,
                 REFERENCED_TABLE_NAME AS to_table, REFERENCED_COLUMN_NAME AS to_column
          FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE
          WHERE TABLE_SCHEMA = DATABASE() AND REFERENCED_TABLE_NAME IS NOT NULL AND TABLE_NAME = '${tableName}'
        `;
      default:
        // Fallback for drivers without schema inspection (e.g. Redis): use naming convention
        return null;
    }
  }

  async function extractForeignKeys() {
    if (dbStore.tables.length === 0) {
      relationships = [];
      return;
    }

    isErdLoading = true;
    const foundRelations: ForeignKeyRelation[] = [];
    const seenKeys = new Set<string>();

    // 1. Attempt to read foreign keys from the database engine itself
    const driver = dbStore.activeConnection.driver;
    if (driver === 'sqlite' || driver === 'postgres' || driver === 'mysql' || driver === 'mariadb' || driver === 'duckdb') {
      for (const table of dbStore.tables) {
        if (table.table_type !== 'table') continue;
        const sql = buildRelationSql(table.name);
        if (!sql) continue;
        try {
          const res = await dbStore.executeQuery(sql);
          if (!res.error && res.rows.length > 0) {
            for (const row of res.rows) {
              if (driver === 'sqlite') {
                // SQLite PRAGMA returns: [to_table, from_col, to_col]
                const toTable = String(row[0] || '').trim();
                const fromCol = String(row[1] || '').trim();
                const toCol = String(row[2] || '').trim();
                if (toTable && fromCol && toCol) {
                  const rel: ForeignKeyRelation = {
                    fromTable: table.name,
                    fromColumn: fromCol,
                    toTable: toTable,
                    toColumn: toCol,
                  };
                  const key = `${rel.fromTable}.${rel.fromColumn}->${rel.toTable}.${rel.toColumn}`;
                  if (!seenKeys.has(key)) {
                    seenKeys.add(key);
                    foundRelations.push(rel);
                  }
                }
              } else {
                const fromTable = String(row[0] || '').trim();
                const fromCol = String(row[1] || '').trim();
                const toTable = String(row[2] || '').trim();
                const toCol = String(row[3] || '').trim();
                if (fromTable && fromCol && toTable && toCol) {
                  const rel: ForeignKeyRelation = {
                    fromTable,
                    fromColumn: fromCol,
                    toTable,
                    toColumn: toCol,
                  };
                  const key = `${rel.fromTable}.${rel.fromColumn}->${rel.toTable}.${rel.toColumn}`;
                  if (!seenKeys.has(key)) {
                    seenKeys.add(key);
                    foundRelations.push(rel);
                  }
                }
              }
            }
          }
        } catch (e) {
          console.warn(`Failed to extract foreign keys for ${table.name}`, e);
        }
      }
    }

    // 2. Naming convention fallback: <table_name>_id columns link to <table_name>
    if (foundRelations.length === 0) {
      const tableNames = new Set(dbStore.tables.map((t) => t.name.toLowerCase()));
      for (const table of dbStore.tables) {
        for (const col of table.columns) {
          if (!col.name.endsWith('_id') || col.name === 'id') continue;
          const targetName = col.name.slice(0, -3); // strip _id
          const pluralTarget = `${targetName}s`;
          let resolvedTarget = '';
          if (tableNames.has(pluralTarget.toLowerCase())) {
            resolvedTarget = pluralTarget;
          } else if (tableNames.has(targetName.toLowerCase())) {
            resolvedTarget = targetName;
          }
          if (resolvedTarget && resolvedTarget.toLowerCase() !== table.name.toLowerCase()) {
            const rel: ForeignKeyRelation = {
              fromTable: table.name,
              fromColumn: col.name,
              toTable: resolvedTarget,
              toColumn: 'id',
            };
            const key = `${rel.fromTable}.${rel.fromColumn}->${rel.toTable}.${rel.toColumn}`;
            if (!seenKeys.has(key)) {
              seenKeys.add(key);
              foundRelations.push(rel);
            }
          }
        }
      }
    }

    relationships = foundRelations;
    isErdLoading = false;
  }

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
    for (const rel of relationships) {
      // Mermaid syntax: <fromTable> ||--o{ <toTable> : <label>
      mermaid += `    ${rel.fromTable} ||--o{ ${rel.toTable} : ${rel.fromColumn}\n`;
    }

    void navigator.clipboard.writeText(mermaid);
    showToast('Dynamic Mermaid ERD syntax copied to clipboard!', 'success');
  }

  function isForeignCol(tableName: string, colName: string): boolean {
    return relationships.some(
      (r) => r.fromTable === tableName && r.fromColumn === colName
    );
  }

  function getRelationshipLabel(tableName: string, colName: string): string {
    const rel = relationships.find((r) => r.fromTable === tableName && r.fromColumn === colName);
    if (rel) {
      return `REFERENCES ${rel.toTable}.${rel.toColumn}`;
    }
    return '';
  }
</script>

<div class="flex flex-col h-full overflow-hidden p-4 max-w-7xl mx-auto w-full space-y-4">
  <!-- Header -->
  <div class="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3 shrink-0">
    <div class="flex items-center gap-3">
      <h2 class="text-sm font-bold text-neutral-900 dark:text-white">Schema Entity Relationship Diagram</h2>
      <span class="text-[10px] font-mono bg-neutral-100 dark:bg-neutral-800 text-neutral-500 dark:text-neutral-400 px-2 py-0.5 rounded">
        {dbStore.tables.length} Entities Mapped · {relationships.length} Foreign Key Relations
      </span>
      {#if isErdLoading}
        <span class="text-[10px] text-blue-500 font-medium animate-pulse">Extracting schema...</span>
      {/if}
    </div>

    <div class="flex items-center gap-2">
      <button
        onclick={() => extractForeignKeys()}
        class="px-2.5 py-1.5 text-xs font-medium text-neutral-600 dark:text-neutral-300 bg-white dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 rounded-lg shadow-sm hover:bg-neutral-50 dark:hover:bg-neutral-700 transition-colors cursor-pointer"
      >
        ↻ Re-Extract
      </button>
      <button
        onclick={copyMermaid}
        class="flex items-center gap-1.5 px-3 py-1.5 text-xs font-semibold text-neutral-700 dark:text-neutral-200 bg-white dark:bg-neutral-800 border border-neutral-300 dark:border-neutral-700 rounded-lg shadow-sm hover:bg-neutral-50 dark:hover:bg-neutral-700 transition-colors cursor-pointer"
      >
        <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" /></svg>
        Copy Mermaid Diagram
      </button>
    </div>
  </div>

  <!-- ERD Canvas -->
  <div class="flex-1 overflow-auto bg-neutral-50 dark:bg-[#12141a] border border-neutral-200 dark:border-neutral-800 rounded-2xl p-6 relative">
    {#if dbStore.tables.length === 0}
      <div class="h-full flex items-center justify-center text-neutral-400 text-xs">No tables found for this database. Create one in SQL Studio to generate an Entity Relationship Diagram.</div>
    {:else}
      <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
        {#each dbStore.tables as table}
          <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl shadow-sm overflow-hidden flex flex-col">
            <div class="px-3.5 py-2.5 bg-blue-50 dark:bg-blue-950/40 border-b border-blue-100 dark:border-blue-900/50 flex items-center justify-between">
              <span class="font-bold text-xs text-blue-900 dark:text-blue-300 font-mono">{table.name}</span>
              <span class="text-[9px] uppercase px-1.5 py-0.5 bg-blue-200/50 dark:bg-blue-800/50 text-blue-800 dark:text-blue-200 rounded font-semibold">{table.columns.length} cols</span>
            </div>

            <div class="p-2.5 space-y-1.5 text-xs">
              {#each table.columns as col}
                {@const isFK = isForeignCol(table.name, col.name)}
                <div class="flex items-center justify-between py-1 px-1.5 rounded hover:bg-neutral-50 dark:hover:bg-neutral-800/40">
                  <span class="font-mono text-[11px] text-neutral-800 dark:text-neutral-200 flex items-center gap-1.5">
                    {#if col.is_pk}
                      <span class="text-amber-500 font-bold text-[10px]" title="Primary Key">🔑</span>
                    {:else if isFK}
                      <span class="text-blue-500 font-bold text-[10px]" title={getRelationshipLabel(table.name, col.name)}>🔗</span>
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
    {/if}
  </div>
</div>
