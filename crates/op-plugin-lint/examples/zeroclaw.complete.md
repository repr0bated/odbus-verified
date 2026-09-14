# Complete plugin: zeroclaw

**Contract:** PLUGIN-RENDER-CONTRACT.md

**Source:** `zeroclaw.rs`

**Identity:** zeroclaw 1.0.0 (llm) — Zeroclaw schema/RPC-native model router for Antigravity UI, CLI providers, and structured JSON output

**State root:** `ZeroclawState`

## Fields

| Field | Rust type | Subid | Doc |
|---|---|---|---|
| `configurable_options` | `ConfigurableOptions` | sch.service.zeroclaw.options@v1 | Configurable options schema: registration, user container options, identity chain, memory namespaces, and privacy rules. |
| `model_assignments` | `ModelAssignments` | mut.service.zeroclaw.model-assignments@v1 | Per-capability model assignments. |
| `projection` | `LlmProjection` | sch.software.zeroclaw.llm-projection@v1 | Shared LLM projection fields (flattened to the top level). |
| `selected_model` | `String` | exp.service.zeroclaw.selected-model@v1 | Selected model identifier. |
| `selected_provider` | `String` | mut.service.zeroclaw.selected-provider@v1 | Selected provider identifier. |
| `status` | `String` | obs.software.zeroclaw.status@v1 | Operational status. |
| `transport` | `LlmTransport` | mut.service.zeroclaw.transport@v1 | Transport layer metadata. |

## Methods (typed)

### `Chat`

- side_effect: `read`
- idempotent: false
- capability: `cap.software.zeroclaw.chat@v1`
- subid: `exp.service.zeroclaw.chat@v1`
- input: `ChatInput` → output: `ChatOutput`

**args**

```json
{
  "properties": {
    "message": {
      "description": "Compatibility form for callers sending a single user turn.",
      "type": "string"
    },
    "messages": {
      "description": "Full ordered conversation. When non-empty this takes precedence over `message`.",
      "type": "array"
    },
    "model": {
      "description": "Model id or route hint. Empty uses `selected_model`.",
      "type": "string"
    },
    "provider": {
      "description": "Provider id, route, or alias. Empty uses `selected_provider`.",
      "type": "string"
    }
  },
  "title": "ChatInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "content": {
      "description": "Assistant response text.",
      "type": "string"
    },
    "finish_reason": {
      "description": "Provider finish reason, or an empty string when none was supplied.",
      "type": "string"
    },
    "model": {
      "description": "Resolved model identifier.",
      "type": "string"
    },
    "provider": {
      "description": "Resolved upstream provider identifier.",
      "type": "string"
    },
    "usage": {
      "description": "Provider-specific token usage object.",
      "type": "object",
      "x-rust-type": "JsonValue"
    }
  },
  "required": [
    "content",
    "finish_reason",
    "model",
    "provider",
    "usage"
  ],
  "title": "ChatOutput",
  "type": "object"
}
```

### `GetConfigSchema`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.config-schema.read@v1`
- subid: `obs.service.zeroclaw.config-schema.get@v1`
- input: `EmptyZeroclawInput` → output: `GetConfigSchemaOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "config_schema": {
      "type": "object",
      "x-rust-type": "ConfigSchema"
    }
  },
  "required": [
    "config_schema"
  ],
  "title": "GetConfigSchemaOutput",
  "type": "object"
}
```

### `GetConfigurableOptions`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.options.read@v1`
- subid: `obs.service.zeroclaw.options.get@v1`
- input: `EmptyZeroclawInput` → output: `GetConfigurableOptionsOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "configurable_options": {
      "type": "object",
      "x-rust-type": "ConfigurableOptions"
    }
  },
  "required": [
    "configurable_options"
  ],
  "title": "GetConfigurableOptionsOutput",
  "type": "object"
}
```

### `GetModelAssignments`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.model-assignments.read@v1`
- subid: `obs.service.zeroclaw.model-assignments.get@v1`
- input: `EmptyZeroclawInput` → output: `GetModelAssignmentsOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "model_assignments": {
      "type": "object",
      "x-rust-type": "ModelAssignments"
    }
  },
  "required": [
    "model_assignments"
  ],
  "title": "GetModelAssignmentsOutput",
  "type": "object"
}
```

