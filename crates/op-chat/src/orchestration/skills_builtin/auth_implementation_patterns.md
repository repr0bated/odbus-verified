# Authentication Implementation Patterns

## JWT Authentication
```python
import jwt
from datetime import datetime, timedelta

def create_token(user_id: str, secret: str) -> str:
    payload = {
        "sub": user_id,
        "iat": datetime.utcnow(),
        "exp": datetime.utcnow() + timedelta(hours=24)
    }
    return jwt.encode(payload, secret, algorithm="HS256")

def verify_token(token: str, secret: str) -> dict:
    return jwt.decode(token, secret, algorithms=["HS256"])
```

## OAuth 2.0 Flows
- **Authorization Code**: Web apps with backend
- **PKCE**: SPAs and mobile apps
- **Client Credentials**: Server-to-server
- **Refresh Tokens**: Long-lived sessions

## Session Management
```python
# Server-side sessions
session_store.set(session_id, {
    "user_id": user.id,
    "expires": datetime.utcnow() + timedelta(days=7)
})

# Cookie settings
response.set_cookie(
    "session_id",
    value=session_id,
    httponly=True,
    secure=True,
    samesite="Lax"
)
```

## RBAC Implementation
```python
class Permission(Enum):
    READ = "read"
    WRITE = "write"
    ADMIN = "admin"

def check_permission(user: User, resource: str, action: Permission) -> bool:
    user_roles = get_user_roles(user)
    for role in user_roles:
        if has_permission(role, resource, action):
            return True
    return False
```

## Best Practices
1. Never store plaintext passwords
2. Use bcrypt/argon2 for password hashing
3. Implement rate limiting on auth endpoints
4. Use short-lived access tokens + refresh tokens
5. Validate all tokens server-side
6. Implement proper logout (invalidate tokens)
