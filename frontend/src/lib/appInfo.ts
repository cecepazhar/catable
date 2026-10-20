// Single source of truth for version string prints.
import { version } from '../../package.json';

export const APP_VERSION: string = version;
export const APP_NAME = 'CATable';
export const REPO_URL = 'https://github.com/cecepazhar/catable';
export const WEBSITE_URL = 'https://catable.fathforce.com';
export const AUTHOR_URL = 'https://cecepazhar.com';
export const PRICING_URL = `${WEBSITE_URL}/#pricing`;
export const GITHUB_SPONSORS_URL = 'https://github.com/sponsors/cecepazhar';
export const KOFI_URL = 'https://ko-fi.com/cecepazhar';
export const PAYPAL_URL = 'https://paypal.me/cecepazhar';

export function pricingUrl(): string {
  return `${WEBSITE_URL}/#pricing`;
}

export function releaseNotesUrl(ver: string): string {
  return `${REPO_URL}/releases/tag/v${ver.replace(/^v/, '')}`;
}
