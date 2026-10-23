//! Preserve the receiver's assigned browser binding for existing SHM streams.
//!
//! Selector admission and private capabilities stay with each stream owner.
//! This wrapper only validates the transport proof and authored target again
//! before response headers and every frame, including existing heartbeats.

use std::pin::Pin;
use std::sync::Arc;

use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status};

use crate::mutation_engine::MutationEngine;
use crate::oracle_assertion::{BoundOracleProof, HumanPrincipalIdentity};
use crate::proto::StateChange;

type StateStream = Pin<Box<dyn Stream<Item = Result<StateChange, Status>> + Send>>;

pub(crate) struct StateSubscriptionBinding {
    engine: Arc<MutationEngine>,
    identity: HumanPrincipalIdentity,
    proof: Option<BoundOracleProof>,
}

impl StateSubscriptionBinding {
    pub(crate) fn new<T>(
        engine: Arc<MutationEngine>,
        request: &Request<T>,
        identity: HumanPrincipalIdentity,
    ) -> Self {
        Self {
            engine,
            identity,
            proof: request.extensions().get::<BoundOracleProof>().cloned(),
        }
    }

    async fn validate(&self) -> Result<(), Status> {
        let Some(proof) = &self.proof else {
            return Ok(());
        };
        // Only expiry is renewable by the browser. Revocation, target changes
        // and credential replacement must stop rather than reconnect blindly.
        if !proof.is_current(chrono::Utc::now().timestamp()) {
            return Err(Status::unauthenticated("Expired"));
        }
        super::revalidate_bound_oracle(
            &self.engine,
            proof,
            &self.identity.principal_id,
            Some(&self.identity.session_id),
            Some(&self.identity.session_genesis),
            true,
        )
        .await
        .map_err(|_| Status::unauthenticated("Assigned browser identity changed"))
    }

    pub(crate) async fn response(
        self,
        mut stream: StateStream,
    ) -> Result<Response<StateStream>, Status> {
        self.validate().await?;
        let proof = self.proof.clone();
        let stream: StateStream = if proof.is_some() {
            Box::pin(async_stream::stream! {
                while let Some(frame) = stream.next().await {
                    let frame = match frame {
                        Ok(frame) => frame,
                        Err(status) => { yield Err(status); return; }
                    };
                    if let Err(status) = self.validate().await {
                        yield Err(status);
                        return;
                    }
                    yield Ok(frame);
                }
            })
        } else {
            stream
        };
        let mut response = Response::new(stream);
        super::mark_bound_oracle_response(&mut response, proof.as_ref())?;
        Ok(response)
    }
}
