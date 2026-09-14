# Secrets Management

## HashiCorp Vault
```bash
# Store secret
vault kv put secret/myapp/config api_key="secret123"

# Read secret
vault kv get -field=api_key secret/myapp/config
```

### Vault Agent Injection (Kubernetes)
```yaml
annotations:
  vault.hashicorp.com/agent-inject: "true"
  vault.hashicorp.com/role: "myapp"
  vault.hashicorp.com/agent-inject-secret-config: "secret/data/myapp/config"
```

## AWS Secrets Manager
```python
import boto3

client = boto3.client('secretsmanager')

def get_secret(secret_name: str) -> dict:
    response = client.get_secret_value(SecretId=secret_name)
    return json.loads(response['SecretString'])
```

## Kubernetes Secrets
```yaml
apiVersion: v1
kind: Secret
metadata:
  name: app-secrets
type: Opaque
stringData:
  DATABASE_URL: "postgres://user:pass@host/db"
```

## External Secrets Operator
```yaml
apiVersion: external-secrets.io/v1beta1
kind: ExternalSecret
metadata:
  name: db-credentials
spec:
  refreshInterval: 1h
  secretStoreRef:
    name: vault-backend
    kind: SecretStore
  target:
    name: db-credentials
  data:
  - secretKey: password
    remoteRef:
      key: secret/data/db
      property: password
```

## Best Practices
1. Never commit secrets to Git
2. Rotate secrets regularly
3. Use dynamic secrets when possible
4. Audit secret access
5. Encrypt secrets at rest
6. Limit secret scope (least privilege)
7. Use environment-specific secrets
