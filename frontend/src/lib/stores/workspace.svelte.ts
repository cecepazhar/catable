// TablePlus-style workspace: open table tabs, back/forward history and panel toggles.
//
// The table browser page reacts to `active`; the sidebar, the command palette and shortcuts only
// call `open()` / `close()` here, so there is one place that decides which table is showing.

class WorkspaceStore {
  /** Open table tabs, in strip order. */
  tabs = $state<string[]>([]);
  /** The table currently shown, or null. */
  active = $state<string | null>(null);
  /** Right-hand row detail panel. */
  inspectorOpen = $state(false);
  /** Filter rows above the grid. */
  filtersOpen = $state(false);
  /** Columns hidden in the grid, per table. */
  hiddenColumns = $state<Record<string, string[]>>({});
  /** Bumped to ask the page to reload the active table (Ctrl+R / refresh button). */
  refreshTick = $state(0);

  private back: string[] = [];
  private forward: string[] = [];
  canBack = $state(false);
  canForward = $state(false);

  private syncNav() {
    this.canBack = this.back.length > 0;
    this.canForward = this.forward.length > 0;
  }

  /** Opens (or focuses) a table tab. */
  open(table: string, { record = true } = {}) {
    if (!this.tabs.includes(table)) this.tabs = [...this.tabs, table];
    if (this.active === table) return;
    if (record && this.active) {
      this.back.push(this.active);
      this.forward = [];
    }
    this.active = table;
    this.syncNav();
  }

  close(table: string) {
    const i = this.tabs.indexOf(table);
    if (i < 0) return;
    this.tabs = this.tabs.filter((t) => t !== table);
    this.back = this.back.filter((t) => t !== table);
    this.forward = this.forward.filter((t) => t !== table);
    if (this.active === table) {
      this.active = this.tabs[Math.min(i, this.tabs.length - 1)] ?? null;
    }
    this.syncNav();
  }

  closeOthers(table: string) {
    this.tabs = this.tabs.filter((t) => t === table);
    this.back = [];
    this.forward = [];
    this.active = table;
    this.syncNav();
  }

  cycle(step: 1 | -1) {
    if (this.tabs.length < 2 || !this.active) return;
    const i = this.tabs.indexOf(this.active);
    const next = this.tabs[(i + step + this.tabs.length) % this.tabs.length];
    this.open(next);
  }

  goBack() {
    const prev = this.back.pop();
    if (!prev) return;
    if (this.active) this.forward.push(this.active);
    if (!this.tabs.includes(prev)) this.tabs = [...this.tabs, prev];
    this.active = prev;
    this.syncNav();
  }

  goForward() {
    const next = this.forward.pop();
    if (!next) return;
    if (this.active) this.back.push(this.active);
    if (!this.tabs.includes(next)) this.tabs = [...this.tabs, next];
    this.active = next;
    this.syncNav();
  }

  /** Puts the previous table back when the user refused to leave the current one. */
  revertTo(table: string | null) {
    if (table && !this.tabs.includes(table)) this.tabs = [...this.tabs, table];
    this.active = table;
  }

  hidden(table: string | null): ReadonlySet<string> {
    return new Set(table ? (this.hiddenColumns[table] ?? []) : []);
  }

  setHidden(table: string, columns: string[]) {
    this.hiddenColumns = { ...this.hiddenColumns, [table]: columns };
  }

  refresh() {
    this.refreshTick++;
  }

  /** Saved tab sets of the other open connections. */
  private parked = new Map<
    string,
    { tabs: string[]; active: string | null; back: string[]; forward: string[]; hidden: Record<string, string[]> }
  >();

  /** Saves this connection's tabs while another connection is shown. */
  park(connectionId: string) {
    this.parked.set(connectionId, {
      tabs: this.tabs,
      active: this.active,
      back: [...this.back],
      forward: [...this.forward],
      hidden: this.hiddenColumns,
    });
  }

  /** Brings back a connection's tabs (or starts empty). */
  unpark(connectionId: string) {
    const s = this.parked.get(connectionId);
    this.parked.delete(connectionId);
    this.tabs = s?.tabs ?? [];
    this.active = s?.active ?? null;
    this.back = s?.back ?? [];
    this.forward = s?.forward ?? [];
    this.hiddenColumns = s?.hidden ?? {};
    this.syncNav();
  }

  forget(connectionId: string) {
    this.parked.delete(connectionId);
  }

  /** Called on connect / disconnect: tabs belong to one connection. */
  reset() {
    this.tabs = [];
    this.active = null;
    this.back = [];
    this.forward = [];
    this.hiddenColumns = {};
    this.syncNav();
  }
}

export const workspace = new WorkspaceStore();
