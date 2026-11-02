-- CATable integration-test seed — MySQL 8.4 and MariaDB 11.
-- Keep in sync with postgres.sql and sqlite.sql: same tables, same rows.
-- MySQL has no timestamptz; TIMESTAMP (stored as UTC) is the closest type.

SET NAMES utf8mb4;
SET time_zone = '+00:00';

CREATE TABLE users (
    id         INT          NOT NULL AUTO_INCREMENT PRIMARY KEY,
    name       VARCHAR(255) NOT NULL,
    email      VARCHAR(255) NOT NULL UNIQUE,
    role       VARCHAR(32)  NOT NULL DEFAULT 'member',
    created_at TIMESTAMP    NOT NULL DEFAULT '2026-01-01 00:00:00'
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4;

INSERT INTO users (id, name, email, role, created_at) VALUES
    (1, 'Ada Lovelace',      'ada@example.test',      'admin',     '2026-01-01 08:00:00'),
    (2, 'Grace Hopper',      'grace@example.test',    'developer', '2026-01-02 09:30:00'),
    (3, 'Seán O''Brien',     'sean@example.test',     'developer', '2026-01-03 10:15:00'),
    (4, 'Dewi Ayu Lestari',  'dewi@example.test',     'member',    '2026-01-04 11:45:00'),
    (5, 'Margaret Hamilton', 'margaret@example.test', 'member',    '2026-01-05 12:00:00');

CREATE TABLE orders (
    id         INT           NOT NULL AUTO_INCREMENT PRIMARY KEY,
    user_id    INT           NOT NULL,
    total      DECIMAL(12,2) NOT NULL,
    status     VARCHAR(32)   NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP     NOT NULL DEFAULT '2026-01-01 00:00:00',
    INDEX idx_orders_user_id (user_id),
    CONSTRAINT fk_orders_user FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4;

INSERT INTO orders (id, user_id, total, status, created_at) VALUES
    (1, 1,  125.50, 'paid',      '2026-02-01 10:00:00'),
    (2, 1,   19.99, 'pending',   '2026-02-02 10:00:00'),
    (3, 2,  300.00, 'paid',      '2026-02-03 10:00:00'),
    (4, 3,   42.00, 'cancelled', '2026-02-04 10:00:00'),
    (5, 3,   88.10, 'paid',      '2026-02-05 10:00:00'),
    (6, 4, 1500.00, 'shipped',   '2026-02-06 10:00:00'),
    (7, 5,    7.25, 'pending',   '2026-02-07 10:00:00'),
    (8, 5,   64.00, 'paid',      '2026-02-08 10:00:00');

CREATE TABLE products (
    id    CHAR(36)       NOT NULL PRIMARY KEY,
    sku   VARCHAR(32)    NOT NULL UNIQUE,
    name  VARCHAR(255)   NOT NULL,
    price DECIMAL(38,10) NOT NULL,
    stock INT            NOT NULL DEFAULT 0
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4;

INSERT INTO products (id, sku, name, price, stock) VALUES
    ('0b9d6c8e-4f5a-4c1e-9a77-2f3c1d5e6a01', 'PAD-001', 'Padel Racket Pro',     '1899000.0000000000',                 12),
    ('5e2f1a3b-7c4d-4e8f-a1b2-c3d4e5f60702', 'BAL-003', 'Padel Balls (3 pack)', '89000.5000000000',                   240),
    ('9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c03', 'GRP-010', 'Overgrip "Tacky"',     '0.0000000001',                       0),
    ('c1d2e3f4-a5b6-4c7d-8e9f-0a1b2c3d4e04', 'BIG-999', 'Precision Ledger',     '1234567890123456789012345678.0123456789', 1);

CREATE TABLE events (
    id          BIGINT      NOT NULL AUTO_INCREMENT PRIMARY KEY,
    kind        VARCHAR(16) NOT NULL,
    payload     JSON        NOT NULL,
    occurred_at TIMESTAMP   NOT NULL,
    note        TEXT        NULL,
    INDEX idx_events_occurred_at (occurred_at)
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4;

-- 10^5 rows from a digits cross join: works on MySQL and MariaDB without
-- touching recursion limits. (A TEMPORARY table cannot be referenced twice
-- in one query, hence a regular helper table.)
CREATE TABLE _seed_digits (d TINYINT NOT NULL PRIMARY KEY);
INSERT INTO _seed_digits VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9);

INSERT INTO events (id, kind, payload, occurred_at, note)
SELECT n,
       ELT(n % 4 + 1, 'login', 'click', 'purchase', 'logout'),
       JSON_OBJECT('n', n, 'user_id', n % 5 + 1),
       TIMESTAMP '2026-01-01 00:00:00' + INTERVAL n SECOND,
       CASE n % 3 WHEN 0 THEN NULL WHEN 1 THEN '' ELSE CONCAT('note ', n) END
FROM (
    SELECT a.d + b.d * 10 + c.d * 100 + e.d * 1000 + f.d * 10000 + 1 AS n
    FROM _seed_digits a, _seed_digits b, _seed_digits c, _seed_digits e, _seed_digits f
) AS seq
ORDER BY n;

DROP TABLE _seed_digits;

CREATE TABLE `Mixed Case Table` (
    `Id`           INT          NOT NULL PRIMARY KEY,
    `Display Name` VARCHAR(255) NOT NULL,
    `isActive`     BOOLEAN      NOT NULL DEFAULT TRUE
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4;

INSERT INTO `Mixed Case Table` (`Id`, `Display Name`, `isActive`) VALUES
    (1, 'Alpha',          TRUE),
    (2, 'Beta "quoted"',  FALSE),
    (3, 'Gamma `tick`',   TRUE),
    (4, 'Δelta ünïcode',  FALSE);

CREATE TABLE audit_log (
    action    VARCHAR(32)  NOT NULL,
    actor     VARCHAR(255) NULL,
    logged_at TIMESTAMP    NOT NULL
) ENGINE = InnoDB DEFAULT CHARSET = utf8mb4;

INSERT INTO audit_log (action, actor, logged_at) VALUES
    ('login',  'ada', '2026-03-01 07:00:00'),
    ('login',  'ada', '2026-03-01 07:00:00'),
    ('delete', NULL,  '2026-03-02 18:30:00');

ANALYZE TABLE users, orders, products, events, `Mixed Case Table`, audit_log;
