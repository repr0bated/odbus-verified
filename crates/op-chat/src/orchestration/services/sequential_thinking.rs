//! SequentialThinkingService gRPC service implementation.
//!
//! Backed by an in-memory `HashMap<String, ThinkingChain>` behind `RwLock`.

use std::pin::Pin;

use tokio::sync::broadcast;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::info;

use crate::orchestration::proto::op_chat_orchestration::{
    sequential_thinking_service_server::SequentialThinkingService, AddThoughtRequest,
    AddThoughtResponse, ChainStatus, ConcludeRequest, ConcludeResponse, ForkChainRequest,
    ForkChainResponse, GetChainRequest, GetChainResponse, StartChainRequest, StartChainResponse,
    ThoughtEvent,
};

use super::{ChainStatusInternal, OrchestrationServer, ThinkingChain, ThoughtEntry};

#[tonic::async_trait]
impl SequentialThinkingService for OrchestrationServer {
    /// Start a new thinking chain. Creates a chain with a UUID, stores it.
    async fn start_chain(
        &self,
        request: Request<StartChainRequest>,
    ) -> Result<Response<StartChainResponse>, Status> {
        let req = request.into_inner();
        let chain_id = uuid::Uuid::new_v4().to_string();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let max_steps = if req.max_steps > 0 { req.max_steps } else { 10 };

        let (event_tx, _) = broadcast::channel(64);

        let chain = ThinkingChain {
            id: chain_id.clone(),
            problem: req.problem,
            thoughts: Vec::new(),
            status: ChainStatusInternal::Thinking,
            max_steps,
            conclusion: None,
            started_at: now_ms,
            completed_at: None,
            context: req.context,
            style: req.style,
            event_tx,
        };

        self.chains.write().await.insert(chain_id.clone(), chain);

        info!(chain_id = %chain_id, max_steps, "StartChain");

        Ok(Response::new(StartChainResponse {
            success: true,
            chain_id,
            max_steps,
        }))
    }

    /// Add a thought to an existing chain.
    async fn add_thought(
        &self,
        request: Request<AddThoughtRequest>,
    ) -> Result<Response<AddThoughtResponse>, Status> {
        let req = request.into_inner();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut chains = self.chains.write().await;
        let chain = chains
            .get_mut(&req.chain_id)
            .ok_or_else(|| Status::not_found(format!("Chain not found: {}", req.chain_id)))?;

        // Check if chain is still accepting thoughts
        if chain.status == ChainStatusInternal::Complete {
            return Err(Status::failed_precondition("Chain is already complete"));
        }

        let step = chain.thoughts.len() as i32 + 1;
        let remaining = chain.max_steps - step;

        // Check max_steps
        if step > chain.max_steps {
            chain.status = ChainStatusInternal::Stuck;
            return Err(Status::resource_exhausted(format!(
                "Chain reached max steps ({})",
                chain.max_steps
            )));
        }

        let entry = ThoughtEntry {
            step,
            thought: req.thought.clone(),
            thought_type: req.thought_type,
            confidence: req.confidence,
            timestamp_ms: now_ms,
        };

        chain.thoughts.push(entry);

        // Update status based on step progress
        if remaining <= 1 {
            chain.status = ChainStatusInternal::Concluding;
        } else if step > 1 {
            chain.status = ChainStatusInternal::Analyzing;
        }

        // Broadcast the thought event
        let event = ThoughtEvent {
            chain_id: req.chain_id.clone(),
            step,
            total_steps: chain.max_steps,
            thought: req.thought,
            thought_type: req.thought_type,
            confidence: req.confidence,
            status: chain.status.to_proto(),
            timestamp_ms: now_ms,
        };
        let _ = chain.event_tx.send(event);

        Ok(Response::new(AddThoughtResponse {
            success: true,
            chain_id: req.chain_id,
            step,
            remaining_steps: remaining.max(0),
        }))
    }

    // -- streaming -----------------------------------------------------------

    type ThinkStreamStream =
        Pin<Box<dyn Stream<Item = Result<ThoughtEvent, Status>> + Send + 'static>>;

