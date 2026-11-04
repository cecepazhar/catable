import { describe, it, expect, beforeEach } from 'vitest';
import { BrowseQuery, OPERATORS, operatorInfo, type ColumnRef } from './browseQuery.svelte';

const columns: ColumnRef[] = [
  { name: 'id', data_type: 'integer' },
  { name: 'name', data_type: 'text' },
  { name: 'payload', data_type: 'jsonb' },
  { name: 'avatar', data_type: 'bytea' },
];

describe('BrowseQuery sorting', () => {
  let q: BrowseQuery;
  beforeEach(() => {
    q = new BrowseQuery();
  });

  it('cycles ascending → descending → off', () => {
    expect(q.sortState('name')).toBeNull();
    q.toggleSort('name');
    expect(q.sortState('name')).toBe('asc');
    q.toggleSort('name');
    expect(q.sortState('name')).toBe('desc');
    q.toggleSort('name');
    expect(q.sortState('name')).toBeNull();
  });

  it('a plain click replaces the sort, Shift+click adds a column', () => {
    q.toggleSort('name');
    q.toggleSort('id');
    expect(q.sort).toEqual([{ column: 'id', descending: false }]);

    q.toggleSort('name', true);
    expect(q.sort.map((s) => s.column)).toEqual(['id', 'name']);
    q.toggleSort('id', true); // id: asc -> desc, name stays
    expect(q.sort).toEqual([
      { column: 'name', descending: false },
      { column: 'id', descending: true },
    ]);
  });
});

describe('BrowseQuery request building', () => {
  let q: BrowseQuery;
  beforeEach(() => {
    q = new BrowseQuery();
  });

  it('sends nothing for an empty query', () => {
    expect(q.toRequestParts(columns)).toEqual({
      filters: [],
      filter_mode: 'and',
      order_by: [],
      search: null,
      search_columns: [],
    });
    expect(q.isFiltering).toBe(false);
  });

  it('ignores incomplete rows instead of sending an accidental "= \'\'"', () => {
    q.addFilter(''); // no column
    q.addFilter('name'); // no value
    expect(q.toRequestParts(columns).filters).toEqual([]);
    expect(q.isFiltering).toBe(false);
  });

  it('builds single, null, between and list operands', () => {
    q.addFilter('name');
    q.filters[0].op = 'contains';
    q.filters[0].value = 'ada';

    q.addFilter('id');
    q.filters[1].op = 'between';
    q.filters[1].value = '1';
    q.filters[1].value2 = '9';

    q.addFilter('id');
    q.filters[2].op = 'in';
    q.filters[2].value = ' 3, 5 ,, 7 ';

    q.addFilter('name');
    q.filters[3].op = 'is_null';

    expect(q.toRequestParts(columns).filters).toEqual([
      { column: 'name', op: 'contains', value: 'ada' },
      { column: 'id', op: 'between', values: ['1', '9'] },
      { column: 'id', op: 'in', values: ['3', '5', '7'] },
      { column: 'name', op: 'is_null' },
    ]);
    expect(q.isFiltering).toBe(true);
  });

  it('a half-filled BETWEEN or an empty IN list is not sent', () => {
    q.addFilter('id');
    q.filters[0].op = 'between';
    q.filters[0].value = '1';
    q.addFilter('id');
    q.filters[1].op = 'in';
    q.filters[1].value = ' , ';
    expect(q.toRequestParts(columns).filters).toEqual([]);
  });

  it('searches only columns that make sense as text', () => {
    q.search = '  ada ';
    const parts = q.toRequestParts(columns);
    expect(parts.search).toBe('ada');
    expect(parts.search_columns).toEqual(['id', 'name']); // not the jsonb / bytea columns
    expect(q.isFiltering).toBe(true);
  });

  it('caps the number of searched columns', () => {
    const wide = Array.from({ length: 40 }, (_, i) => ({ name: `c${i}`, data_type: 'text' }));
    q.search = 'x';
    expect(q.toRequestParts(wide).search_columns).toHaveLength(12);
  });

  it('carries the AND/OR mode and the sort order', () => {
    q.mode = 'or';
    q.toggleSort('name');
    q.toggleSort('id', true);
    q.toggleSort('id', true);
    const parts = q.toRequestParts(columns);
    expect(parts.filter_mode).toBe('or');
    expect(parts.order_by).toEqual([
      { column: 'name', descending: false },
      { column: 'id', descending: true },
    ]);
  });

  it('clearFilters keeps the sort, reset clears everything', () => {
    q.addFilter('name');
    q.filters[0].value = 'x';
    q.search = 'y';
    q.toggleSort('id');
    q.clearFilters();
    expect(q.filters).toEqual([]);
    expect(q.search).toBe('');
    expect(q.sort).toHaveLength(1);
    q.reset();
    expect(q.sort).toEqual([]);
  });

  it('every operator is described and known to the backend contract', () => {
    const backend = ['eq', 'ne', 'lt', 'lte', 'gt', 'gte', 'like', 'not_like', 'contains', 'starts_with', 'in', 'not_in', 'is_null', 'is_not_null', 'between'];
    expect(OPERATORS.map((o) => o.op).sort()).toEqual([...backend].sort());
    expect(operatorInfo('between').operands).toBe(2);
    expect(operatorInfo('is_null').operands).toBe(0);
  });
});
