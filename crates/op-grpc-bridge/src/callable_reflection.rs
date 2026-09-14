//! Callable gRPC reflection snapshot.
//!
//! `tonic-reflection` serves an immutable `FileDescriptorSet`. The bridge must
//! only advertise services that are mounted in `build_operation_routes`.
//!
//! Plugin method services are generated in `build.rs` from `PluginSchema.methods`
//! into the `operation.plugin.v1` package. Those services accept and return
//! `google.protobuf.Struct`, are compiled into `operation_descriptor.bin`, and
//! are mounted by the generated route adder. Older per-method descriptor helpers
//! can synthesize `operation.method.*` descriptors, but those services are not
//! mounted here and therefore are not part of this reflection snapshot.

/// Return the static descriptor set for the callable gRPC surface.
pub fn descriptor_bytes() -> Vec<u8> {
    crate::proto::FILE_DESCRIPTOR_SET.to_vec()
}
