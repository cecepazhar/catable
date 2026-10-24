// CATable Pro state for the UI: who is signed in, what this device is entitled to, and whether
// the Pro server is reachable. Entitlement comes from the signed token checked offline by the
// backend, so being offline never downgrades a paying user (within the 14-day grace).
import {
  proStatus,
  proServerAvailable,
  proCommitPending,
  proSync,
  proErrorCode,
  proSyncVault,
  type ProStatus,
  type SyncOutcome
} from '$lib/api/pro';

/** How often an unlocked app re-syncs its licence (the server re-issues a 14-day token). */
const SYNC_INTERVAL_MS = 6 * 60 * 1000;

/** How often the Zero-Knowledge E2EE vault re-syncs in the background (30 minutes). */
const VAULT_SYNC_INTERVAL_MS = 30 * 60 * 1000;

let status = $state<ProStatus | null>(null);
let serverAvailable = $state<boolean | null>(null);
let syncing = $state(false);
let syncTimer: ReturnType<typeof setInterval> | null = null;
let vaultSyncTimer: ReturnType<typeof setInterval> | null = null;
let vaultSyncing = $state(false);
let lastVaultSyncTs = $state<number | null>(null);

export function getPro() {
  return {
    get status() {
      return status;
    },
    /** This account's own Pro plan or trial is valid on this device. */
    get isPro() {
      const ent = status?.entitlement;
      const rawIsPro = status?.is_pro ?? status?.isPro;
      return (
        (ent?.state === 'valid' && ent.features.includes('cloud_sync')) ||
        rawIsPro === true
      );
    },
    /** Zero-Knowledge E2EE cloud sync is connected (pro && last sync succeeded). */
    get isSyncConnected() {
      const raw = status?.sync_connected ?? status?.syncConnected;
      const vaultOk = lastVaultSyncTs !== null && !vaultSyncing;
      return getPro().isPro && (raw === true || vaultOk);
    },
    /** Last Zero-Knowledge E2EE vault sync timestamp (seconds). */
    get lastVaultSyncTs() {
      return lastVaultSyncTs;
    },
    /** Whether a vault sync is currently in progress. */
    get vaultSyncing() {
      return vaultSyncing;
    },
    /** SSH Bastion 1-Click tunnel builder is available. */
    get isBastionAvailable() {
      const raw = status?.ssh_bastion_available ?? status?.sshBastionAvailable;
      return getPro().isPro && raw === true;
    },
    /** Blast-Radius Shield is enabled on this device. */
    get blastRadiusShieldAvailable() {
      const raw =
        status?.blast_radius_shield_available ?? status?.blastRadiusShieldAvailable;
      return getPro().isPro && raw === true;
    },
    /** PII Masking is a Pro entitlement. */
    get isPiiMaskingEnabled() {
      const ent = status?.entitlement;
      return getPro().isPro && (ent?.state !== 'valid' || ent.features.includes('pii_masking'));
    },
    /** Access through someone else's team only (free account in an entitled team). */
    get isTeamMember() {
      const ent = status?.entitlement;
      return ent?.state === 'valid' && ent.tier === 'team';
    },
    /** null while unknown (not checked yet / browser preview). */
    get serverAvailable() {
      return serverAvailable;
    },
    get syncing() {
      return syncing;
    }
  };
}

export async function refreshProStatus(): Promise<void> {
  try {
    status = await proStatus();
  } catch {
    // Browser preview or vault locked mid-call: keep what we had.
  }
}

export async function checkProServer(): Promise<boolean> {
  try {
    serverAvailable = await proServerAvailable();
  } catch {
    serverAvailable = false;
  }
  return serverAvailable;
}

/** Heartbeat. Throws on failure so callers can show why; the status is refreshed either way. */
export async function syncPro(): Promise<SyncOutcome | null> {
  if (syncing) return null;
  syncing = true;
  try {
    const outcome = await proSync();
    status = outcome.status;
    return outcome;
  } catch (err) {
    if (proErrorCode(err) === 'SESSION_EXPIRED') await refreshProStatus();
    throw err;
  } finally {
    syncing = false;
  }
}

/** Zero-Knowledge E2EE vault sync (returns last-sync timestamp in seconds). */
export async function syncVaultNow(): Promise<number | null> {
  if (vaultSyncing) return lastVaultSyncTs;
  vaultSyncing = true;
  try {
    const ts = await proSyncVault();
    lastVaultSyncTs = ts;
    await refreshProStatus();
    return ts;
  } catch {
    return lastVaultSyncTs;
  } finally {
    vaultSyncing = false;
  }
}

export function setProStatus(next: ProStatus): void {
  status = next;
}

/**
 * Called once the vault is open: commits any pending sign-in, then syncs the Pro license
 * and the Zero-Knowledge E2EE vault in the background.
 */
export async function onVaultUnlocked(): Promise<void> {
  try {
    await proCommitPending();
  } catch {
    // Nothing pending, or no backend (browser preview).
  }
  await refreshProStatus();
  void checkProServer();
  if (status?.signedIn) void syncPro().catch(() => {});
  if (getPro().isPro) void syncVaultNow().catch(() => {});

  if (syncTimer) clearInterval(syncTimer);
  syncTimer = setInterval(() => {
    if (status?.signedIn) void syncPro().catch(() => {});
  }, SYNC_INTERVAL_MS);

  if (vaultSyncTimer) clearInterval(vaultSyncTimer);
  vaultSyncTimer = setInterval(() => {
    if (getPro().isPro) void syncVaultNow().catch(() => {});
  }, VAULT_SYNC_INTERVAL_MS);
}

/** Vault locked or app signed out: forget in-memory Pro state until the next unlock. */
export function onVaultLocked(): void {
  if (syncTimer) clearInterval(syncTimer);
  syncTimer = null;
  if (vaultSyncTimer) clearInterval(vaultSyncTimer);
  vaultSyncTimer = null;
  status = null;
  lastVaultSyncTs = null;
}
