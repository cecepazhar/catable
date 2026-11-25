// Drives the boot splash defined inline in app.html (same pattern as CAPost).
// The splash stays up while the app warms up: at least MIN_MS so it never just flickers past,
// never more than MAX_MS (app.html also enforces a hard cap of its own, so a stuck promise can
// never keep the user waiting).

const MIN_MS = 1300;
const MAX_MS = 3200;

type BootApi = { start: number; step: (text: string, pct?: number) => void; done: () => void };

const api = (): BootApi | undefined => (typeof window !== 'undefined' ? (window as any).__catableBoot : undefined);

// rAF never fires while the window is hidden/occluded, so cap the wait.
const frame = () =>
  Promise.race([
    new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))),
    new Promise<void>((r) => setTimeout(r, 120)),
  ]);
const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
const settle = (p: Promise<unknown>) => p.then(() => undefined, () => undefined);

let finished = false;

/**
 * Runs the start-up work behind the splash, reporting each step, then fades it out.
 * `loadConnections` is the app's real first job; the rest warms things the first clicks need.
 * Step texts are English; app.html shows them in Indonesian when that is the language.
 */
export async function runBoot(loadConnections: () => Promise<unknown>): Promise<void> {
  const boot = api();
  if (!boot || finished) {
    await settle(loadConnections());
    return;
  }
  finished = true;

  const work = (async () => {
    boot.step('Opening vault…', 30);
    await settle(loadConnections());

    boot.step('Preparing workspace…', 60);
    // The table grid is the first screen people open after connecting: load its code now.
    await settle(
      Promise.all([
        import('$lib/grid/EditableGrid.svelte'),
        import('$lib/grid/FilterBar.svelte'),
        import('$lib/components/RowDetailsInspector.svelte'),
      ])
    );

    boot.step('Loading fonts…', 80);
    if (document.fonts?.ready) await Promise.race([settle(document.fonts.ready), sleep(400)]);

    boot.step('Ready', 95);
    await frame(); // let the real UI paint underneath before the splash fades
  })();

  await Promise.race([work, sleep(MAX_MS)]);
  const elapsed = performance.now() - boot.start;
  if (elapsed < MIN_MS) await sleep(MIN_MS - elapsed);
  boot.done();
}
