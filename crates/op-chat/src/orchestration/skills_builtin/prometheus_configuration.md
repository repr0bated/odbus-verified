# Prometheus Configuration

## Metric Types
- **Counter**: Monotonically increasing (requests_total)
- **Gauge**: Can go up/down (temperature, connections)
- **Histogram**: Observations in buckets (request_duration)
- **Summary**: Quantiles over sliding window

## Instrumentation
```python
from prometheus_client import Counter, Histogram, Gauge

# Counter
requests_total = Counter(
    'http_requests_total',
    'Total HTTP requests',
    ['method', 'endpoint', 'status']
)
requests_total.labels(method='GET', endpoint='/api', status='200').inc()

# Histogram
request_duration = Histogram(
    'http_request_duration_seconds',
    'Request duration',
    buckets=[0.1, 0.5, 1.0, 2.0, 5.0]
)
with request_duration.time():
    process_request()

# Gauge
active_connections = Gauge('active_connections', 'Active connections')
active_connections.inc()
active_connections.dec()
```

## Prometheus Config
```yaml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: 'myapp'
    static_configs:
      - targets: ['localhost:8000']
    
  - job_name: 'kubernetes-pods'
    kubernetes_sd_configs:
      - role: pod
    relabel_configs:
      - source_labels: [__meta_kubernetes_pod_annotation_prometheus_io_scrape]
        action: keep
        regex: true
```

## PromQL Queries
```promql
# Request rate
rate(http_requests_total[5m])

# Error rate
sum(rate(http_requests_total{status=~"5.."}[5m])) / sum(rate(http_requests_total[5m]))

# 95th percentile latency
histogram_quantile(0.95, rate(http_request_duration_seconds_bucket[5m]))
```

## Alerting Rules
```yaml
groups:
  - name: app-alerts
    rules:
      - alert: HighErrorRate
        expr: sum(rate(http_requests_total{status=~"5.."}[5m])) / sum(rate(http_requests_total[5m])) > 0.05
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "High error rate detected"
```
