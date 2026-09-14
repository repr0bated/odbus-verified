# Distributed Tracing

## OpenTelemetry Setup
```python
from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor
from opentelemetry.exporter.jaeger.thrift import JaegerExporter

# Initialize
trace.set_tracer_provider(TracerProvider())
tracer = trace.get_tracer(__name__)

# Export to Jaeger
jaeger_exporter = JaegerExporter(
    agent_host_name="localhost",
    agent_port=6831,
)
trace.get_tracer_provider().add_span_processor(
    BatchSpanProcessor(jaeger_exporter)
)
```

## Creating Spans
```python
with tracer.start_as_current_span("operation-name") as span:
    span.set_attribute("user.id", user_id)
    span.set_attribute("http.method", "GET")
    
    # Nested span
    with tracer.start_as_current_span("database-query"):
        result = db.query(...)
    
    span.add_event("processing-complete")
```

## Context Propagation
```python
# HTTP Headers (W3C Trace Context)
traceparent: 00-{trace_id}-{span_id}-{flags}

# Inject into outgoing request
from opentelemetry.propagate import inject
headers = {}
inject(headers)
requests.get(url, headers=headers)

# Extract from incoming request
from opentelemetry.propagate import extract
context = extract(request.headers)
```

## Jaeger UI Features
- Service dependency graph
- Trace timeline view
- Compare traces
- Search by service, operation, tags

## Best Practices
1. Trace all service boundaries
2. Include meaningful span names
3. Add relevant attributes (user_id, request_id)
4. Sample appropriately in production
5. Set up trace-based alerting
6. Correlate traces with logs
