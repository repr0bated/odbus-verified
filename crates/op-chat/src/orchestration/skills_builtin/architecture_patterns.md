# Architecture Patterns

## Clean Architecture
- **Entities**: Core business objects, independent of external concerns
- **Use Cases**: Application-specific business rules
- **Interface Adapters**: Convert data between use cases and external systems
- **Frameworks & Drivers**: External tools, databases, UI

**Dependency Rule**: Dependencies point inward only.

## Hexagonal Architecture (Ports & Adapters)
- **Ports**: Interfaces defining how application interacts with outside
- **Adapters**: Implementations connecting ports to external systems
- **Core**: Business logic isolated from infrastructure

## Domain-Driven Design (DDD)
### Strategic Patterns
- **Bounded Contexts**: Clear boundaries around domain models
- **Context Maps**: Relationships between bounded contexts
- **Ubiquitous Language**: Shared vocabulary within context

### Tactical Patterns
- **Entities**: Objects with identity and lifecycle
- **Value Objects**: Immutable objects defined by attributes
- **Aggregates**: Clusters of entities with root
- **Domain Events**: Record of something that happened
- **Repositories**: Abstractions for data access
- **Domain Services**: Stateless operations

## Best Practices
1. Keep domain logic pure and framework-agnostic
2. Use dependency injection for flexibility
3. Define clear interfaces at boundaries
4. Test business logic in isolation
5. Model real domain concepts
