-- CATable integration-test seed — SQLite.
-- Keep in sync with postgres.sql and mysql.sql: same tables, same rows.
-- SQLite has no native uuid/timestamptz/json types: TEXT holds them
-- (timestamps as ISO-8601 UTC, JSON via json_object()).

PRAGMA foreign_keys = ON;

CREATE TABLE users (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    email      TEXT NOT NULL UNIQUE,
    role       TEXT NOT NULL DEFAULT 'member',
    created_at TEXT NOT NULL DEFAULT '2026-01-01T00:00:00Z'
);

INSERT INTO users (id, name, email, role, created_at) VALUES
    (1, 'Ada Lovelace',      'ada@example.test',      'admin',     '2026-01-01T08:00:00Z'),
    (2, 'Grace Hopper',      'grace@example.test',    'developer', '2026-01-02T09:30:00Z'),
    (3, 'Seán O''Brien',     'sean@example.test',     'developer', '2026-01-03T10:15:00Z'),
    (4, 'Dewi Ayu Lestari',  'dewi@example.test',     'member',    '2026-01-04T11:45:00Z'),
    (5, 'Margaret Hamilton', 'margaret@example.test', 'member',    '2026-01-05T12:00:00Z');

CREATE TABLE orders (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER        NOT NULL,
    total      NUMERIC(12,2)  NOT NULL,
    status     TEXT           NOT NULL DEFAULT 'pending',
    created_at TEXT           NOT NULL DEFAULT '2026-01-01T00:00:00Z',
    CONSTRAINT fk_orders_user FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
);
CREATE INDEX idx_orders_user_id ON orders (user_id);

INSERT INTO orders (id, user_id, total, status, created_at) VALUES
    (1, 1,  125.50, 'paid',      '2026-02-01T10:00:00Z'),
    (2, 1,   19.99, 'pending',   '2026-02-02T10:00:00Z'),
    (3, 2,  300.00, 'paid',      '2026-02-03T10:00:00Z'),
    (4, 3,   42.00, 'cancelled', '2026-02-04T10:00:00Z'),
    (5, 3,   88.10, 'paid',      '2026-02-05T10:00:00Z'),
    (6, 4, 1500.00, 'shipped',   '2026-02-06T10:00:00Z'),
    (7, 5,    7.25, 'pending',   '2026-02-07T10:00:00Z'),
    (8, 5,   64.00, 'paid',      '2026-02-08T10:00:00Z');

CREATE TABLE products (
    id    TEXT           NOT NULL PRIMARY KEY,
    sku   TEXT           NOT NULL UNIQUE,
    name  TEXT           NOT NULL,
    price NUMERIC(38,10) NOT NULL,
    stock INTEGER        NOT NULL DEFAULT 0
);

INSERT INTO products (id, sku, name, price, stock) VALUES
    ('0b9d6c8e-4f5a-4c1e-9a77-2f3c1d5e6a01', 'PAD-001', 'Padel Racket Pro',     '1899000.0000000000',                 12),
    ('5e2f1a3b-7c4d-4e8f-a1b2-c3d4e5f60702', 'BAL-003', 'Padel Balls (3 pack)', '89000.5000000000',                   240),
    ('9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c03', 'GRP-010', 'Overgrip "Tacky"',     '0.0000000001',                       0),
    ('c1d2e3f4-a5b6-4c7d-8e9f-0a1b2c3d4e04', 'BIG-999', 'Precision Ledger',     '1234567890123456789012345678.0123456789', 1);

CREATE TABLE events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL,
    payload     TEXT NOT NULL CHECK (json_valid(payload)),
    occurred_at TEXT NOT NULL,
    note        TEXT
);

WITH RECURSIVE seq(n) AS (
    SELECT 1
    UNION ALL
    SELECT n + 1 FROM seq WHERE n < 100000
)
INSERT INTO events (id, kind, payload, occurred_at, note)
SELECT n,
       CASE n % 4 WHEN 0 THEN 'login' WHEN 1 THEN 'click' WHEN 2 THEN 'purchase' ELSE 'logout' END,
       json_object('n', n, 'user_id', n % 5 + 1),
       strftime('%Y-%m-%dT%H:%M:%SZ', '2026-01-01 00:00:00', '+' || n || ' seconds'),
       CASE n % 3 WHEN 0 THEN NULL WHEN 1 THEN '' ELSE 'note ' || n END
FROM seq;
CREATE INDEX idx_events_occurred_at ON events (occurred_at);

CREATE TABLE "Mixed Case Table" (
    "Id"           INTEGER PRIMARY KEY,
    "Display Name" TEXT    NOT NULL,
    "isActive"     BOOLEAN NOT NULL DEFAULT TRUE
);

INSERT INTO "Mixed Case Table" ("Id", "Display Name", "isActive") VALUES
    (1, 'Alpha',          TRUE),
    (2, 'Beta "quoted"',  FALSE),
    (3, 'Gamma `tick`',   TRUE),
    (4, 'Δelta ünïcode',  FALSE);

CREATE TABLE audit_log (
    action    TEXT NOT NULL,
    actor     TEXT,
    logged_at TEXT NOT NULL
);

INSERT INTO audit_log (action, actor, logged_at) VALUES
    ('login',  'ada', '2026-03-01T07:00:00Z'),
    ('login',  'ada', '2026-03-01T07:00:00Z'),
    ('delete', NULL,  '2026-03-02T18:30:00Z');

ANALYZE;