### `GetModelRoutes`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.routes.read@v1`
- subid: `obs.service.zeroclaw.model-routes.list@v1`
- input: `EmptyZeroclawInput` → output: `GetModelRoutesOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "model_routes": {
      "description": "Declared model routes.",
      "type": "array"
    }
  },
  "required": [
    "model_routes"
  ],
  "title": "GetModelRoutesOutput",
  "type": "object"
}
```

### `GetProviderCatalog`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.providers.read@v1`
- subid: `obs.service.zeroclaw.provider-catalog.list@v1`
- input: `EmptyZeroclawInput` → output: `GetProviderCatalogOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "providers": {
      "description": "Declared providers.",
      "type": "array"
    }
  },
  "required": [
    "providers"
  ],
  "title": "GetProviderCatalogOutput",
  "type": "object"
}
```

### `GetRouter`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.router.read@v1`
- subid: `obs.service.zeroclaw.router.get@v1`
- input: `EmptyZeroclawInput` → output: `GetRouterOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "router": {
      "type": "object",
      "x-rust-type": "Router"
    }
  },
  "required": [
    "router"
  ],
  "title": "GetRouterOutput",
  "type": "object"
}
```

### `GetState`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.state.read@v1`
- subid: `obs.service.zeroclaw.state.get@v1`
- input: `EmptyZeroclawInput` → output: `GetStateOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "state": {
      "description": "Complete projected ZeroClaw state.",
      "type": "object",
      "x-rust-type": "ZeroclawState"
    }
  },
  "required": [
    "state"
  ],
  "title": "GetStateOutput",
  "type": "object"
}
```

### `GetStructuredOutput`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.structured-output.read@v1`
- subid: `obs.service.zeroclaw.structured-output.get@v1`
- input: `EmptyZeroclawInput` → output: `GetStructuredOutputOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "structured_output": {
      "type": "object",
      "x-rust-type": "StructuredOutput"
    }
  },
  "required": [
    "structured_output"
  ],
  "title": "GetStructuredOutputOutput",
  "type": "object"
}
```

### `GetTools`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.tools.read@v1`
- subid: `obs.service.zeroclaw.tools.list@v1`
- input: `EmptyZeroclawInput` → output: `GetToolsOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "tools": {
      "description": "Declared LLM tools.",
      "type": "array"
    }
  },
  "required": [
    "tools"
  ],
  "title": "GetToolsOutput",
  "type": "object"
}
```

### `ListModels`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.models.read@v1`
- subid: `obs.service.zeroclaw.models.list@v1`
- input: `ListModelsInput` → output: `ListModelsOutput`

**args**

```json
{
  "properties": {
    "provider": {
      "description": "Optional provider id, route, or alias used to filter the model catalog.",
      "type": "string"
    }
  },
  "title": "ListModelsInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "model_routes": {
      "description": "Schema-declared model routes, optionally filtered by provider.",
      "type": "array"
    }
  },
  "required": [
    "model_routes"
  ],
  "title": "ListModelsOutput",
  "type": "object"
}
```

### `ListProviders`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.providers.read@v1`
- subid: `obs.service.zeroclaw.providers.list@v1`
- input: `EmptyZeroclawInput` → output: `ListProvidersOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "providers": {
      "type": "array"
    }
  },
  "required": [
    "providers"
  ],
  "title": "ListProvidersOutput",
  "type": "object"
}
```

### `ListUiSurfaces`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.ui-surfaces.read@v1`
- subid: `obs.service.zeroclaw.ui-surfaces.list@v1`
- input: `EmptyZeroclawInput` → output: `ListUiSurfacesOutput`

**args**

```json
{
  "properties": {},
  "title": "EmptyZeroclawInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "ui_surfaces": {
      "type": "array"
    }
  },
  "required": [
    "ui_surfaces"
  ],
  "title": "ListUiSurfacesOutput",
  "type": "object"
}
```

### `ResolveRoute`

- side_effect: `read`
- idempotent: true
- capability: `cap.software.zeroclaw.route.resolve@v1`
- subid: `obs.service.zeroclaw.route.resolve@v1`
- input: `ResolveRouteInput` → output: `ResolveRouteOutput`

