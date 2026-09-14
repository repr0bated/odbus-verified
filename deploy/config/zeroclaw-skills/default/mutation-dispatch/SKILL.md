---
name: mutation-dispatch
description: Trace PluginService.CallMethod through MutationEngine for UI-facing plugins.
author: 3tched
version: 0.1.0
category: control-plane
---

# Mutation Dispatch

Use when a plugin method echoes `{}` or fails closed. Trace CallMethod → `MutationEngine::dispatch_method_call` → the plugin dispatch helper, then confirm state_cache / SHM projection updates.
