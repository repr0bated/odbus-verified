---
name: mutation-dispatch
description: Trace PluginService.CallMethod through MutationEngine for UI-facing plugins.
author: 3tched
version: 0.1.0
category: control-plane
---

# Mutation Dispatch

Use when a plugin method echoes `{}` or fails closed. Invoke the sealed
operation through its mounted 3tchedFS `call` file and inspect its own terminal
receipt or error. Reopen the projected state to verify the committed result.
The filesystem dispatches through MutationEngine; do not route around a
denial with direct HTTP, database or shell operations. Source-level debugging
may trace `MutationEngine::dispatch_method_call` to the plugin handler, but
source presence is not proof of a completed operation.
