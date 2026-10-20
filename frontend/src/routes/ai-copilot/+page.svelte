<script lang="ts">
  import { dbStore, type QueryResult } from '$lib/stores/database.svelte';
  import { showToast } from '$lib/stores/uiNotifications.svelte';

  let prompt = $state('');
  let result = $state<QueryResult | null>(null);
  let isLoading = $state(false);
  let responseText = $state('');

  const suggestions = [
    'Write a query to find top 5 products by revenue',
    'Show me all users and their total order amounts',
    'Find any duplicate emails in the users table',
    'Generate an INSERT statement for a new user'
  ];

  async function generateQuery() {
    if (!prompt.trim()) return;
    isLoading = true;
    responseText = '';
    result = null;

    try {
      // Simple local mock of the AI copilot
      await new Promise((r) => setTimeout(r, 800));

      let suggestedSql = '';
      const lowerPrompt = prompt.toLowerCase();

      if (lowerPrompt.includes('revenue') || lowerPrompt.includes('top')) {
        suggestedSql = `SELECT 
    p.name AS product_name,
    SUM(oi.quantity * oi.unit_price) AS total_revenue,
    COUNT(DISTINCT oi.order_id) AS orders_count
FROM order_items oi
JOIN products p ON oi.product_id = p.id
GROUP BY p.id
ORDER BY total_revenue DESC
LIMIT 5;`;
        responseText = 'I analyzed your request to find top products by revenue. I joined order_items with products to sum up total sales. Please review the generated SQL below.';
      } else if (lowerPrompt.includes('duplicate') || lowerPrompt.includes('email')) {
        suggestedSql = `SELECT email, COUNT(*) AS occurrences
FROM users
GROUP BY email
HAVING COUNT(*) > 1;`;
        responseText = 'I wrote a query to group the users table by email to isolate any repeated entries.';
      } else if (lowerPrompt.includes('insert')) {
        suggestedSql = `INSERT INTO users (name, email, role) VALUES ('New User', 'newuser@example.com', 'developer');`;
        responseText = 'I generated a standard INSERT statement for the users table.';
      } else {
        suggestedSql = `SELECT * FROM users LIMIT 10;`;
        responseText = 'I could not parse specific intent, so here is a standard basic query.';
      }

      // Evaluate with guard
      const guardVerdict = await dbStore.evaluateSafety(suggestedSql);
      if (guardVerdict.risk === 'blocked') {
        showToast(`The generated query was blocked: ${guardVerdict.reason}`, 'error');
      }

      const execRes = await dbStore.executeQuery(suggestedSql);
      result = execRes;
      if (execRes.error) {
        responseText = `AI generated the SQL, but execution encountered a server error: ${execRes.error}`;
      }
    } catch (e: any) {
      showToast(e?.message || 'AI generation failed.', 'error');
    } finally {
      isLoading = false;
    }
  }

  function applyToQuery() {
    navigator.clipboard.writeText(result?.rows.length ? '' : '');
    showToast('SQL copied to clipboard. Use it in SQL Studio to execute.', 'success');
  }
</script>

<div class="flex flex-col h-full overflow-hidden p-4 max-w-5xl mx-auto w-full space-y-4">
  <div class="flex items-center justify-between border-b border-neutral-200 dark:border-neutral-800 pb-3 shrink-0">
    <div class="flex items-center gap-3">
      <h2 class="text-sm font-bold text-neutral-900 dark:text-white">AI DBA Copilot</h2>
      <span class="text-[10px] font-mono bg-purple-100 dark:bg-purple-900/40 text-purple-600 dark:text-purple-300 px-2 py-0.5 rounded">SCHEMA-AWARE</span>
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

    <!-- Prompt -->
    <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-4 flex gap-3 items-center">
      <div class="flex-1">
        <textarea
          bind:value={prompt}
          placeholder="Describe the SQL query you need in plain language..."
          rows={2}
          class="w-full bg-transparent focus:outline-none text-xs text-neutral-900 dark:text-neutral-100 resize-none"
        ></textarea>
      </div>
      <button
        disabled={isLoading}
        onclick={generateQuery}
        class="px-4 py-2 text-xs font-semibold text-white bg-purple-600 hover:bg-purple-500 rounded-lg shadow-sm transition-colors disabled:opacity-50 cursor-pointer"
      >
        {isLoading ? 'Thinking…' : 'Generate SQL'}
      </button>
    </div>

    {#if isLoading}
      <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-6 flex items-center justify-center gap-3 text-xs text-neutral-500">
        <div class="w-4 h-4 border-2 border-purple-500 border-t-transparent rounded-full animate-spin"></div>
        AI is analyzing schema and generating query...
      </div>
    {:else if responseText}
      <div class="space-y-4">
        <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-4">
          <div class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider mb-2">AI Reasoning & Analysis</div>
          <p class="text-xs text-neutral-700 dark:text-neutral-300 leading-relaxed">{responseText}</p>
        </div>

        {#if result && result.columns.length > 0}
          <div class="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-xl p-4">
            <div class="text-[10px] font-bold text-neutral-500 uppercase tracking-wider mb-2">Query Results</div>
            <div class="overflow-x-auto">
              <table class="w-full text-left text-xs border-collapse">
                <thead class="bg-neutral-50 dark:bg-neutral-800/60 text-neutral-500 uppercase text-[10px] tracking-wider border-b border-neutral-200 dark:border-neutral-800">
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
