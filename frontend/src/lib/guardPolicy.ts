/** When must the user explicitly confirm a query before it runs? (The backend still enforces `blocked`.) */

export type Risk = 'safe' | 'warning' | 'destructive' | 'blocked';
export type Environment = 'development' | 'staging' | 'production';

/**
 * - `destructive` (unbounded DELETE/UPDATE, DROP, TRUNCATE…) always asks, in every environment —
 *   it used to be skipped on Development, which is exactly where a stray `DELETE FROM users` happens.
 * - `warning` (a targeted UPDATE/DELETE, a schema change) asks outside Development so day-to-day
 *   work on a local database is not interrupted.
 */
export function needsConfirmation(risk: Risk, environment: Environment): boolean {
  if (risk === 'destructive') return true;
  if (risk === 'warning') return environment !== 'development';
  return false;
}
