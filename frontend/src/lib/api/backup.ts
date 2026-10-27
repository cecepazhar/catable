import { invoke } from '@tauri-apps/api/core';

export async function exportEncryptedBackup(password: string): Promise<number[]> {
  return invoke<number[]>('export_encrypted_backup', { passphrase: password });
}

export async function importEncryptedBackup(data: number[], password: string): Promise<number> {
  return invoke<number>('import_encrypted_backup', { data, passphrase: password });
}

export async function exportJsonBackup(): Promise<string> {
  return invoke<string>('export_json_backup');
}

export async function importJsonBackup(jsonStr: string): Promise<number> {
  return invoke<number>('import_json_backup', { jsonStr });
}
