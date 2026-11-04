// Staged row edits for the data grid.
//
// The grid never writes to the database directly. Edits, new rows and deletions accumulate here
// (shown as coloured cells), and only "Commit" turns them into a `RowChangeSet` that the backend
// previews as real SQL and applies in one transaction.

export type CellValue = { kind: 'null' } | { kind: 'text'; value: string } | { kind: 'default' };

export interface KeyPart {
  column: string;
  value: CellValue;
}

export interface ChangeSet {
  table: string;
  schema?: string | null;
  updates: Array<{
    key: KeyPart[];
    changes: Array<{ column: string; value: CellValue; original?: CellValue }>;
  }>;
  inserts: Array<{ values: Array<{ column: string; value: CellValue }> }>;
  deletes: Array<{ key: KeyPart[] }>;
}

/**
 * Column types whose displayed text is a faithful way to find the row again. For other types
 * (timestamps, decimals, json…) the value the user saw may be formatted differently from what the
 * server compares against, so a "stale edit" check on them would raise false conflicts.
 */
const ORIGINAL_CHECK_TYPES = /char|text|string|int|serial|bool|uuid|enum/i;

export const NULL_CELL: CellValue = { kind: 'null' };

export function toCell(value: unknown): CellValue {
  if (value === null || value === undefined) return NULL_CELL;
  if (typeof value === 'object') return { kind: 'text', value: JSON.stringify(value) };
  return { kind: 'text', value: String(value) };
}

export function sameCell(a: CellValue, b: CellValue): boolean {
  if (a.kind !== b.kind) return false;
  return a.kind === 'text' && b.kind === 'text' ? a.value === b.value : true;
}

export function cellText(cell: CellValue): string {
  return cell.kind === 'text' ? cell.value : '';
}

export class EditStaging {
  table = $state('');
  schema = $state<string | null>(null);
  columns = $state<string[]>([]);
  keyColumns = $state<string[]>([]);
  columnTypes = $state<Record<string, string>>({});
  /** The rows as loaded from the database (never mutated by edits). */
  rows = $state<unknown[][]>([]);

  edits = $state<Record<number, Record<string, CellValue>>>({});
  deleted = $state<Record<number, true>>({});
  inserts = $state<Array<Record<string, CellValue>>>([]);

  /** Replaces the loaded page and drops anything staged (callers confirm first). */
  load(args: {
    table: string;
    schema?: string | null;
    columns: string[];
    rows: unknown[][];
    keyColumns: string[];
    columnTypes?: Record<string, string>;
  }) {
    this.table = args.table;
    this.schema = args.schema ?? null;
    this.columns = args.columns;
    this.rows = args.rows;
    this.keyColumns = args.keyColumns;
    this.columnTypes = args.columnTypes ?? {};
    this.discard();
  }

  discard() {
    this.edits = {};
    this.deleted = {};
    this.inserts = [];
  }

  /** Number of rows with a pending change (edited, deleted or new). */
  get pendingCount(): number {
    const editedRows = Object.keys(this.edits).filter((r) => !this.deleted[Number(r)]).length;
    return editedRows + Object.keys(this.deleted).length + this.inserts.length;
  }

  get hasPending(): boolean {
    return this.pendingCount > 0;
  }

  originalCell(row: number, column: string): CellValue {
    const index = this.columns.indexOf(column);
    return index < 0 ? NULL_CELL : toCell(this.rows[row]?.[index]);
  }

  cell(row: number, column: string): CellValue {
    return this.edits[row]?.[column] ?? this.originalCell(row, column);
  }

  isDirty(row: number, column: string): boolean {
    return this.edits[row]?.[column] !== undefined;
  }

  isDeleted(row: number): boolean {
    return this.deleted[row] === true;
  }

  /** Stages a new value; setting the original value back un-stages it. */
  setCell(row: number, column: string, value: CellValue) {
    if (sameCell(value, this.originalCell(row, column))) {
      this.revertCell(row, column);
      return;
    }
    this.edits = { ...this.edits, [row]: { ...this.edits[row], [column]: value } };
  }

  revertCell(row: number, column: string) {
    const rowEdits = this.edits[row];
    if (!rowEdits || rowEdits[column] === undefined) return;
    const { [column]: _removed, ...rest } = rowEdits;
    const next = { ...this.edits };
    if (Object.keys(rest).length === 0) delete next[row];
    else next[row] = rest;
    this.edits = next;
  }

  toggleDelete(row: number) {
    const next = { ...this.deleted };
    if (next[row]) delete next[row];
    else next[row] = true;
    this.deleted = next;
  }

  addRow(): number {
    this.inserts = [...this.inserts, {}];
    return this.inserts.length - 1;
  }

  setInsertCell(index: number, column: string, value: CellValue) {
    this.inserts = this.inserts.map((row, i) => (i === index ? { ...row, [column]: value } : row));
  }

  removeInsert(index: number) {
    this.inserts = this.inserts.filter((_, i) => i !== index);
  }

  /** Why editing is unavailable, or null when it is allowed. */
  get readOnlyReason(): string | null {
    if (this.keyColumns.length === 0) {
      return 'This table has no primary key, so a row cannot be identified safely.';
    }
    return null;
  }

  private keyOf(row: number): KeyPart[] {
    // Always the ORIGINAL key values, even if the user also edited a key column.
    return this.keyColumns.map((column) => ({ column, value: this.originalCell(row, column) }));
  }

  /** The change set the backend previews and applies. */
  toChangeSet(): ChangeSet {
    const updates: ChangeSet['updates'] = [];
    for (const [rowKey, rowEdits] of Object.entries(this.edits)) {
      const row = Number(rowKey);
      if (this.deleted[row]) continue;
      const changes = Object.entries(rowEdits).map(([column, value]) => {
        const change: { column: string; value: CellValue; original?: CellValue } = { column, value };
        if (ORIGINAL_CHECK_TYPES.test(this.columnTypes[column] ?? '')) {
          change.original = this.originalCell(row, column);
        }
        return change;
      });
      updates.push({ key: this.keyOf(row), changes });
    }

    const inserts = this.inserts.map((row) => ({
      values: Object.entries(row).map(([column, value]) => ({ column, value })),
    }));

    const deletes = Object.keys(this.deleted).map((r) => ({ key: this.keyOf(Number(r)) }));

    return {
      table: this.table,
      schema: this.schema,
      updates,
      inserts,
      deletes,
    };
  }
}
