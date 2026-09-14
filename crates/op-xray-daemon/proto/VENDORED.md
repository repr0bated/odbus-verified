# Vendored Xray-core commander protos

Source: https://github.com/XTLS/Xray-core
Tag: v26.3.27
Commit: d2758a023cd7f4174a5a5fa4ff66e487d4342ba0

The original vendor pass selected this revision to match its observed binary.
No current runtime version or live service availability is inferred from these
files. These layouts are version dependent: use a compatible prepared Xray
binary and re-vendor against its exact revision when changing that dependency.

The vendored services are StatsService, RoutingService, LoggerService and
HandlerService. HandlerService's exact upstream dependency closure additionally
contains the core handler envelope, receiver/stream settings, common protocol
user/address/port types, VLESS and VMess accounts/inbounds, Trojan configuration,
and TLS transport configuration. Files retain their original upstream package,
field numbers and contents; all come from the commit above. Data-plane
GRPCService/Hunk transport is unrelated and is not vendored.

These files are backing wire dependencies, not a second public plugin schema.
The public `xray` contract is defined once in
`crates/op-plugins/src/state_plugins/xray.rs`, then sealed by op-blob. Generated
HandlerService server types are used only by isolated transport fixtures; the
bridge does not mount a new upstream service on its public listener.

The implemented runtime mutations translate typed VLESS/VMess/Trojan users and
TCP listeners into the upstream protobuf messages. TLS credentials are inline
and bounded; plaintext listeners are restricted to loopback. No JSON payload is
passed off as protobuf config. The commander configuration must already enable
HandlerService. These operations do not persist or rewrite
`/etc/xray/xray_config.json`, reload Xray, or establish deployment readiness.

Upstream behavioral references at the same revision:

- `app/proxyman/command/command.go`: `AlterInbound` uses typed operations;
  `AddInbound` consumes `core.InboundHandlerConfig`.
- `infra/conf/vless.go`: inbound account encryption is empty while the inbound
  config's supported unencrypted setting is `decryption = "none"`.
- `proxy/vless/validator.go`: `ProcessUUID` masks UUID bytes 6 and 7; the adapter
  checks this actual identity equivalence before adding duplicate credentials.
- `proxy/trojan/validator.go`: reused credentials can replace a user map entry;
  native account/user preflight prevents that through the plugin path.