**args**

```json
{
  "properties": {
    "hint": {
      "description": "Route hint or model identifier.",
      "type": "string"
    }
  },
  "required": [
    "hint"
  ],
  "title": "ResolveRouteInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "route": {
      "description": "Resolved route.",
      "type": "object",
      "x-rust-type": "ModelRoute"
    }
  },
  "required": [
    "route"
  ],
  "title": "ResolveRouteOutput",
  "type": "object"
}
```

### `SetModel`

- side_effect: `mutation`
- idempotent: false
- capability: `cap.software.zeroclaw.model.set@v1`
- subid: `mut.service.zeroclaw.model.set@v1`
- input: `SetModelInput` → output: `SetModelOutput`

**args**

```json
{
  "properties": {
    "model_id": {
      "description": "Model identifier declared in the model route catalog.",
      "type": "string"
    }
  },
  "required": [
    "model_id"
  ],
  "title": "SetModelInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "selected_model": {
      "description": "Selected model identifier.",
      "type": "string"
    }
  },
  "required": [
    "selected_model"
  ],
  "title": "SetModelOutput",
  "type": "object"
}
```

### `SetObfuscationModel`

- side_effect: `mutation`
- idempotent: false
- capability: `cap.software.zeroclaw.model-assignments.obfuscation.set@v1`
- subid: `mut.service.zeroclaw.model-assignments.obfuscation.set@v1`
- input: `SetObfuscationModelInput` → output: `SetObfuscationModelOutput`

**args**

```json
{
  "properties": {
    "model_id": {
      "type": "string"
    }
  },
  "required": [
    "model_id"
  ],
  "title": "SetObfuscationModelInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "obfuscation": {
      "type": "string"
    }
  },
  "required": [
    "obfuscation"
  ],
  "title": "SetObfuscationModelOutput",
  "type": "object"
}
```

### `SetOvsRoutingModel`

- side_effect: `mutation`
- idempotent: false
- capability: `cap.software.zeroclaw.model-assignments.ovs-routing.set@v1`
- subid: `mut.service.zeroclaw.model-assignments.ovs-routing.set@v1`
- input: `SetOvsRoutingModelInput` → output: `SetOvsRoutingModelOutput`

**args**

```json
{
  "properties": {
    "model_id": {
      "type": "string"
    }
  },
  "required": [
    "model_id"
  ],
  "title": "SetOvsRoutingModelInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "ovs_routing": {
      "type": "string"
    }
  },
  "required": [
    "ovs_routing"
  ],
  "title": "SetOvsRoutingModelOutput",
  "type": "object"
}
```

### `SetProvider`

- side_effect: `mutation`
- idempotent: false
- capability: `cap.software.zeroclaw.provider.set@v1`
- subid: `mut.service.zeroclaw.provider.set@v1`
- input: `SetProviderInput` → output: `SetProviderOutput`

**args**

```json
{
  "properties": {
    "provider_id": {
      "description": "Provider identifier declared in the provider catalog.",
      "type": "string"
    }
  },
  "required": [
    "provider_id"
  ],
  "title": "SetProviderInput",
  "type": "object"
}
```

**returns**

```json
{
  "properties": {
    "selected_provider": {
      "description": "Selected provider identifier.",
      "type": "string"
    }
  },
  "required": [
    "selected_provider"
  ],
  "title": "SetProviderOutput",
  "type": "object"
}
```

## Audit

Status: FAIL (fail=41 warn=90 hint=41)

## Introspect findings (gaps vs plugin)

- surface: repomix:/home/jeremy/zeroclaw/repomix-output.xml (repomix)
- element_paths: 21220
- missing_from_plugin: **11590**
- missing_cli_commands: **374**
- missing_config_fields: **743**

Introspected findings not represented in the plugin .rs — this is the gap list. Full catalog is --surface-out.

### CLI commands not in plugin

