//! Dynamic codec for gRPC frames carrying DynamicMessage payload.
//!
//! Encodes and decodes gRPC messages without type information, using
//! MessageDescriptor to handle serialization/deserialization.

use prost::Message;
use prost_reflect::{DynamicMessage, MessageDescriptor};
use tonic::Status;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

/// Encoder for dynamic messages.
pub struct DynamicEncoder {
    #[allow(dead_code)]
    output_descriptor: MessageDescriptor,
}

impl Encoder for DynamicEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        item.encode(dst)
            .map_err(|e| Status::internal(format!("failed to encode dynamic message: {}", e)))
    }
}

/// Decoder for dynamic messages.
pub struct DynamicDecoder {
    input_descriptor: MessageDescriptor,
}

impl Decoder for DynamicDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        let input_descriptor = self.input_descriptor.clone();
        DynamicMessage::decode(input_descriptor, src)
            .map(Some)
            .map_err(|e| Status::internal(format!("failed to decode dynamic message: {}", e)))
    }
}

/// Codec for routing through descriptor-mounted methods.
///
/// Handles encoding/decoding of DynamicMessage without type information,
/// used by the descriptor router to serve sealed blob methods at runtime.
pub struct DynamicCodec {
    input_descriptor: MessageDescriptor,
    output_descriptor: MessageDescriptor,
}

impl DynamicCodec {
    /// Create a codec for a given method's input and output messages.
    pub fn new(input_descriptor: MessageDescriptor, output_descriptor: MessageDescriptor) -> Self {
        Self {
            input_descriptor,
            output_descriptor,
        }
    }
}

impl Codec for DynamicCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynamicEncoder;
    type Decoder = DynamicDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        DynamicEncoder {
            output_descriptor: self.output_descriptor.clone(),
        }
    }

    fn decoder(&mut self) -> Self::Decoder {
        DynamicDecoder {
            input_descriptor: self.input_descriptor.clone(),
        }
    }
}
