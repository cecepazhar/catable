<script lang="ts">
  import { dbStore, type QueryResult } from '$lib/stores/database.svelte';
  import { showToast } from '$lib/stores/uiNotifications.svelte';
  import { aiChat, aiExplain, type AiExplainResponse } from '$lib/api/copilot';

  let prompt = $state('');
  let result = $state<QueryResult | null>(null);
  let isLoading = $state(false);
  let isExplaining = $state(false);
  let responseText = $state('');
  let currentSql = $state('');
  let explainData = $state<AiExplainResponse | null>(null);

  const suggestions = [
    'Write a query to find top 5 products by revenue',
    'Show me all users and their total order amounts',
    'Find any duplicate emails in the users table',
    'Generate an INSERT statement for a new user with developer role'
  ];

  async function generateQuery() {
    if (!prompt.trim()) return;
    isLoading = true;
    responseText = '';
    currentSql = '';
    result = null;
    explainData = null;

    try {
      // Connect to real 9Router (localhost:20128) or Ollama with dynamic schema prompt injection
      const chatRes = await aiChat(prompt, dbStore.activeConnection);
      responseText = chatRes.reply;

      if (chatRes.sql) {
        currentSql = chatRes.sql;

        // Evaluate with safety guardrail
        const guardVerdict = await dbStore.evaluateSafety(chatRes.sql);
        if (guardVerdict.risk === 'blocked') {
          showToast(`Generated query was blocked: ${guardVerdict.reason}`, 'error');
        }

        // Execute query on active database
        const execRes = await dbStore.executeQuery(chatRes.sql);
        result = execRes;
        if (execRes.error) {
          showToast(`Query execution error: ${execRes.error}`, 'error');
        }
      }
    } catch (e: any) {
      showToast(e?.message || 'AI generation failed.', 'error');
      responseText = `Failed to connect to AI Copilot: ${e?.message || e}. Ensure 9Router is active on localhost:20128 or Ollama is running.`;
    } finally {
      isLoading = false;
    }
  }

  async function handleExplain() {
    if (!currentSql) return;
    isExplaining = true;
    try {
      const exp = await aiExplain(currentSql, dbStore.activeConnection);
      explainData = exp;
      showToast('Query plan & index analysis ready.', 'success');
    } catch (e: any) {
      showToast(e?.message || 'Explain query failed.', 'error');
    } finally {
      isExplaining = false;
    }
  }

  async function executeCurrentSql() {
    if (!currentSql) return;
    isLoading = true;
    try {
      const guardVerdict = await dbStore.evaluateSafety(currentSql);
      if (guardVerdict.risk === 'blocked') {
        showToast(`Query blocked by guardrail: ${guardVerdict.reason}`, 'error');
        return;
      }
      const execRes = await dbStore.executeQuery(currentSql);
      result = execRes;
      if (execRes.error) {
        showToast(`Error: ${execRes.error}`, 'error');
      } else {
        showToast(`Executed in ${execRes.duration_ms}ms (${execRes.rows_affected} rows)`, 'success');
      }
    } catch (e: any) {
      showToast(e?.message || 'Execution failed.', 'error');
    } finally {
      isLoading = false;
    }
  }

  function copySql() {
    if (!currentSql) return;
    navigator.clipboard.writeText(currentSql);
    showToast('SQL copied to clipboard.', 'success');
  }
</script>