- `enum.zeroclaw.AgentsCommands.Create`
- `enum.zeroclaw.AgentsCommands.Delete`
- `enum.zeroclaw.AgentsCommands.List`
- `enum.zeroclaw.AgentsCommands.Rename`
- `enum.zeroclaw.AuthCommands.EmailLogin`
- `enum.zeroclaw.AuthCommands.List`
- `enum.zeroclaw.AuthCommands.Login`
- `enum.zeroclaw.AuthCommands.Logout`
- `enum.zeroclaw.AuthCommands.PasteRedirect`
- `enum.zeroclaw.AuthCommands.PasteToken`
- `enum.zeroclaw.AuthCommands.Refresh`
- `enum.zeroclaw.AuthCommands.SetupToken`
- `enum.zeroclaw.AuthCommands.Use`
- `enum.zeroclaw.ChannelCommands.Add`
- `enum.zeroclaw.ChannelCommands.BindTelegram`
- `enum.zeroclaw.ChannelCommands.Doctor`
- `enum.zeroclaw.ChannelCommands.List`
- `enum.zeroclaw.ChannelCommands.Remove`
- `enum.zeroclaw.ChannelCommands.Send`
- `enum.zeroclaw.ChannelCommands.Start`
- `enum.zeroclaw.ChannelsCommands.Create`
- `enum.zeroclaw.ChannelsCommands.Delete`
- `enum.zeroclaw.ChannelsCommands.List`
- `enum.zeroclaw.ChannelsCommands.Rename`
- `enum.zeroclaw.Commands.Acp`
- `enum.zeroclaw.Commands.Agent`
- `enum.zeroclaw.Commands.Agents`
- `enum.zeroclaw.Commands.Browse`
- `enum.zeroclaw.Commands.Channel`
- `enum.zeroclaw.Commands.Channels`
- `enum.zeroclaw.Commands.Completions`
- `enum.zeroclaw.Commands.Config`
- `enum.zeroclaw.Commands.Cron`
- `enum.zeroclaw.Commands.Daemon`
- `enum.zeroclaw.Commands.Desktop`
- `enum.zeroclaw.Commands.Doctor`
- `enum.zeroclaw.Commands.Estop`
- `enum.zeroclaw.Commands.Eval`
- `enum.zeroclaw.Commands.Gateway`
- `enum.zeroclaw.Commands.Hardware`
- `enum.zeroclaw.Commands.Integrations`
- `enum.zeroclaw.Commands.MarkdownHelp`
- `enum.zeroclaw.Commands.MarkdownSchema`
- `enum.zeroclaw.Commands.Memory`
- `enum.zeroclaw.Commands.Migrate`
- `enum.zeroclaw.Commands.Models`
- `enum.zeroclaw.Commands.Onboard`
- `enum.zeroclaw.Commands.Peripheral`
- `enum.zeroclaw.Commands.Plugin`
- `enum.zeroclaw.Commands.Props`
- `enum.zeroclaw.Commands.Quickstart`
- `enum.zeroclaw.Commands.Security`
- `enum.zeroclaw.Commands.SelfTest`
- `enum.zeroclaw.Commands.Skills`
- `enum.zeroclaw.Commands.Sop`
- `enum.zeroclaw.Commands.Update`
- `enum.zeroclaw.ConfigCommands.Complete`
- `enum.zeroclaw.ConfigCommands.Docs`
- `enum.zeroclaw.ConfigCommands.Generate`
- `enum.zeroclaw.ConfigCommands.Get`
- `enum.zeroclaw.ConfigCommands.Init`
- `enum.zeroclaw.ConfigCommands.List`
- `enum.zeroclaw.ConfigCommands.Migrate`
- `enum.zeroclaw.ConfigCommands.Patch`
- `enum.zeroclaw.ConfigCommands.Set`
- `enum.zeroclaw.CronCommands.Add`
- `enum.zeroclaw.CronCommands.AddAt`
- `enum.zeroclaw.CronCommands.AddEvery`
- `enum.zeroclaw.CronCommands.List`
- `enum.zeroclaw.CronCommands.Once`
- `enum.zeroclaw.CronCommands.Pause`
- `enum.zeroclaw.CronCommands.Remove`
- `enum.zeroclaw.CronCommands.Resume`
- `enum.zeroclaw.CronCommands.Update`
- `enum.zeroclaw.DeprecatedPropsCommands.Any`
- `enum.zeroclaw.DoctorCommands.Models`
- `enum.zeroclaw.DoctorCommands.Traces`
- `enum.zeroclaw.DoctorCommands.UpdateContextWindows`
- `enum.zeroclaw.EvalCommands.Run`
- `enum.zeroclaw.GatewayCommands.GetPaircode`
- `enum.zeroclaw.GatewayCommands.Restart`
- `enum.zeroclaw.GatewayCommands.Start`
- `enum.zeroclaw.HardwareCommands.Discover`
- `enum.zeroclaw.HardwareCommands.Info`
- `enum.zeroclaw.HardwareCommands.Introspect`
- `enum.zeroclaw.IntegrationCommands.Info`
- `enum.zeroclaw.MemoryCommands.Clear`
- `enum.zeroclaw.MemoryCommands.Get`
- `enum.zeroclaw.MemoryCommands.List`
- `enum.zeroclaw.MemoryCommands.Reindex`
- `enum.zeroclaw.MemoryCommands.Stats`
- `enum.zeroclaw.MigrateCommands.Openclaw`
- `enum.zeroclaw.ModelCommands.List`
- `enum.zeroclaw.ModelCommands.Refresh`
- `enum.zeroclaw.PeripheralCommands.Add`
- `enum.zeroclaw.PeripheralCommands.Flash`
- `enum.zeroclaw.PeripheralCommands.FlashNucleo`
- `enum.zeroclaw.PeripheralCommands.List`
- `enum.zeroclaw.PeripheralCommands.SetupUnoQ`
- `enum.zeroclaw.PluginCommands.Info`
- `enum.zeroclaw.PluginCommands.Install`
- `enum.zeroclaw.PluginCommands.List`
- `enum.zeroclaw.PluginCommands.Migrate`
- `enum.zeroclaw.PluginCommands.Remove`
- `enum.zeroclaw.PluginCommands.Search`
- `enum.zeroclaw.ProvidersCommands.Create`
- `enum.zeroclaw.ProvidersCommands.Delete`
- `enum.zeroclaw.ProvidersCommands.Rename`
- `enum.zeroclaw.ServiceCommands.Install`
- `enum.zeroclaw.ServiceCommands.Logs`
- `enum.zeroclaw.ServiceCommands.Restart`
- `enum.zeroclaw.ServiceCommands.Start`
- `enum.zeroclaw.ServiceCommands.Stop`
- `enum.zeroclaw.ServiceCommands.Uninstall`
- `enum.zeroclaw.SkillBundleCommands.Add`
- `enum.zeroclaw.SkillBundleCommands.List`
- `enum.zeroclaw.SkillBundleCommands.Remove`
- `enum.zeroclaw.SkillBundleCommands.Rename`
- `enum.zeroclaw.SkillBundleCommands.Show`
- `enum.zeroclaw.SkillCommands.Add`
- … +254 more

