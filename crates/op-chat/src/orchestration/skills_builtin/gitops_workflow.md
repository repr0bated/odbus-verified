# GitOps Workflow

## Principles (OpenGitOps)
1. **Declarative**: Entire system described declaratively
2. **Versioned**: Desired state stored in Git
3. **Automated**: Agents pull and apply desired state
4. **Reconciled**: Continuous sync of actual vs desired

## ArgoCD Setup
```yaml
apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: my-app
  namespace: argocd
spec:
  project: default
  source:
    repoURL: https://github.com/org/gitops-repo
    targetRevision: main
    path: apps/production/my-app
  destination:
    server: https://kubernetes.default.svc
    namespace: production
  syncPolicy:
    automated:
      prune: true
      selfHeal: true
```

## Repository Structure
```
gitops-repo/
├── apps/
│   ├── production/
│   └── staging/
├── infrastructure/
│   ├── ingress/
│   └── monitoring/
└── argocd/
    └── applications/
```

## Flux CD Alternative
```yaml
apiVersion: source.toolkit.fluxcd.io/v1
kind: GitRepository
metadata:
  name: my-app
spec:
  interval: 1m
  url: https://github.com/org/my-app
  ref:
    branch: main
```

## Progressive Delivery
- Canary deployments with traffic shifting
- Blue-green deployments
- Feature flags integration

## Secret Management
- Sealed Secrets (encrypted in Git)
- External Secrets Operator
- HashiCorp Vault integration

## Best Practices
1. Separate repos for app code and config
2. Use Kustomize or Helm for templating
3. Implement approval gates for production
4. Monitor sync status and alert on failures
