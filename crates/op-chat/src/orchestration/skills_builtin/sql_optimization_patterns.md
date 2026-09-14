# SQL Optimization Patterns

## EXPLAIN Analysis
```sql
EXPLAIN ANALYZE SELECT * FROM users WHERE email = 'user@example.com';

-- Look for:
-- - Seq Scan (bad for large tables)
-- - Index Scan (good)
-- - Nested Loop vs Hash Join
-- - Actual vs estimated rows
```

## Index Strategies
```sql
-- Single column index
CREATE INDEX idx_users_email ON users(email);

-- Composite index (column order matters!)
CREATE INDEX idx_orders_user_date ON orders(user_id, created_at DESC);

-- Partial index
CREATE INDEX idx_active_users ON users(email) WHERE active = true;

-- Covering index (includes all needed columns)
CREATE INDEX idx_users_covering ON users(email) INCLUDE (name, created_at);
```

## Query Optimization
```sql
-- Bad: SELECT *
SELECT * FROM users WHERE id = 1;

-- Good: Select only needed columns
SELECT id, name, email FROM users WHERE id = 1;

-- Bad: N+1 queries
for user in users:
    orders = db.query("SELECT * FROM orders WHERE user_id = ?", user.id)

-- Good: JOIN or batch
SELECT u.*, o.* FROM users u
LEFT JOIN orders o ON o.user_id = u.id
WHERE u.active = true;

-- Pagination
SELECT * FROM posts
WHERE id < :last_id
ORDER BY id DESC
LIMIT 20;
```

## Common Anti-Patterns
```sql
-- Avoid functions on indexed columns
WHERE LOWER(email) = 'user@example.com'  -- Won't use index
WHERE email = 'user@example.com'          -- Uses index

-- Avoid OR, use UNION or IN
WHERE status = 'active' OR status = 'pending'  -- Slower
WHERE status IN ('active', 'pending')           -- Faster

-- Avoid SELECT DISTINCT when not needed
-- Use EXISTS instead of COUNT for existence checks
```

## Connection Pooling
- Use PgBouncer or built-in pooling
- Set appropriate pool size (CPU cores * 2-4)
- Monitor connection usage