### Config / struct fields not in plugin (sample)

- `enum.zeroclaw_config.AccessMode`
- `enum.zeroclaw_config.AccessMode.Read`
- `enum.zeroclaw_config.AccessMode.ReadWrite`
- `enum.zeroclaw_config.AccessMode.Write`
- `enum.zeroclaw_config.AliasKind`
- `enum.zeroclaw_config.AliasKind.Agent`
- `enum.zeroclaw_config.AliasKind.Channel`
- `enum.zeroclaw_config.AliasSource`
- `enum.zeroclaw_config.AliasSource.Agents`
- `enum.zeroclaw_config.AliasSource.Channels`
- `enum.zeroclaw_config.AliasSource.KnowledgeBundles`
- `enum.zeroclaw_config.AliasSource.McpBundles`
- `enum.zeroclaw_config.AliasSource.ModelProviders`
- `enum.zeroclaw_config.AliasSource.RiskProfiles`
- `enum.zeroclaw_config.AliasSource.RuntimeProfiles`
- `enum.zeroclaw_config.AliasSource.SkillBundles`
- `enum.zeroclaw_config.AliasSource.TranscriptionProviders`
- `enum.zeroclaw_config.AliasSource.TtsProviders`
- `enum.zeroclaw_config.AutonomyLevel`
- `enum.zeroclaw_config.AutonomyLevel.Full`
- `enum.zeroclaw_config.AutonomyLevel.ReadOnly`
- `enum.zeroclaw_config.AutonomyLevel.Supervised`
- `enum.zeroclaw_config.BudgetCheck`
- `enum.zeroclaw_config.BudgetCheck.Allowed`
- `enum.zeroclaw_config.BudgetCheck.Exceeded`
- `enum.zeroclaw_config.BudgetCheck.Warning`
- `enum.zeroclaw_config.BundleDirectoryError`
- `enum.zeroclaw_config.BundleDirectoryError.DirectoryCollision`
- `enum.zeroclaw_config.BundleDirectoryError.EscapesShared`
- `enum.zeroclaw_config.BundleDirectoryError.UnknownBundle`
- `enum.zeroclaw_config.CascadeError`
- `enum.zeroclaw_config.CascadeError.NotFound`
- `enum.zeroclaw_config.CascadeError.NotImplemented`
- `enum.zeroclaw_config.CascadeError.PostCondition`
- `enum.zeroclaw_config.CascadeError.Refused`
- `enum.zeroclaw_config.CascadePolicy`
- `enum.zeroclaw_config.CascadePolicy.DryRun`
- `enum.zeroclaw_config.CascadePolicy.RefuseOnHard`
- `enum.zeroclaw_config.CommandRiskLevel`
- `enum.zeroclaw_config.CommandRiskLevel.High`
- `enum.zeroclaw_config.CommandRiskLevel.Low`
- `enum.zeroclaw_config.CommandRiskLevel.Medium`
- `enum.zeroclaw_config.ConfigApiCode`
- `enum.zeroclaw_config.ConfigApiCode.ConfigChangedExternally`
- `enum.zeroclaw_config.ConfigApiCode.DanglingReference`
- `enum.zeroclaw_config.ConfigApiCode.InternalError`
- `enum.zeroclaw_config.ConfigApiCode.InvalidEnumVariant`
- `enum.zeroclaw_config.ConfigApiCode.InvalidFormat`
- `enum.zeroclaw_config.ConfigApiCode.InvalidNumericRange`
- `enum.zeroclaw_config.ConfigApiCode.OpNotSupported`
- `enum.zeroclaw_config.ConfigApiCode.PathNotFound`
- `enum.zeroclaw_config.ConfigApiCode.ReloadFailed`
- `enum.zeroclaw_config.ConfigApiCode.RequiredFieldEmpty`
- `enum.zeroclaw_config.ConfigApiCode.SecretTestForbidden`
- `enum.zeroclaw_config.ConfigApiCode.ValidationFailed`
- `enum.zeroclaw_config.ConfigApiCode.ValueTypeMismatch`
- `enum.zeroclaw_config.ConfigTab`
- `enum.zeroclaw_config.ConfigTab.Advanced`
- `enum.zeroclaw_config.ConfigTab.Behavior`
- `enum.zeroclaw_config.ConfigTab.Bundles`
- `enum.zeroclaw_config.ConfigTab.Channels`
- `enum.zeroclaw_config.ConfigTab.Connection`
- `enum.zeroclaw_config.ConfigTab.Costs`
- `enum.zeroclaw_config.ConfigTab.Cron`
- `enum.zeroclaw_config.ConfigTab.General`
- `enum.zeroclaw_config.ConfigTab.Limits`
- `enum.zeroclaw_config.ConfigTab.Memory`
- `enum.zeroclaw_config.ConfigTab.None`
- `enum.zeroclaw_config.ConfigTab.PeerGroups`
- `enum.zeroclaw_config.ConfigTab.Personality`
- `enum.zeroclaw_config.ConfigTab.Servers`
- `enum.zeroclaw_config.ConfigTab.Settings`
- `enum.zeroclaw_config.ConfigTab.Skills`
- `enum.zeroclaw_config.ConfigTab.Tuning`
- `enum.zeroclaw_config.ConfigTab.Workspace`
- `enum.zeroclaw_config.CreateError`
- `enum.zeroclaw_config.CreateError.Invalid`
- `enum.zeroclaw_config.CreateError.Reserved`
- `enum.zeroclaw_config.CredentialSurfaceClass`
- `enum.zeroclaw_config.CredentialSurfaceClass.EncryptedSecret`
