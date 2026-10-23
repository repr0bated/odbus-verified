# UI API Integration

## REST Client (`src/api/rest.ts`)

```typescript
import { api } from '@/api';

// Health check
await api.health();

// Chat
const res = await api.chat('hello', sessionId);

// Tools
const tools = await api.tools.list();
const tool = await api.tools.get('shell_execute');
const result = await api.tools.execute('shell_execute', { command: 'ls' });

// LLM
const status = await api.llm.status();
const models = await api.llm.models();
```

## WebSocket Client (`src/api/ws.ts`)

```typescript
import { ws } from '@/api';

// Connect
ws.connect();

// Subscribe to events
const unsub = ws.on('tool_executed', (data) => console.log(data));

// Send message
ws.send('subscribe', { topic: 'executions' });

// Cleanup
unsub();
ws.disconnect();
```

## State Stores (`src/stores/`)

```typescript
import { useAuthStore, useQuotaStore, useUiStore } from '@/stores';

// Auth
const { user, token, setUser, logout } = useAuthStore();

// Quota
const { used, limit, setQuota } = useQuotaStore();

// UI
const { sidebarOpen, theme, toggleSidebar, setTheme } = useUiStore();
```
