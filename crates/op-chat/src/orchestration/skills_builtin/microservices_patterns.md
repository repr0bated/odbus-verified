# Microservices Patterns

## Service Decomposition
- **By Business Capability**: Align services with business functions
- **By Subdomain**: Use DDD bounded contexts
- **Strangler Pattern**: Gradually replace monolith

## Communication Patterns
### Synchronous
- REST APIs for simple request/response
- gRPC for high-performance, typed communication
- GraphQL for flexible client queries

### Asynchronous
- Message queues (RabbitMQ, SQS) for decoupling
- Event streaming (Kafka) for real-time data
- Pub/Sub for broadcasting events

## Data Management
- **Database per Service**: Each service owns its data
- **Event Sourcing**: Store state as sequence of events
- **CQRS**: Separate read and write models
- **Saga Pattern**: Distributed transactions via choreography/orchestration

## Resilience Patterns
- **Circuit Breaker**: Prevent cascade failures
- **Bulkhead**: Isolate failures
- **Retry with Backoff**: Handle transient failures
- **Timeout**: Prevent indefinite waiting
- **Fallback**: Graceful degradation

## Service Discovery
- Client-side (Consul, etcd)
- Server-side (Kubernetes, load balancer)
- Service mesh (Istio, Linkerd)

## API Gateway
- Request routing and composition
- Authentication/authorization
- Rate limiting and throttling
- Response caching