    /// Stream the thinking process for a new chain.
    ///
    /// Creates a chain from the request, then subscribes to its broadcast
    /// channel, yielding `ThoughtEvent`s until the chain is complete.
    async fn think_stream(
        &self,
        request: Request<StartChainRequest>,
    ) -> Result<Response<Self::ThinkStreamStream>, Status> {
        let req = request.into_inner();
        let chain_id = uuid::Uuid::new_v4().to_string();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let max_steps = if req.max_steps > 0 { req.max_steps } else { 10 };

        let (event_tx, mut event_rx) = broadcast::channel(64);

        let chain = ThinkingChain {
            id: chain_id.clone(),
            problem: req.problem.clone(),
            thoughts: Vec::new(),
            status: ChainStatusInternal::Thinking,
            max_steps,
            conclusion: None,
            started_at: now_ms,
            completed_at: None,
            context: req.context.clone(),
            style: req.style,
            event_tx,
        };

        self.chains.write().await.insert(chain_id.clone(), chain);

        info!(chain_id = %chain_id, "ThinkStream started");

        // Create a channel to stream events to the client
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ThoughtEvent, Status>>(64);

        tokio::spawn(async move {
            // Send initial "thinking started" event
            let initial_event = ThoughtEvent {
                chain_id: chain_id.clone(),
                step: 0,
                total_steps: max_steps,
                thought: format!("Starting thinking chain for: {}", req.problem),
                thought_type: 0, // Observation
                confidence: 0.0,
                status: ChainStatus::Thinking.into(),
                timestamp_ms: now_ms,
            };
            if tx.send(Ok(initial_event)).await.is_err() {
                return;
            }

            // Forward broadcast events to the stream
            loop {
                match event_rx.recv().await {
                    Ok(event) => {
                        let is_complete = event.status == ChainStatus::Complete as i32;
                        if tx.send(Ok(event)).await.is_err() {
                            break;
                        }
                        if is_complete {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "ThinkStream broadcast lagged");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::ThinkStreamStream))
    }

    /// Conclude a thinking chain.
    async fn conclude(
        &self,
        request: Request<ConcludeRequest>,
    ) -> Result<Response<ConcludeResponse>, Status> {
        let req = request.into_inner();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut chains = self.chains.write().await;
        let chain = chains
            .get_mut(&req.chain_id)
            .ok_or_else(|| Status::not_found(format!("Chain not found: {}", req.chain_id)))?;

        if chain.status == ChainStatusInternal::Complete {
            // Already concluded -- return existing conclusion
            return Ok(Response::new(ConcludeResponse {
                success: true,
                chain_id: req.chain_id,
                conclusion: chain.conclusion.clone().unwrap_or_default(),
                overall_confidence: chain.thoughts.iter().map(|t| t.confidence).sum::<f32>()
                    / chain.thoughts.len().max(1) as f32,
                steps_used: chain.thoughts.len() as i32,
                key_insights: chain.thoughts.iter().map(|t| t.thought.clone()).collect(),
            }));
        }

        // Check if we should force conclude
        if !req.force && chain.thoughts.is_empty() {
            return Err(Status::failed_precondition(
                "Chain has no thoughts yet. Use force=true to conclude anyway.",
            ));
        }

        // Generate conclusion from thoughts
        let conclusion = if chain.thoughts.is_empty() {
            format!("No analysis was performed for: {}", chain.problem)
        } else {
            let thought_summaries: Vec<String> = chain
                .thoughts
                .iter()
                .map(|t| format!("Step {}: {}", t.step, t.thought))
                .collect();
            format!(
                "Conclusion for '{}': After {} steps of analysis: {}",
                chain.problem,
                chain.thoughts.len(),
                thought_summaries.join("; ")
            )
        };

        let overall_confidence = if chain.thoughts.is_empty() {
            0.0
        } else {
            chain.thoughts.iter().map(|t| t.confidence).sum::<f32>() / chain.thoughts.len() as f32
        };

        let key_insights: Vec<String> = chain
            .thoughts
            .iter()
            .filter(|t| t.confidence >= 0.5)
            .map(|t| t.thought.clone())
            .collect();

        chain.status = ChainStatusInternal::Complete;
        chain.conclusion = Some(conclusion.clone());
        chain.completed_at = Some(now_ms);

        let steps_used = chain.thoughts.len() as i32;

        // Broadcast completion event
        let event = ThoughtEvent {
            chain_id: req.chain_id.clone(),
            step: steps_used,
            total_steps: chain.max_steps,
            thought: conclusion.clone(),
            thought_type: 3, // Conclusion
            confidence: overall_confidence,
            status: ChainStatus::Complete.into(),
            timestamp_ms: now_ms,
        };
        let _ = chain.event_tx.send(event);

        info!(chain_id = %req.chain_id, steps = steps_used, "Chain concluded");

        Ok(Response::new(ConcludeResponse {
            success: true,
            chain_id: req.chain_id,
            conclusion,
            overall_confidence,
            steps_used,
            key_insights,
        }))
    }

    /// Get the full state of a chain.
    async fn get_chain(
        &self,
        request: Request<GetChainRequest>,
    ) -> Result<Response<GetChainResponse>, Status> {
        let req = request.into_inner();
        let chains = self.chains.read().await;

        match chains.get(&req.chain_id) {
            Some(chain) => {
                let thoughts: Vec<ThoughtEvent> = chain
                    .thoughts
                    .iter()
                    .map(|t| ThoughtEvent {
                        chain_id: chain.id.clone(),
                        step: t.step,
                        total_steps: chain.max_steps,
                        thought: t.thought.clone(),
                        thought_type: t.thought_type,
                        confidence: t.confidence,
                        status: chain.status.to_proto(),
                        timestamp_ms: t.timestamp_ms,
                    })
                    .collect();

                Ok(Response::new(GetChainResponse {
                    found: true,
                    chain_id: chain.id.clone(),
                    problem: chain.problem.clone(),
                    thoughts,
                    status: chain.status.to_proto(),
                    conclusion: chain.conclusion.clone().unwrap_or_default(),
                    started_at_ms: chain.started_at,
                    completed_at_ms: chain.completed_at.unwrap_or(0),
                }))
            }
            None => Ok(Response::new(GetChainResponse {
                found: false,
                chain_id: req.chain_id,
                ..Default::default()
            })),
        }
    }

    /// Fork a chain at a specific step, creating a new chain with thoughts
    /// up to (and including) `fork_at_step`, plus an alternative thought.
    async fn fork_chain(
        &self,
        request: Request<ForkChainRequest>,
    ) -> Result<Response<ForkChainResponse>, Status> {
        let req = request.into_inner();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let new_chain_id = uuid::Uuid::new_v4().to_string();

        let mut chains = self.chains.write().await;
        let parent = chains
            .get(&req.chain_id)
            .ok_or_else(|| Status::not_found(format!("Chain not found: {}", req.chain_id)))?;

        // Clone thoughts up to fork_at_step
        let forked_thoughts: Vec<ThoughtEntry> = parent
            .thoughts
            .iter()
            .filter(|t| t.step <= req.fork_at_step)
            .cloned()
            .collect();

        let (event_tx, _) = broadcast::channel(64);

        let mut new_chain = ThinkingChain {
            id: new_chain_id.clone(),
            problem: parent.problem.clone(),
            thoughts: forked_thoughts,
            status: ChainStatusInternal::Thinking,
            max_steps: parent.max_steps,
            conclusion: None,
            started_at: now_ms,
            completed_at: None,
            context: parent.context.clone(),
            style: parent.style,
            event_tx,
        };

        // Add the alternative thought if provided
        if !req.alternative_thought.is_empty() {
            let step = new_chain.thoughts.len() as i32 + 1;
            new_chain.thoughts.push(ThoughtEntry {
                step,
                thought: req.alternative_thought,
                thought_type: 1, // Hypothesis
                confidence: 0.5,
                timestamp_ms: now_ms,
            });

            if step > 1 {
                new_chain.status = ChainStatusInternal::Analyzing;
            }
        }

        let forked_at = req.fork_at_step;
        let parent_id = req.chain_id.clone();

        chains.insert(new_chain_id.clone(), new_chain);

        info!(
            parent = %parent_id,
            fork = %new_chain_id,
            at_step = forked_at,
            "Chain forked"
        );

        Ok(Response::new(ForkChainResponse {
            success: true,
            new_chain_id,
            parent_chain_id: parent_id,
            forked_at_step: forked_at,
        }))
    }
}
