//! Native transport adapter for the MutationEngine generation owner. The
//! upstream stateless branch resolves one explicit agent/provider/model before
//! its versioned ACK and never constructs Agent/tools/memory. The WS adapter
//! must verify that ACK before sending the prompt; an older executable is not
//! compatible merely because its plugin contract has been rebuilt.
use crate::{
    generation_jobs_dispatch::GenerationBackend,
    tched_router_generation_native::{NativeGenerationAdmission, NativeGenerationTask},
    tched_router_runtime::TchedRouterRuntimeClient,
};
use anyhow::Result;
use op_plugins::state_plugins::tched_router::generation::StartGenerationInput;
use std::sync::Arc;

impl GenerationBackend for TchedRouterRuntimeClient {
    fn spawn(
        &self,
        input: StartGenerationInput,
        native_session: String,
        gate: Arc<dyn NativeGenerationAdmission>,
    ) -> Result<NativeGenerationTask> {
        self.spawn_stateless_generation(input, native_session, gate)
    }
}
