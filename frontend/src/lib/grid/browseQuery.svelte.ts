// What the user asked to see: filters, quick search and sort for the table browser.
//
// This is pure UI state. It is turned into a `BrowseRequest` and sent to the backend, which builds
// the SQL (the UI never concatenates SQL) — so sorting and filtering are server-side and work on
// the whole table, not only on the rows that happen to be loaded.

import type { BrowseFilter, BrowseOrderBy, FilterOp } from '$lib/stores/database.svelte';

/** `operands`: how many values the operator needs. `list` = comma separated. */
export const OPERATORS: ReadonlyArray<{ op: FilterOp; label: string; operands: 0 | 1 | 2 | 'list' }> = [
  { op: 'eq', label: 'is', operands: 1 },
  { op: 'ne', label: 'is not', operands: 1 },
  { op: 'contains', label: 'contains', operands: 1 },
  { op: 'starts_with', label: 'starts with', operands: 1 },
  { op: 'like', label: 'LIKE', operands: 1 },
  { op: 'not_like', label: 'NOT LIKE', operands: 1 },
  { op: 'gt', label: '>', operands: 1 },
  { op: 'gte', label: '≥', operands: 1 },
  { op: 'lt', label: '<', operands: 1 },
  { op: 'lte', label: '≤', operands: 1 },
  { op: 'between', label: 'between', operands: 2 },
  { op: 'in', label: 'in', operands: 'list' },
  { op: 'not_in', label: 'not in', operands: 'list' },
  { op: 'is_null', label: 'is NULL', operands: 0 },
  { op: 'is_not_null', label: 'is not NULL', operands: 0 },
];

export interface FilterRow {
  id: number;
  column: string;
  op: FilterOp;
  value: string;
  value2: string;
}

export interface ColumnRef {
  name: string;
  data_type: string;
}

export interface BrowseParts {
  filters: BrowseFilter[];
  filter_mode: 'and' | 'or';
  order_by: BrowseOrderBy[];
  search: string | null;
  search_columns: string[];
}

/** Columns that make no sense to search as text. */
const NOT_SEARCHABLE = /blob|bytea|binary|json|xml|geometry|geography|point|array|vector/i;
const MAX_SEARCH_COLUMNS = 12;

export function operatorInfo(op: FilterOp) {
  return OPERATORS.find((o) => o.op === op) ?? OPERATORS[0];
}

export class BrowseQuery {
  filters = $state<FilterRow[]>([]);
  mode = $state<'and' | 'or'>('and');
  search = $state('');
  sort = $state<BrowseOrderBy[]>([]);
  private nextId = 1;

  addFilter(column = '') {
    this.filters = [...this.filters, { id: this.nextId++, column, op: 'eq', value: '', value2: '' }];
  }

  removeFilter(id: number) {
    this.filters = this.filters.filter((f) => f.id !== id);
  }

  /** Clears filters and search; keeps the sort order. */
  clearFilters() {
    this.filters = [];
    this.search = '';
  }

  /** Back to a pristine state (used when another table is opened). */
  reset() {
    this.clearFilters();
    this.sort = [];
    this.mode = 'and';
  }

  sortState(column: string): 'asc' | 'desc' | null {
    const entry = this.sort.find((s) => s.column === column);
    if (!entry) return null;
    return entry.descending ? 'desc' : 'asc';
  }

  /** Click = asc → desc → off. `additive` (Shift+click) keeps the other sort columns. */
  toggleSort(column: string, additive = false) {
    const current = this.sortState(column);
    const others = additive ? this.sort.filter((s) => s.column !== column) : [];
    if (current === null) this.sort = [...others, { column, descending: false }];
    else if (current === 'asc') this.sort = [...others, { column, descending: true }];
    else this.sort = others;
  }

  /** Filters that are complete enough to send. Blank values are ignored, never sent as `= ''`. */
  private sendableFilters(): BrowseFilter[] {
    const out: BrowseFilter[] = [];
    for (const f of this.filters) {
      if (!f.column) continue;
      const { operands } = operatorInfo(f.op);
      if (operands === 0) {
        out.push({ column: f.column, op: f.op });
      } else if (operands === 1) {
        if (f.value !== '') out.push({ column: f.column, op: f.op, value: f.value });
      } else if (operands === 2) {
        if (f.value !== '' && f.value2 !== '') out.push({ column: f.column, op: f.op, values: [f.value, f.value2] });
      } else {
        const values = f.value
          .split(',')
          .map((v) => v.trim())
          .filter((v) => v !== '');
        if (values.length > 0) out.push({ column: f.column, op: f.op, values });
      }
    }
    return out;
  }

  /** True when filters or a search are narrowing the result. */
  get isFiltering(): boolean {
    return this.sendableFilters().length > 0 || this.search.trim() !== '';
  }

  toRequestParts(columns: ColumnRef[]): BrowseParts {
    const text = this.search.trim();
    const searchColumns = text
      ? columns
          .filter((c) => !NOT_SEARCHABLE.test(c.data_type))
          .map((c) => c.name)
          .slice(0, MAX_SEARCH_COLUMNS)
      : [];
    return {
      filters: this.sendableFilters(),
      filter_mode: this.mode,
      order_by: this.sort.map((s) => ({ column: s.column, descending: !!s.descending })),
      search: text || null,
      search_columns: searchColumns,
    };
  }
}
