<script lang="ts">
  // Sign in / create account / forgot password for CATable Pro. Used on the lock screen (before
  // the vault is open — the backend holds the session until unlock) and in Settings.
  // The Pro password is deliberately separate from the vault master password.
  import { t, getLocale } from '$lib/i18n/index.svelte';
  import {
    proLogin,
    proRegister,
    proResendVerification,
    proForgotPassword,
    proErrorCode,
    type ProAccount
  } from '$lib/api/pro';
  import { getPro } from '$lib/stores/pro.svelte';
  import { errorText } from '$lib/errors';

  let { onSignedIn }: { onSignedIn: (account: ProAccount) => void } = $props();

  const pro = getPro();

  const proFeatures = $derived([
    { id: 'cloud_sync', name: 'Cloud Sync', desc: 'Zero-Knowledge E2EE Sync', active: pro.isPro },
    { id: 'ssh_bastion', name: 'SSH Bastion', desc: '1-Click Bastion Tunnels', active: pro.isPro },
    { id: 'blast_shield', name: 'Blast-Radius Shield', desc: 'Dry-Run Simulation', active: pro.isPro },
    { id: 'erd_diff', name: 'ERD Diff', desc: 'Visual Schema Migrations', active: pro.isPro },
    { id: 'ai_copilot', name: 'AI DBA Copilot', desc: 'Context-Aware SQL Assistant', active: pro.isPro },
    { id: 'pii_masking', name: 'PII Masking', desc: 'Automatic Sensitive Data Redaction', active: pro.isPro }
  ]);

  type Mode = 'login' | 'register' | 'forgot' | 'checkEmail';
  let mode = $state<Mode>('login');
  let name = $state('');
  let email = $state('');
  let password = $state('');
  let confirm = $state('');
  let busy = $state(false);
  let error = $state('');
  let errorCode = $state<string | null>(null);
  let notice = $state('');

  const INPUT =
    'w-full px-3 py-2.5 bg-white dark:bg-neutral-900 border border-neutral-300 dark:border-neutral-800 rounded-lg text-sm text-neutral-900 dark:text-white placeholder-neutral-400 dark:placeholder-neutral-600 focus:outline-none focus:border-sky-500 transition-colors';
  const LABEL = 'block text-xs font-semibold uppercase tracking-wider text-neutral-500 dark:text-neutral-400 mb-1.5';
  const LINK = 'text-sky-600 dark:text-sky-400 hover:underline font-medium';

  function switchMode(next: Mode) {
    mode = next;
    error = '';
    errorCode = null;
    notice = '';
  }

  function fail(err: unknown) {
    errorCode = proErrorCode(err);
    const key = errorCode ? `pro.errors.${errorCode}` : '';
    const translated = key ? t(key) : '';
    error = translated && translated !== key ? translated : errorText(err);
  }

  async function submit(event: Event) {
    event.preventDefault();
    error = '';
    errorCode = null;
    notice = '';
    if (mode === 'register' && password !== confirm) {
      error = t('pro.errors.PASSWORD_MISMATCH');
      return;
    }
    busy = true;
    try {
      if (mode === 'login') {
        const account = await proLogin(email, password);
        password = '';
        onSignedIn(account);
      } else if (mode === 'register') {
        await proRegister(email, password, name, getLocale());
        password = '';
        confirm = '';
        switchMode('checkEmail');
      } else if (mode === 'forgot') {
        await proForgotPassword(email, getLocale());
        notice = t('pro.login.resetSent');
      }
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }

  async function resendVerification() {
    busy = true;
    try {
      await proResendVerification(email);
      notice = t('pro.login.verificationResent');
      error = '';
      errorCode = null;
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }

  async function sendSetPasswordLink() {
    busy = true;
    try {
      await proForgotPassword(email, getLocale());
      notice = t('pro.login.resetSent');
      error = '';
      errorCode = null;
    } catch (err) {
      fail(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="space-y-4">
  <div class="text-center space-y-1">
    <h3 class="text-base font-bold text-neutral-900 dark:text-white">
      {mode === 'register' ? t('pro.login.registerTitle') : mode === 'forgot' ? t('pro.login.forgotTitle') : mode === 'checkEmail' ? t('pro.login.checkEmailTitle') : t('pro.login.title')}
    </h3>
    <p class="text-xs text-neutral-500 dark:text-neutral-400">
      {mode === 'checkEmail' ? t('pro.login.checkEmailBody', { email }) : t('pro.login.separateNote')}
    </p>
  </div>

  {#if error}
    <div class="p-3 text-xs rounded-lg bg-rose-500/10 border border-rose-500/30 text-rose-700 dark:text-rose-400 space-y-1.5" role="alert">
      <p>{error}</p>
      {#if errorCode === 'EMAIL_NOT_VERIFIED'}
        <button type="button" class={LINK} disabled={busy} onclick={resendVerification}>{t('pro.login.resendVerification')}</button>
      {:else if errorCode === 'PASSWORD_NOT_SET'}
        <button type="button" class={LINK} disabled={busy} onclick={sendSetPasswordLink}>{t('pro.login.sendSetPassword')}</button>
      {/if}
    </div>
  {/if}
  {#if notice}
    <div class="p-3 text-xs rounded-lg bg-emerald-500/10 border border-emerald-500/30 text-emerald-700 dark:text-emerald-400" role="status">{notice}</div>
  {/if}

  {#if mode === 'checkEmail'}
    <div class="flex flex-col gap-2 text-xs text-center">
      <button type="button" class={LINK} disabled={busy} onclick={resendVerification}>{t('pro.login.resendVerification')}</button>
      <button type="button" class={LINK} onclick={() => switchMode('login')}>{t('pro.login.backToLogin')}</button>
    </div>
  {:else}
    <form onsubmit={submit} class="space-y-3">
      {#if mode === 'register'}
        <div>
          <label for="pro-name" class={LABEL}>{t('pro.login.name')}</label>
          <input id="pro-name" type="text" bind:value={name} maxlength="64" autocomplete="name" class={INPUT} />
        </div>
      {/if}
      <div>
        <label for="pro-email" class={LABEL}>{t('pro.login.email')}</label>
        <input id="pro-email" type="email" bind:value={email} required autocomplete="email" class={INPUT} />
      </div>
      {#if mode !== 'forgot'}
        <div>
          <label for="pro-password" class={LABEL}>{t('pro.login.password')}</label>
          <input id="pro-password" type="password" bind:value={password} required minlength="8" autocomplete={mode === 'register' ? 'new-password' : 'current-password'} class={INPUT} />
        </div>
      {/if}
      {#if mode === 'register'}
        <div>
          <label for="pro-confirm" class={LABEL}>{t('pro.login.confirmPassword')}</label>
          <input id="pro-confirm" type="password" bind:value={confirm} required minlength="8" autocomplete="new-password" class={INPUT} />
        </div>
      {/if}

      <button
        type="submit"
        disabled={busy}
        class="w-full py-2.5 bg-sky-600 hover:bg-sky-500 disabled:opacity-50 text-white font-medium rounded-lg shadow-sm transition-colors text-sm"
      >
        {#if busy}
          {t('pro.login.working')}
        {:else if mode === 'register'}
          {t('pro.login.createAccount')}
        {:else if mode === 'forgot'}
          {t('pro.login.sendResetLink')}
        {:else}
          {t('pro.login.signIn')}
        {/if}
      </button>
    </form>

    <div class="flex items-center justify-between text-xs">
      {#if mode === 'login'}
        <button type="button" class={LINK} onclick={() => switchMode('register')}>{t('pro.login.noAccount')}</button>
        <button type="button" class={LINK} onclick={() => switchMode('forgot')}>{t('pro.login.forgot')}</button>
      {:else}
        <button type="button" class={LINK} onclick={() => switchMode('login')}>{t('pro.login.backToLogin')}</button>
      {/if}
    </div>
  {/if}

  <!-- Dynamic Pro Tier Features Preview -->
  <div class="pt-3 border-t border-neutral-200 dark:border-neutral-800 space-y-2">
    <div class="flex items-center justify-between">
      <span class="text-xs font-semibold text-neutral-900 dark:text-neutral-100 flex items-center gap-1.5">
        <svg class="w-3.5 h-3.5 text-sky-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4M7.835 4.697a3.42 3.42 0 001.946-.806 3.42 3.42 0 014.438 0 3.42 3.42 0 001.946.806 3.42 3.42 0 013.138 3.138 3.42 3.42 0 00.806 1.946 3.42 3.42 0 010 4.438 3.42 3.42 0 00-.806 1.946 3.42 3.42 0 01-3.138 3.138 3.42 3.42 0 00-1.946.806 3.42 3.42 0 01-4.438 0 3.42 3.42 0 00-1.946-.806 3.42 3.42 0 01-3.138-3.138 3.42 3.42 0 00-.806-1.946 3.42 3.42 0 010-4.438 3.42 3.42 0 00.806-1.946 3.42 3.42 0 013.138-3.138z" />
        </svg>
        Pro Capabilities
      </span>
      <span class="text-[10px] font-bold px-1.5 py-0.5 rounded {pro.isPro ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/30' : 'bg-neutral-100 dark:bg-neutral-800 text-neutral-500 border border-neutral-200 dark:border-neutral-700'}">
        {pro.isPro ? 'UNLOCKED' : 'PRO TIER'}
      </span>
    </div>

    <div class="grid grid-cols-2 gap-1.5 pt-1">
      {#each proFeatures as feat (feat.id)}
        <div class="flex items-center gap-1.5 p-1.5 rounded-md border text-xs {feat.active ? 'bg-emerald-500/5 border-emerald-500/20 text-neutral-800 dark:text-neutral-200' : 'bg-neutral-50/50 dark:bg-neutral-900/40 border-neutral-200/60 dark:border-neutral-800/60 text-neutral-400 dark:text-neutral-500'}">
          {#if feat.active}
            <svg class="w-3.5 h-3.5 text-emerald-500 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2.2" d="M5 13l4 4L19 7" /></svg>
          {:else}
            <svg class="w-3.5 h-3.5 text-neutral-400 dark:text-neutral-600 shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.8" d="M12 15v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2zm10-10V7a4 4 0 00-8 0v4h8z" /></svg>
          {/if}
          <div class="min-w-0">
            <span class="block font-medium truncate text-[11px] {feat.active ? 'text-neutral-900 dark:text-white' : 'text-neutral-500 dark:text-neutral-400'}">{feat.name}</span>
          </div>
        </div>
      {/each}
    </div>
  </div>
</div>
