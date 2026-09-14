# Error Handling Patterns

## Result Types (Rust/TypeScript)
```rust
// Rust
fn parse_config(path: &str) -> Result<Config, ConfigError> {
    let content = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

// Handle
match parse_config("config.toml") {
    Ok(config) => use_config(config),
    Err(ConfigError::NotFound) => use_defaults(),
    Err(e) => return Err(e.into()),
}
```

```typescript
// TypeScript
type Result<T, E> = { ok: true; value: T } | { ok: false; error: E };

function parseConfig(path: string): Result<Config, ConfigError> {
    // ...
}
```

## Exception Hierarchies
```python
class AppError(Exception):
    """Base application error"""
    pass

class ValidationError(AppError):
    """Input validation failed"""
    pass

class NotFoundError(AppError):
    """Resource not found"""
    pass

class AuthError(AppError):
    """Authentication/authorization failed"""
    pass
```

## Graceful Degradation
```python
async def get_user_profile(user_id: str) -> Profile:
    try:
        profile = await cache.get(f"profile:{user_id}")
        if profile:
            return profile
    except CacheError:
        logger.warning("Cache unavailable, falling back to DB")
    
    profile = await db.get_profile(user_id)
    
    try:
        await cache.set(f"profile:{user_id}", profile, ttl=300)
    except CacheError:
        pass  # Cache write failure is non-critical
    
    return profile
```

## Error Boundaries (React)
```tsx
class ErrorBoundary extends React.Component {
    state = { hasError: false };
    
    static getDerivedStateFromError(error) {
        return { hasError: true };
    }
    
    componentDidCatch(error, info) {
        logError(error, info);
    }
    
    render() {
        if (this.state.hasError) {
            return <FallbackUI />;
        }
        return this.props.children;
    }
}
```

## Best Practices
1. Fail fast on unrecoverable errors
2. Provide context in error messages
3. Log errors with stack traces
4. Use structured error types
5. Don't swallow exceptions silently
6. Document error conditions
