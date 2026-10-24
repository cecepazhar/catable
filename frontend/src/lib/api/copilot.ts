import { invoke } from '@tauri-apps/api/core';
import type { ConnectionConfig } from '$lib/stores/database.svelte';

export interface AiBackendConfig {
  base_url: string;
  api_key?: string | null;
  model: string;
  max_tokens: number;
  temperature: number;
}

export interface AiChatResponse {
  reply: string;
  sql?: string | null;
}

export interface AiExplainResponse {
  explanation: string;
  index_suggestions: string[];
}

export async function aiChat(
  prompt: string,
  connection?: ConnectionConfig | null,
  backend?: AiBackendConfig | null
): Promise<AiChatResponse> {
  return invoke<AiChatResponse>('ai_chat', {
    prompt,
    connection: connection ?? null,
    backend: backend ?? null,
  });
}

export async function aiExplain(
  sql: string,
  connection?: ConnectionConfig | null,
  backend?: AiBackendConfig | null
): Promise<AiExplainResponse> {
  return invoke<AiExplainResponse>('ai_explain', {
    sql,
    connection: connection ?? null,
    backend: backend ?? null,
  });
}
