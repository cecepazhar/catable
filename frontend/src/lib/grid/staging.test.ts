import { describe, it, expect, beforeEach } from 'vitest';
import { EditStaging, NULL_CELL, toCell, type CellValue } from './staging.svelte';

const text = (value: string): CellValue => ({ kind: 'text', value });

function loaded(): EditStaging {
  const s = new EditStaging();
  s.load({
    table: 'users',
    columns: ['id', 'name', 'note', 'created_at'],
    rows: [
      [1, 'Ada', null, '2026-01-01T08:00:00Z'],
      [2, 'Grace', '', '2026-01-02T09:30:00Z'],
      [3, 'Sean', 'x', '2026-01-03T10:15:00Z'],
    ],
    keyColumns: ['id'],
    columnTypes: { id: 'integer', name: 'text', note: 'text', created_at: 'timestamp with time zone' },
  });
  return s;
}

describe('EditStaging', () => {
  let s: EditStaging;
  beforeEach(() => {
    s = loaded();
  });

  it('stages an edit as dirty without touching the loaded rows', () => {
    s.setCell(0, 'name', text('Ada L.'));
    expect(s.isDirty(0, 'name')).toBe(true);
    expect(s.cell(0, 'name')).toEqual(text('Ada L.'));
    expect(s.originalCell(0, 'name')).toEqual(text('Ada'));
    expect(s.rows[0][1]).toBe('Ada');
    expect(s.pendingCount).toBe(1);
  });

  it('typing the original value back un-stages the cell', () => {
    s.setCell(0, 'name', text('X'));
    s.setCell(0, 'name', text('Ada'));
    expect(s.isDirty(0, 'name')).toBe(false);
    expect(s.hasPending).toBe(false);
  });

  it('keeps NULL and empty string distinct', () => {
    expect(s.cell(0, 'note')).toEqual(NULL_CELL);
    expect(s.cell(1, 'note')).toEqual(text(''));
    // NULL -> '' is a real change, and so is '' -> NULL.
    s.setCell(0, 'note', text(''));
    s.setCell(1, 'note', NULL_CELL);
    expect(s.isDirty(0, 'note')).toBe(true);
    expect(s.isDirty(1, 'note')).toBe(true);
    const cs = s.toChangeSet();
    const byKey = (id: string) => cs.updates.find((u) => u.key[0].value.kind === 'text' && u.key[0].value.value === id)!;
    expect(byKey('1').changes[0].value).toEqual(text(''));
    expect(byKey('2').changes[0].value).toEqual(NULL_CELL);
  });

  it('builds updates keyed by the ORIGINAL primary key, even when the key itself is edited', () => {
    s.setCell(0, 'id', text('100'));
    s.setCell(0, 'name', text('Ada L.'));
    const cs = s.toChangeSet();
    expect(cs.updates).toHaveLength(1);
    expect(cs.updates[0].key).toEqual([{ column: 'id', value: text('1') }]);
    expect(cs.updates[0].changes.map((c) => c.column).sort()).toEqual(['id', 'name']);
  });

  it('sends the original value for checkable types, not for timestamps', () => {
    s.setCell(0, 'name', text('A'));
    s.setCell(0, 'created_at', text('2030-01-01'));
    const changes = s.toChangeSet().updates[0].changes;
    const name = changes.find((c) => c.column === 'name')!;
    const created = changes.find((c) => c.column === 'created_at')!;
    expect(name.original).toEqual(text('Ada'));
    expect(created.original).toBeUndefined();
  });

  it('marks rows for deletion and ignores edits on a deleted row', () => {
    s.setCell(2, 'name', text('changed'));
    s.toggleDelete(2);
    expect(s.isDeleted(2)).toBe(true);
    const cs = s.toChangeSet();
    expect(cs.updates).toHaveLength(0);
    expect(cs.deletes).toEqual([{ key: [{ column: 'id', value: text('3') }] }]);
    expect(s.pendingCount).toBe(1);

    s.toggleDelete(2);
    expect(s.isDeleted(2)).toBe(false);
  });

  it('stages new rows and lets them be removed', () => {
    const i = s.addRow();
    s.setInsertCell(i, 'name', text('New'));
    s.setInsertCell(i, 'note', NULL_CELL);
    expect(s.pendingCount).toBe(1);
    expect(s.toChangeSet().inserts[0].values).toEqual([
      { column: 'name', value: text('New') },
      { column: 'note', value: NULL_CELL },
    ]);
    s.removeInsert(i);
    expect(s.hasPending).toBe(false);
  });

  it('counts each affected row once and discard clears everything', () => {
    s.setCell(0, 'name', text('A'));
    s.setCell(0, 'note', text('B'));
    s.setCell(1, 'name', text('C'));
    s.toggleDelete(2);
    s.addRow();
    expect(s.pendingCount).toBe(4);
    s.discard();
    expect(s.pendingCount).toBe(0);
    expect(s.toChangeSet()).toEqual({ table: 'users', schema: null, updates: [], inserts: [], deletes: [] });
  });

  it('is read-only when the table has no primary key', () => {
    const t = new EditStaging();
    t.load({ table: 'audit_log', columns: ['a'], rows: [[1]], keyColumns: [] });
    expect(t.readOnlyReason).toMatch(/no primary key/i);
    expect(s.readOnlyReason).toBeNull();
  });

  it('loading a new page discards staged changes', () => {
    s.setCell(0, 'name', text('A'));
    s.load({ table: 'users', columns: ['id'], rows: [[9]], keyColumns: ['id'] });
    expect(s.hasPending).toBe(false);
  });

  it('serialises to the exact shape the Rust serde types expect', () => {
    s.setCell(0, 'note', NULL_CELL);
    s.setCell(0, 'name', text('Z'));
    const json = JSON.parse(JSON.stringify(s.toChangeSet()));
    expect(json.updates[0].key[0]).toEqual({ column: 'id', value: { kind: 'text', value: '1' } });
    expect(json.updates[0].changes.find((c: { column: string }) => c.column === 'name')).toEqual({
      column: 'name',
      value: { kind: 'text', value: 'Z' },
      original: { kind: 'text', value: 'Ada' },
    });
  });

  it('toCell flattens JSON and numbers to text', () => {
    expect(toCell(5)).toEqual(text('5'));
    expect(toCell(true)).toEqual(text('true'));
    expect(toCell({ a: 1 })).toEqual(text('{"a":1}'));
    expect(toCell(undefined)).toEqual(NULL_CELL);
  });
});
