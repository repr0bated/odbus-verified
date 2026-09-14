// Re-export generated protobuf types
#[allow(clippy::all)]
#[allow(warnings)]
pub mod op_chat_orchestration {
    include!("op_chat.orchestration.rs");
}

#[allow(clippy::all)]
#[allow(warnings)]
pub mod op_chat_chat {
    include!("op_chat.chat.rs");
}

/// Combined file descriptor set for tonic-reflection.
/// Includes both orchestration and chat protos.
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("op_chat_descriptor");