<div class="flex flex-col h-full overflow-hidden p-4 max-w-5xl mx-auto w-full space-y-4">
  <div class="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3 shrink-0">
    <div class="flex items-center gap-3">
      <h2 class="text-sm font-bold text-neutral-900 dark:text-white">AI DBA Copilot</h2>
      <span class="text-[10px] font-mono bg-purple-100 dark:bg-purple-900/40 text-purple-600 dark:text-purple-300 px-2 py-0.5 rounded">
        9ROUTER / OLLAMA
      </span>
      <span class="text-[10px] font-mono bg-emerald-100 dark:bg-emerald-900/40 text-emerald-600 dark:text-emerald-300 px-2 py-0.5 rounded">
        SCHEMA-INJECTED ({dbStore.activeConnection.name} • {dbStore.tables.length} tables)
      </span>
    </div>
  </div>

  <div class="flex-1 flex flex-col gap-4 overflow-y-auto">
    <!-- Suggestions -->
    <div class="flex flex-wrap gap-2">
      {#each suggestions as sugg}
        <button
          onclick={() => (prompt = sugg)}
          class="px-3 py-1 text-[11px] font-medium bg-purple-50 dark:bg-purple-900/30 text-purple-600 dark:text-purple-300 border border-purple-100 dark:border-purple-800/50 rounded-full hover:bg-purple-100 dark:hover:bg-purple-900/50 transition-colors cursor-pointer"
        >
          {sugg}
        </button>
      {/each}
    </div>

    <!-- Prompt Input -->
    <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-4 flex gap-3 items-center">
      <div class="flex-1">
        <textarea
          bind:value={prompt}
          placeholder="Describe the query you need, ask for performance advice, or request schema changes..."
          rows={2}
          class="w-full bg-transparent focus:outline-none text-xs text-neutral-900 dark:text-neutral-100 resize-none font-sans"
        ></textarea>
      </div>
      <button
        disabled={isLoading || !prompt.trim()}
        onclick={generateQuery}
        class="px-4 py-2 text-xs font-semibold text-white bg-purple-600 hover:bg-purple-500 rounded-lg shadow-sm transition-colors disabled:opacity-50 cursor-pointer"
      >
        {isLoading ? 'Reasoning…' : 'Generate SQL'}
      </button>
    </div>

    {#if isLoading}
      <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-6 flex items-center justify-center gap-3 text-xs text-neutral-500">
        <div class="w-4 h-4 border-2 border-purple-500 border-t-transparent rounded-full animate-spin"></div>
        AI DBA is inspecting active schema ({dbStore.activeConnection.driver}) and formulating query...
      </div>
    {:else if responseText}
      <div class="space-y-4">
        <!-- AI Response & Reasoning -->
        <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-4">
          <div class="flex items-center justify-between mb-2">
            <div class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">AI Copilot Analysis</div>
            {#if currentSql}
              <div class="flex items-center gap-2">
                <button
                  disabled={isExplaining}
                  onclick={handleExplain}
                  class="text-[11px] font-medium text-purple-600 hover:text-purple-500 flex items-center gap-1 cursor-pointer"
                >
                  {isExplaining ? 'Analyzing plan…' : 'Explain SQL & Index Hints'}
                </button>
                <span class="text-neutral-300 dark:text-neutral-700">•</span>
                <button
                  onclick={copySql}
                  class="text-[11px] font-medium text-neutral-600 dark:text-neutral-400 hover:text-neutral-900 dark:hover:text-neutral-200 cursor-pointer"
                >
                  Copy SQL
                </button>
                <span class="text-neutral-300 dark:text-neutral-700">•</span>
                <button
                  onclick={executeCurrentSql}
                  class="text-[11px] font-medium text-emerald-600 hover:text-emerald-500 cursor-pointer"
                >
                  Execute Query
                </button>
              </div>
            {/if}
          </div>
          <p class="text-xs text-neutral-700 dark:text-neutral-300 leading-relaxed whitespace-pre-wrap">{responseText}</p>
        </div>

        <!-- Generated SQL Card -->
        {#if currentSql}
          <div class="bg-neutral-950 border border-neutral-800 rounded-xl p-4 font-mono text-xs text-emerald-400 overflow-x-auto">
            <div class="flex items-center justify-between pb-2 border-b border-neutral-800 text-[10px] text-neutral-400 font-sans font-medium uppercase mb-2">
              <span>Generated SQL Statement</span>
              <button onclick={copySql} class="hover:text-white cursor-pointer">Copy</button>
            </div>
            <pre class="whitespace-pre">{currentSql}</pre>
          </div>
        {/if}

        <!-- Explain & Index Advice -->
        {#if explainData}
          <div class="bg-purple-50/50 dark:bg-purple-950/20 border border-purple-200 dark:border-purple-800/40 rounded-xl p-4 space-y-3">
            <div class="text-[10px] font-bold text-purple-600 dark:text-purple-400 uppercase tracking-wider">
              Query Execution Plan & Optimization
            </div>
            <p class="text-xs text-neutral-800 dark:text-neutral-200 leading-relaxed whitespace-pre-wrap">
              {explainData.explanation}
            </p>
            {#if explainData.index_suggestions.length > 0}
              <div class="pt-2 border-t border-purple-200 dark:border-purple-800/40">
                <div class="text-[10px] font-semibold text-purple-700 dark:text-purple-300 uppercase mb-2">
                  Suggested Index Optimizations:
                </div>
                <div class="space-y-1.5">
                  {#each explainData.index_suggestions as hint}
                    <div class="bg-white dark:bg-neutral-900 border border-purple-100 dark:border-purple-900/60 p-2 rounded text-xs font-mono text-purple-600 dark:text-purple-400 flex items-center justify-between">
                      <span>{hint}</span>
                      <button
                        onclick={() => {
                          navigator.clipboard.writeText(hint);
                          showToast('Index statement copied.', 'success');
                        }}
                        class="text-[10px] text-neutral-400 hover:text-neutral-100 cursor-pointer uppercase font-sans"
                      >
                        Copy
                      </button>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {/if}

        <!-- Execution Results -->
        {#if result && result.columns.length > 0}
          <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-4">
            <div class="flex items-center justify-between mb-2">
              <div class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider">
                Execution Results ({result.rows_affected} rows • {result.duration_ms}ms)
              </div>
            </div>
            <div class="overflow-x-auto max-h-60 overflow-y-auto">
              <table class="w-full text-left text-xs border-collapse">
                <thead class="bg-neutral-50 dark:bg-neutral-800/60 text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800 sticky top-0">
                  <tr>
                    {#each result.columns as col}
                      <th class="py-2 px-3 font-semibold">{col}</th>
                    {/each}
                  </tr>
                </thead>
                <tbody class="divide-y divide-neutral-100 dark:divide-neutral-800/80">
                  {#each result.rows as row}
                    <tr>
                      {#each row as val}
                        <td class="py-2 px-3 font-mono text-neutral-700 dark:text-neutral-300">
                          {#if val === null}<span class="italic text-neutral-400">NULL</span>{:else}{String(val)}{/if}
                        </td>
                      {/each}
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
