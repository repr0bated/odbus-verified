# Compliance Corpora Knowledge Base

> **Generated**: 2026-04-16
> **Sources**: 6 repositories in `/home/jeremy/git/compliance_corpora/`
> **Purpose**: Exhaustive compliance knowledge reference for AI governance, data protection, cloud security, and organizational security policies.

---

## Table of Contents

1. [EU AI Act Compliance Layer](#1-eu-ai-act-compliance-layer)
2. [compl-ai LLM Benchmark Framework](#2-compl-ai-llm-benchmark-framework)
3. [GDPR Developer Guide (CNIL)](#3-gdpr-developer-guide-cnil)
4. [NIST 800-53 Rego Policies](#4-nist-800-53-rego-policies)
5. [Security Policy Templates + Controls Mapping](#5-security-policy-templates--controls-mapping)
6. [TechOps Documentation Templates](#6-techops-documentation-templates)
7. [Cross-Corpus Analysis](#7-cross-corpus-analysis)

---

## 1. EU AI Act Compliance Layer

**Source**: `compliance_corpora/eu-ai-act-layer-lite/`
**Version**: 3.6.0-lite (Free Technical Baseline License)
**Provider**: X-Loop3 Labs (Gossau, St. Gallen, Switzerland)

### 1.1 Complete JSON Schema

The EU AI Act Layer Lite is a structured governance baseline for AI systems. It provides:

#### Module Identification
- **module_id**: `EU-AI-ACT-LAYER-LITE`
- **version**: `3.6.0-lite`
- **status**: `released`
- **tier**: `lite`
- **delivery_model**: `artifacts_only`

#### Positioning
- 100% Free, implementation-ready governance baseline
- Translates regulatory requirements into concrete engineering artifacts, schemas, and checklists
- Not legal advice, not compliance certification, not support/consulting/managed services

#### Intended Use
- **Target system classes**: High-risk Annex III systems, General-purpose AI with EU exposure, Startups/SMEs requiring baseline structure
- **NOT for**: Runtime enforcement, Automated regulatory submission, Sovereign/national security use

#### Core Principles
1. Explicit system purpose and boundaries before operation
2. Prohibited practices must be screened before deployment
3. Risks must be documented with mitigations
4. Human oversight paths must be defined for high-severity risks
5. Evidence must be structured, not narrative

#### Required Artifacts
| Artifact | Status | Description |
|----------|--------|-------------|
| `SYSTEM_PROFILE` | **Required** | Intended purpose, boundaries, affected groups |
| `ANNEX_III_MAPPING` | **Required** | Risk classification mapping (all 8 categories) |
| `PROHIBITED_PRACTICES_SCREEN` | **Required** | Article 5 checks (5 prohibited practices) |
| `RISK_REGISTER` | **Required** | Severity, mitigation, ownership |
| `HUMAN_OVERSIGHT_SCHEMA` | **Required** | Oversight mode, override mechanisms, escalation |

#### Optional Artifacts
| Artifact | Status |
|----------|--------|
| `DATA_LINEAGE` | Optional |
| `MODEL_CARD` | Optional |
| `TEST_REPORTS` | Optional |
| `POST_MARKET_MONITORING` | Optional |
| `CHANGE_LOG` | Optional |
| `INCIDENT_REPORT` | Optional |

#### Annex III Risk Categories (8 Total)
1. `1_biometric_identification`
2. `2_critical_infrastructure`
3. `3_education_training`
4. `4_employment`
5. `5_essential_services`
6. `6_law_enforcement`
7. `7_migration_asylum`
8. `8_judicial_democratic`

#### Controlled Vocabulary
- **Mandatory terms**: Intent, Risk, Human Oversight, Incident
- **Normalizations**: HITL → Human Oversight, human-in-the-loop → Human Oversight, post deployment monitoring → Post-market Monitoring

#### Gating States
- `PASS` / `FAIL`
- Lite provides readiness signals only — no hard gating
- No human review required at this tier

#### Prohibited Practices Screen (Article 5)
| Check | Statement |
|-------|-----------|
| PP-01 | No subliminal or manipulative techniques |
| PP-02 | No exploitation of vulnerable groups |
| PP-03 | No social scoring |
| PP-04 | No emotion recognition in workplace/education |
| PP-05 | No behavior distortion causing harm |

### 1.2 Example Session (MedAssist AI)

The session.json provides a complete worked example:
- **System**: MedAssist AI (Healthcare diagnostic support)
- **Classification**: High-Risk (Annex III, category 8)
- **Deployment**: Cloud (EU: DE, FR, NL)
- **Affected Groups**: Radiologists, referring physicians, patients (indirect)
- **Human Oversight Mode**: Human-in-the-loop (physician confirmation required)
- **Top Risks**: Incorrect diagnostic suggestion (Critical), Model drift (High), Privacy breach via image logging (High), Demographic bias (High), System unavailability (Medium)
- **Governance Readiness**: 5/5 artifacts completed → PASS

### 1.3 Tier Structure
| Tier | Features | Price |
|------|----------|-------|
| **Lite** | Governance baseline, templates, checklists | Free |
| **Lite + Text Addon** | Structured evidence exports, PDF-ready HTML | Free |
| **Pro** | Consistency engine, HERM-Light, multi-stage gating, trade-off governance | Commercial |
| **Enterprise** | Full HERM-CORE, evidence bundles, audit index, hash-chain integrity | Commercial |
| **Black Tier** | Constitutional invariants, multi-party governance, ZK proof interface | Invite-only |

### 1.4 Technical Implementation Requirements

To implement EU AI Act compliance technically, organizations need:

1. **System profiling engine** — Capture intended purpose, deployment context, geographic scope, affected user groups
2. **Annex III classifier** — Map system to one of 8 risk categories with evidence
3. **Prohibited practices validator** — Automated/manual screening for Article 5 violations
4. **Risk register** — Structured repository with severity levels (critical/high/medium/low), mitigations, ownership, status tracking
5. **Human oversight configuration** — Mode selection (HITL/HOTL/HIC), override mechanisms, escalation paths
6. **Evidence management** — Structured data (not narrative), completion tracking, export capabilities
7. **GDPR alignment checklist** — Cross-reference with data protection requirements
8. **Conformity assessment guidance** — Preparation steps for notified body review

---

## 2. compl-ai LLM Benchmark Framework

**Source**: `compliance_corpora/compl-ai/`
**Version**: 2.0.0
**Providers**: ETH Zurich, INSAIT, LatticeFlow AI
**License**: Apache 2.0
**Paper**: arXiv:2410.07959

### 2.1 Overview

COMPL-AI is a compliance-centered evaluation framework for LLMs, providing:
- Technical interpretation of the EU AI Act
- Open-source benchmarking suite (29+ benchmarks)
- Built on UK Government BEIS Inspect evaluation framework
- Public Hugging Face leaderboard
- Custom CLI tool (`complai`)

### 2.2 Six Core EU AI Act Principles

The framework maps benchmarks to 6 core principles:

| # | Principle | Description |
|---|-----------|-------------|
| 1 | **Human Agency and Oversight** | AI systems supervised by people, not automation alone |
| 2 | **Technical Robustness and Safety** | Safe, secure systems with risk management and cybersecurity |
| 3 | **Privacy and Data Governance** | Quality/governance of data, protection of personal/sensitive info |
| 4 | **Transparency** | Users understand AI interactions and system function |
| 5 | **Diversity, Non-Discrimination, Fairness** | Uphold human rights, avoid discriminatory biases |
| 6 | **Societal and Environmental Well-being** | Benefit society, avoid negative impacts on rights/values |

### 2.3 Technical Requirements and Benchmark Mapping

| Technical Requirement | Benchmarks | EU AI Act Principle |
|----------------------|------------|---------------------|
| **Capabilities, Performance, and Limitations** | aime_2025, arc_challenge, gpqa_diamond, hle, ifbench, include, livebench_coding, mmlu_pro, swe_bench_verified | Human Agency & Oversight |
| **Representation — Absence of Bias** | bbq, bold, cab | Diversity, Non-Discrimination, Fairness |
| **Interpretability** | bigbench_calibration, triviaqa_calibration | Transparency |
| **Robustness and Predictability** | boolq_contrast, forecast_consistency, imdb_contrast, mmlu_pro_robustness, self_check_consistency | Technical Robustness & Safety |
| **Fairness — Absence of Discrimination** | decoding_trust, fairllm | Diversity, Non-Discrimination, Fairness |
| **Disclosure of AI** | human_deception | Transparency |
| **Cyberattack Resilience** | instruction_goal_hijacking, llm_rules, strong_reject | Technical Robustness & Safety |
| **Societal Alignment** | mask, simpleqa_verified, truthfulqa | Societal & Environmental Well-being |
| **Harmful Content and Toxicity** | realtoxicityprompts | Societal & Environmental Well-being |

### 2.4 All 31 Registered Benchmarks

1. **aime_2025** — Math competition (4 epochs)
2. **arc_challenge** — AI2 Reasoning Challenge
3. **bbq** — Bias Benchmark for QA (subsets, shuffle options)
4. **bigbench_calibration** — Calibration via BIG-Bench tasks (emoji_movie, 3-shot)
5. **bold** — Bias in Open-ended Language Generation (GPU scorer)
6. **boolq_contrast** — Boolean QA with contrastive examples (3 contrasts)
7. **cab** — Contextual Attribution Bias (judge: gpt-5-mini, attributes: gender/race/religion)
8. **decoding_trust** — DecodingTrust trustworthiness benchmark
9. **fairllm** — Fairness evaluation (200 samples, 20 recommendations)
10. **forecast_consistency** — Prediction consistency
11. **gpqa_diamond** — Graduate-level QA (4 epochs)
12. **hle** — Hard Language Evaluation (grader: o3-mini, text-only, 8192 tokens)
13. **human_deception** — AI disclosure detection
14. **ifbench** — Instruction Following benchmark
15. **imdb_contrast** — IMDB sentiment with contrastive sets
16. **include** — Inclusive evaluation
17. **instruction_goal_hijacking** — Prompt injection resistance (multiple_user strategy)
18. **livebench_coding** — Live coding evaluation
19. **llm_rules** — Rule-following evaluation (basic category)
20. **mask** — Moral reasoning evaluation (binary+numeric judges)
21. **mmlu_pro** — MMLU Professional (0-shot)
22. **mmlu_pro_robustness** — Robustness perturbations (typos, synonyms, spaces, paraphrase, misspelling, lowercase, gender, filler, dialect, contraction)
23. **mmmu_pro** — Multimodal understanding (standard_10 subset)
24. **realtoxicityprompts** — Toxicity generation evaluation (GPU scorer)
25. **self_check_consistency** — Self-consistency checking (argumentation+judge models)
26. **simpleqa_verified** — Verified simple QA (grader: gpt-4.1)
27. **strong_reject** — Refusal strength evaluation (all jailbreak methods)
28. **swe_bench_verified** — Software engineering (200 message limit)
29. **triviaqa_calibration** — TriviaQA calibration (rc.wikipedia subset)
30. **truthfulqa** — Truthfulness evaluation (mc2 target)
31. **mmmu_pro** — Multimodal reasoning

### 2.5 Configuration Structure

Configuration via YAML files (`default_config.yaml`):
```yaml
# Example task configuration
mmlu_pro:
  num_fewshot: 0
bbq:
  subsets: null
  shuffle: false
cab:
  judge_model: openai/gpt-5-mini-2025-08-07
  num_responses: 3
  attributes: [gender, race, religion]
strong_reject:
  full: false
  jailbreak_methods: all
  judge: openai/gpt-4o-mini
```

### 2.6 How Benchmarks Are Run and Scored

```bash
# Install and run
uv sync && source .venv/bin/activate
export OPENAI_API_KEY=your_key

# Run specific benchmark with limit
complai eval openai/gpt-5-nano --tasks mmlu_pro --limit 5

# Run full evaluation suite
complai eval openai/gpt-5-nano

# List all available tasks and their requirements
complai list

# Use task configuration file
complai eval openai/gpt-5-nano --task-config default_config.yaml

# View results
inspect view
```

**Supported providers**: OpenAI API, Anthropic API, HuggingFace (local), vLLM (local)

### 2.7 Dependencies
- `inspect-ai` + `inspect-evals` (UK Gov BEIS framework)
- `torch`, `numpy`, `scipy` (ML computing)
- `detoxify` (toxicity scoring)
- `fairlearn` (fairness metrics)
- `spacy` + `en-core-web-sm` (NLP)
- `strong-reject`, `llm-rules` (security benchmarks)
- `polars`, `datasets` (data handling)

---

## 3. GDPR Developer Guide (CNIL)

**Source**: `compliance_corpora/GDPR-Developer-Guide/`
**Publisher**: CNIL (Commission Nationale de l'Informatique et des Libertés — French Data Protection Authority)
**License**: GPLv3 / Open License 2.0 (CC-BY 4.0 compatible)
**Format**: 17 thematic sheets (00-16)

### 3.1 Complete Sheet Summaries with Extractable Compliance Rules

---

#### Sheet 0: Develop in Compliance with the GDPR
**Category**: Program Management

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-0.1 | Identify a person responsible for monitoring compliance (DPO if applicable) | Assign DPO role in org chart; implement DPO contact mechanism |
| GDPR-0.2 | Map and categorize all data and processing | Maintain a processing activities registry (structured data inventory) |
| GDPR-0.3 | Prioritize required actions based on risk | Implement risk scoring for each processing operation |
| GDPR-0.4 | Conduct Privacy Impact Assessment for high-risk processing | PIA tool integration; document risk/mitigation pairs |
| GDPR-0.5 | Put in place internal compliance processes | Automated workflows for security breaches, access requests, data changes |
| GDPR-0.6 | Document development compliance continuously | Version-controlled compliance documentation alongside code |

---

#### Sheet 1: Identify Personal Data
**Category**: Data Classification

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-1.1 | Identify all personal data in the system | Data discovery/scanning tools; tag all PII fields |
| GDPR-1.2 | Recognize direct identifiers (name, email, phone, IP, cookie IDs, biometric data) | PII detection regex/ML classifiers in data pipelines |
| GDPR-1.3 | Recognize indirect identifiers (combinations that can identify) | Cross-reference analysis; linkability assessment |
| GDPR-1.4 | Identify sensitive data categories (health, sexual orientation, racial origin, political opinions, genetic/biometric) | Sensitive data classifier; special handling flags |
| GDPR-1.5 | Distinguish anonymization (irreversible) from pseudonymization (reversible) | Implement proper anonymization techniques preventing singling out, linkability, and inference |
| GDPR-1.6 | Pseudonymized data remains personal data and subject to GDPR | Maintain GDPR controls on pseudonymized datasets |

---

#### Sheet 2: Prepare Your Development
**Category**: Privacy by Design

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-2.1 | Adopt Privacy By Design methodology | Embed privacy checks in CI/CD pipeline |
| GDPR-2.2 | Integrate security in agile processes | Security stories in sprints; threat modeling in design phases |
| GDPR-2.3 | Consider privacy settings defaults | Default to most restrictive privacy settings |
| GDPR-2.4 | Conduct PIA for applicable processing | Integrate PIA tooling into project lifecycle |
| GDPR-2.5 | Maintain control of system complexity | Start simple, add complexity incrementally with security |
| GDPR-2.6 | Don't rely on single line of defense | Defense-in-depth: input validation + database protection + output encoding |
| GDPR-2.7 | Use security-aware programming standards | IDE plugins for CERT coding standards; OWASP integration |

---

#### Sheet 3: Secure Your Development Environment
**Category**: Environment Security

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-3.1 | Assess risks on all development tools including SaaS | Tool inventory with risk ratings; evaluate Slack, GitHub, Trello security |
| GDPR-3.2 | Secure servers and workstations uniformly | Configuration management (Ansible/Puppet/Chef); documented security baselines |
| GDPR-3.3 | Update systems automatically | Automated patching; NVD vulnerability feed monitoring |
| GDPR-3.4 | Manage SSH keys properly | State-of-art crypto, passphrase protection, key rotation |
| GDPR-3.5 | Use strong authentication for development services | MFA on all dev tools and services |
| GDPR-3.6 | Trace access and analyze logs | Centralized logging; avoid generic accounts; automated log analysis |

---

#### Sheet 4: Manage Your Source Code
**Category**: Source Code Management

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-4.1 | Use version control with strong authentication | Git/Mercurial with SSH keys or strong auth |
| GDPR-4.2 | Define access levels and permissions | Role-based access (guest/developer/admin) |
| GDPR-4.3 | Make regular backups of source code | Automated backup of main/central repository |
| GDPR-4.4 | Keep secrets out of source code | Use .gitignore, environment variables, secret management (git-crypt) |
| GDPR-4.5 | Purge repository after committing sensitive data | git filter-branch or BFG to rewrite history |
| GDPR-4.6 | Review entire content before publishing source code | Pre-publish audit for PII, passwords, secrets in history |
| GDPR-4.7 | Implement code quality metrics | Pre-commit hooks; quality gate checks on commit |

---

#### Sheet 5: Make an Informed Choice of Architecture
**Category**: Architecture & Data Lifecycle

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-5.1 | Map data flows and lifecycle from collection to erasure | Data flow diagrams; lifecycle state machine |
| GDPR-5.2 | For local storage, focus on security and let users control retention | Client-side encryption; user-controlled deletion |
| GDPR-5.3 | Choose hosting provider with appropriate security and transparency | Vendor security assessment; DPA/BAA contracts |
| GDPR-5.4 | Know geographic location of data servers | Data residency tracking; EU/EEA compliance |
| GDPR-5.5 | Ensure adequate protection for cross-border transfers | Standard Contractual Clauses; adequacy decisions |
| GDPR-5.6 | Health data requires certified/approved hosting | HDS certification verification |

---

#### Sheet 6: Secure Your Websites, Applications and Servers
**Category**: Technical Security

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-6.1 | Implement TLS 1.2+ on all websites and data transmissions | LetsEncrypt certificates; TLS configuration audit |
| GDPR-6.2 | Make TLS mandatory for all pages | HSTS headers; redirect HTTP→HTTPS |
| GDPR-6.3 | Limit communication ports strictly | Firewall rules: only 443/80 for web servers |
| GDPR-6.4 | Follow CNIL password recommendations | Minimum length/complexity; limit login attempts |
| GDPR-6.5 | Never store passwords in clear text | bcrypt hashing with proven libraries |
| GDPR-6.6 | Secure cookies (secure flag, HttpOnly, HSTS) | Cookie security attributes on all auth cookies |
| GDPR-6.7 | Disable obsolete cryptographic suites | No RC4, MD4, MD5; prefer AES256 |
| GDPR-6.8 | Specific admin password policy (10-20 chars or MFA) | Rotate admin passwords; limit admin account knowledge |
| GDPR-6.9 | Secure remote admin access (VPN + strong auth) | VPN with MFA for admin interfaces |
| GDPR-6.10 | Encrypted backups checked regularly | Automated backup verification; ransomware protection |
| GDPR-6.11 | Vulnerability detection and automated patching | NVD feeds; vulnerability scanners; IDS/IPS |
| GDPR-6.12 | Protect databases (IP filtering, change default passwords) | DB access control; nominative accounts; SQL injection protection |

---

#### Sheet 7: Minimize Data Collection
**Category**: Data Minimization

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-7.1 | Document data types needed before implementation | Data requirements specification; justify each field |
| GDPR-7.2 | Don't collect data not needed for specific user categories | Conditional data collection forms |
| GDPR-7.3 | Reduce data accuracy where possible (pseudonymization) | Store year-of-birth instead of full date where sufficient |
| GDPR-7.4 | Collect minimum required sensitive data (ideally none) | Sensitive data flag with justification requirements |
| GDPR-7.5 | Minimize log data; no sensitive data in logs | Log sanitization pipeline; structured logging with PII filters |
| GDPR-7.6 | Optional features require explicit user choice | Opt-in toggles for geolocation, personalization features |
| GDPR-7.7 | Associate retention periods with each data category | Retention policy metadata on every data category |
| GDPR-7.8 | Implement automatic purge at end of retention | Scheduled deletion jobs; physical data erasure tools |
| GDPR-7.9 | Log automatic deletion procedures for proof | Deletion audit trail |

---

#### Sheet 8: Manage User Profiles
**Category**: Access Control

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-8.1 | Use unique and individual identifiers | No shared accounts; unique user IDs |
| GDPR-8.2 | Impose authentication before any personal data access | Auth middleware on all PII endpoints |
| GDPR-8.3 | Implement differentiated access management (RBAC) | Role-based permissions (read/write/delete per role) |
| GDPR-8.4 | Use logging for activity tracing and anomaly detection | Access audit logs; 6-month retention |
| GDPR-8.5 | Document and automate personnel access changes | LDAP integration; automated de-provisioning on role change |
| GDPR-8.6 | Regular permission review | Quarterly access review; principle of least privilege |
| GDPR-8.7 | Restrict root/admin account usage | Strong passwords (10-20 chars + MFA); minimal admin knowledge |
| GDPR-8.8 | Use password managers and strong authentication | Team password manager; MFA everywhere possible |

---

#### Sheet 9: Control Your Libraries and SDKs
**Category**: Third-Party Components

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-9.1 | Assess value of each dependency; minimize attack surface | Dependency audit; remove unused deps |
| GDPR-9.2 | Choose maintained software with active communities | Dependency health scoring (update frequency, CVE response time) |
| GDPR-9.3 | Verify SDKs don't secretly collect/sell personal data | Privacy audit of third-party SDKs; consent mechanisms |
| GDPR-9.4 | Use recognized crypto libraries, never roll your own | Approved crypto library list |
| GDPR-9.5 | Read documentation and change default configs | Security config review checklist |
| GDPR-9.6 | Audit libraries for data flows and responsibilities | Dependency data flow mapping |
| GDPR-9.7 | Map all dependencies including transitive ones | Software Bill of Materials (SBOM); dependency tree analysis |
| GDPR-9.8 | Watch for typosquatting attacks | Package name verification; lockfile integrity checks |
| GDPR-9.9 | Use dependency management and update systems | Dependabot/Renovate; documented update procedures |

---

#### Sheet 10: Ensure Quality of Code and Documentation
**Category**: Code Quality

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-10.1 | Document architecture, not just code | Architecture diagrams; component interaction docs |
| GDPR-10.2 | Maintain documentation with code changes | Doc updates in same PR/commit as code changes |
| GDPR-10.3 | Document security configurations | Security config reference in project docs |
| GDPR-10.4 | Adopt coding conventions consistently | Linter configurations; style guides |
| GDPR-10.5 | Use explicit variable/function names | Naming conventions; code review standards |
| GDPR-10.6 | Use code quality measurement tools | SonarQube/similar; IDE plugins for conventions |

---

#### Sheet 11: Test Your Applications
**Category**: Testing

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-11.1 | Run both development and security tests | Unit/functional tests + fuzzing + vulnerability scans |
| GDPR-11.2 | Set up continuous integration for automated testing | CI pipeline running tests on every commit |
| GDPR-11.3 | Define acceptable test metrics jointly | Coverage targets, duplication limits, vulnerability thresholds |
| GDPR-11.4 | Never use real production data in testing | Synthetic/dummy data generation |
| GDPR-11.5 | If importing production configs, anonymize personal data | Data anonymization pipeline for test environments |

---

#### Sheet 12: Inform Users
**Category**: Transparency & Notification

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-12.1 | Inform during direct data collection | In-context privacy notices on forms/collection points |
| GDPR-12.2 | Inform ASAP for indirect collection (max 1 month) | Automated notification system for indirect data acquisition |
| GDPR-12.3 | Inform on substantial changes or events | Change notification system; breach notification pipeline |
| GDPR-12.4 | Include: identity, purposes, lawful basis, data categories, recipients, retention, rights, DPO contact, complaint right | Structured privacy notice template with all required fields |
| GDPR-12.5 | Information must be easy to access, clear, concise, distinguishable | UX review of privacy notices; layered approach |
| GDPR-12.6 | Report data breaches to authority within 72 hours | Incident response system with 72-hour timer; automated CNIL notification |
| GDPR-12.7 | Notify affected individuals of high-risk breaches | Mass notification system; breach remediation guidance |

---

#### Sheet 13: Prepare for Exercise of People's Rights
**Category**: Data Subject Rights

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-13.1 | Indicate where/how individuals can exercise rights | Visible rights exercise channel (email, web form, privacy policy) |
| GDPR-13.2 | **Right of access**: Provide copy of all personal data | Data export function (display or downloadable archive) |
| GDPR-13.3 | **Right to erasure**: Delete all data on request | Delete function + notify processors + handle backups |
| GDPR-13.4 | **Right to object**: Stop processing for specific purpose | Object function; cease data collection for that person |
| GDPR-13.5 | **Right to data portability**: Machine-readable export | Export in CSV/XML/JSON format |
| GDPR-13.6 | **Right to rectification**: Allow data modification | Edit function in user account |
| GDPR-13.7 | **Right to restriction**: Quarantine data temporarily | Admin quarantine function; data frozen from read/write |
| GDPR-13.8 | Manage authentication securely for rights exercise | Identity verification before fulfilling requests |
| GDPR-13.9 | Trace all operations impacting personal data | Comprehensive audit trail |

---

#### Sheet 14: Define a Data Retention Period
**Category**: Data Lifecycle

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-14.1 | Define 3-phase retention: active → intermediate archive → final archive/deletion | Database partitioning by lifecycle phase |
| GDPR-14.2 | Active database: only keep for purpose duration | TTL/expiry fields on records |
| GDPR-14.3 | Archived data: restricted access to specific service | Separate archive storage with limited access |
| GDPR-14.4 | Align purging with right to erasure implementation | Shared deletion mechanism |
| GDPR-14.5 | Reference retention periods: payroll 5yr, medical 20yr, prospect 3yr, logs 6mo | Configurable retention policies per data category |

---

#### Sheet 15: Take into Account the Legal Basis
**Category**: Legal Basis Implementation

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-15.1 | Choose one legal basis per purpose (Contract, Legitimate Interest, Consent, Legal Obligation, Public Interest, Vital Interests) | Legal basis metadata on each processing operation |
| GDPR-15.2 | Implement rights matrix per legal basis | Rights availability engine based on legal basis |
| GDPR-15.3 | Consent requires active, explicit, free, specific, informed mechanism | Consent management platform; granular opt-in |
| GDPR-15.4 | Legitimate interest requires documented interest and balancing test | LIA documentation system |
| GDPR-15.5 | Document legal basis choice with processing | Audit trail linking processing to legal basis |
| GDPR-15.6 | Cookie consent required (except strictly necessary cookies) | Cookie consent banner with granular controls |

**Rights Matrix by Legal Basis:**

| Legal Basis | Access | Rectification | Erasure | Restriction | Portability | Object |
|-------------|--------|---------------|---------|-------------|-------------|--------|
| Consent | ✓ | ✓ | ✓ | ✓ | ✓ | Withdraw consent |
| Contract | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ |
| Legitimate Interest | ✓ | ✓ | ✓ | ✓ | ✗ | ✓ |
| Legal Obligation | ✓ | ✓ | ✗ | ✓ | ✗ | ✗ |
| Public Interest | ✓ | ✓ | ✗ | ✓ | ✗ | ✓ |
| Vital Interests | ✓ | ✓ | ✓ | ✓ | ✗ | ✗ |

---

#### Sheet 16: Use Analytics on Your Websites and Applications
**Category**: Analytics & Tracking

| Rule ID | Description | Technical Implementation |
|---------|-------------|------------------------|
| GDPR-16.1 | Obtain consent before cookies/tracers (general rule) | Consent management platform before any tracking |
| GDPR-16.2 | Exempt from consent if: audience measurement only, A/B testing only, no cross-processing, single site scope, truncated IP, 13-month lifetime | Cookie audit tool; exemption validation |
| GDPR-16.3 | Most large analytics offerings do NOT qualify for exemption | Self-hosted analytics (e.g., Matomo) for consent-free tracking |

### 3.2 Templates Structure

The GDPR Developer Guide includes:
- `templates/mytemplate.html` — Pandoc HTML template for rendering
- `templates/pandoc.css` — CSS styling for rendered output
- `templates/BANNIERE-EN.JPG` — CNIL banner image

Build commands:
```bash
# Generate .docx
pandoc -s --toc --toc-depth=1 -o GDPR_developer_guide.docx [0-9][0-9]*.md

# Generate .html
pandoc -s --template="templates/mytemplate.html" -H templates/pandoc.css -o index.html README.md [0-9][0-9]*.md
```

---

## 4. NIST 800-53 Rego Policies

**Source**: `compliance_corpora/rego-cns/`
**Providers covered**: AWS CloudFormation, AWS Terraform, GCP Terraform
**Policy Language**: Open Policy Agent (OPA) Rego

### 4.1 Policy Structure and Pattern

All Rego policies follow this pattern:

```rego
package <provider>_<resource_type>

# Default deny
default allow = false

# Allow if specific conditions are met
allow {
    resource := input.Resources[_]    # or input.resource.<type>[_]
    <type_check>(resource)
    <compliance_check>(resource)
}

# Deny message with reference URL
deny_message[msg] {
    not allow
    msg := "<documentation_url>"
}
```

**Key concepts:**
1. **Input**: JSON representation of infrastructure-as-code (CloudFormation templates or Terraform plans)
2. **Evaluation**: Policies evaluate resource properties against NIST 800-53 control requirements
3. **Output**: Pass/Fail with reference documentation URLs
4. **Composability**: Each policy targets a specific control; policies can be composed for full compliance checks

### 4.2 AWS CloudFormation NIST 800-53 Policies (26 policies)

#### Representative Policies

**CloudTrail Encryption (SC-28 Data at Rest)**
```rego
package aws.cfn.cloudTrailEncryptionEnabled
default allow = false
allow {
    resource := input.Resources[_]
    resource.Type == "AWS::CloudTrail::Trail"
    resource.Properties["KMSKeyId"]
}
```
*Ensures CloudTrail is encrypted with KMS key.*

**IAM Password Policy — Minimum Length (IA-5 Authenticator Management)**
```rego
package aws.cfn.iamPasswordPolicyMinimumPasswordLength
min_password_length = 12
allow { iamPasswordPolicyMinimumPasswordLength }
iamPasswordPolicyMinimumPasswordLength[msg] {
    resource := input.Resources[_]
    resource.Type == "AWS::IAM::AccountPasswordPolicy"
    resource.Properties.MinimumPasswordLength == min_password_length
}
```
*Enforces minimum 12-character passwords.*

**S3 Public Access Prohibition (AC-3 Access Enforcement)**
```rego
package aws.cfn.s3BucketLevelPublicAccessProhibited
allow { s3BucketLevelPublicAccessProhibited }
s3BucketLevelPublicAccessProhibited[msg] {
    resource := input.Resources[_]
    resource.Type == "AWS::S3::Bucket"
    resource.Properties.BlockPublicAcls == true
    resource.Properties.IgnorePublicAcls == true
    resource.Properties.BlockPublicPolicy == true
    resource.Properties.RestrictPublicBuckets == true
}
```
*Requires all four S3 public access blocks enabled.*

**EC2 No Public IP (SC-7 Boundary Protection)**
```rego
package aws.cfn.ec2InstanceNoPublicIp
allow {
    resource := input.Resources[_]
    resource.Type == "AWS::EC2::Instance"
    resource.Properties.NetworkInterfaces[_].AssociatePublicIpAddress == false
}
```

**Security Group Restricted SSH (AC-17 Remote Access)**
```rego
package aws.cfn.secgroupRestrictedSsh
allow {
    resource := input.Resources[_]
    resource.Type == "AWS::EC2::SecurityGroup"
    resource.Properties.SecurityGroupIngress[_].FromPort == 22
    resource.Properties.SecurityGroupIngress[_].CidrIp != "0.0.0.0/0"
}
```

#### Full AWS CloudFormation Policy List
| Policy | NIST Control Family |
|--------|-------------------|
| api_gwcache_encrypted | SC (System & Communications Protection) |
| cloudtrail_encryption_enabled | SC-28 |
| cloudwatch_loggroup_encrypted | SC-28 |
| dynamodb_table_encrypted_using_kms | SC-28 |
| ebs_volume_delete_on_termination | MP (Media Protection) |
| ec2_ebs_encryption_bydefault | SC-28 |
| ec2_instance_nopublic_ip | SC-7 |
| efs_encrypted | SC-28 |
| elasticsearch_encrypted_atrest | SC-28 |
| elb_deletion_protection_enabled | CP (Contingency Planning) |
| iam_passwordpolicy_maxpassword_age | IA-5 |
| iam_passwordpolicy_minimum_passwordlength | IA-5 |
| iam_passwordpolicy_password_reuse_prevention | IA-5 |
| iam_passwordpolicy_require_lowercasecharacters | IA-5 |
| iam_passwordpolicy_require_numbers | IA-5 |
| iam_passwordpolicy_require_symbols | IA-5 |
| iam_passwordpolicy_require_uppercasecharacters | IA-5 |
| maxaccesskeyage | IA-5 |
| mfa_enabled_iamconsole_access | IA-2 |
| rds_instance_public_accesscheck | SC-7 |
| redshift_cluster_public_accesscheck | SC-7 |
| rotation_customer_created_cmks_enabled | SC-12 |
| s3bucket_level_public_access_prohibited | AC-3 |
| s3bucket_level_public_access_prohibited_singlebucket | AC-3 |
| sagemaker_notebook_instance_kmskeyconfigured | SC-28 |
| security_group_restricted_ssh | AC-17 |

### 4.3 AWS Terraform NIST 800-53 Policies (17 policies)

#### Representative Policies

**S3 Server-Side Encryption (SC-28)**
```rego
package AWS_Terraform_aws_security_s3_bucket_server_side_encryption_rule
deny { not(aws_security_s3_bucket_server_side_encryption_rule) }
aws_security_s3_bucket_server_side_encryption_rule[msg] {
    check_rule = input.resource.aws_s3_bucket_server_side_encryption_configuration[_]
    not check_rule.rule
    msg := "Ensure the rule for S3 buckets is not empty and has the Amazon S3 default encryption enabled"
}
```

**IAM No Wildcards (AC-6 Least Privilege)**
```rego
package AWS_Terraform_
deny { (aws_security_iam_no_wildcards_policies) }
resource_wildcard[msg] {
    policy := input.resource.aws_iam_policy.policy.policy
    statement := policy.Statement[_]
    statement.Resource == "*"
    msg := "AWS IAM policy contains a statement with Resource=*"
}
```

**EC2 IMDSv2 Required (IA-2)**
```rego
package AWS_Terraform_aws_security_ec2_imdsv2
deny { not (aws_security_ec2_imdsv2) }
aws_security_ec2_imdsv2[msg] {
    input.resource.aws_instance[_].metadata_options.http_tokens == "optional"
    input.resource.aws_instance[_].metadata_options.http_endpoint == "enabled"
    msg := "Ensure IMDSv2 with session authentication tokens is active"
}
```

#### Full AWS Terraform Policy List
| Policy | NIST Control Family |
|--------|-------------------|
| api_gw_cache_enabled_and_encrypted | SC |
| api_gw_execution_logging_enabled | AU |
| api_gw_require_authentication | IA |
| api_gw_tls | SC |
| api_gw_xray_enabled | AU |
| ebs_default_encrypt | SC-28 |
| ec2_imdsv2 | IA-2 |
| ec2_user_data_no_secrets | IA-5 |
| eks_encrypt_secrets | SC-28 |
| eks_no_public_cluster | SC-7 |
| iam_no_wildcards_policies | AC-6 |
| no_static_credentials_in_providers | IA-5 |
| s3_bucket_level_public_access_prohibited | AC-3 |
| s3_bucket_logging_enabled | AU-2 |
| s3_bucket_public_read_and_write_prohibited | AC-3 |
| s3_bucket_server_side_encryption_rule | SC-28 |
| s3_bucket_versioning_enabled | CP-9 |

### 4.4 GCP Terraform NIST 800-53 Policies (34 policies)

#### Representative Policies

**GKE No Basic Authentication (IA-2)**
```rego
package GCP_Terraform_gcp_security_gke_no_basic_authentication
deny { not(gcp_security_gke_no_basic_authentication) }
gcp_security_gke_no_basic_authentication[msg] {
    input.resource.google_container_cluster[_].master_auth.username == ""
    msg := "Username must not be empty"
}
```

**Compute Shielded VM (SI-7 Software/Firmware Integrity)**
```rego
package GCP_terraform_gcp_security_compute_enable_shielded_vm
deny { not(gcp_security_compute_enable_shielded_vm) }
shielded_instance_config[msg] {
    shielded_config := input.resource.google_compute_instance[_].shielded_instance_config
    shielded_config.enable_vtpm == false
    shielded_config.enable_integrity_monitoring == false
    msg := "enable_vtpm and enable_integrity_monitoring must be true"
}
```

**Storage No Public Access (AC-3)**
```rego
package GCP_Terraform_gcp_security_storage_no_public_access
deny { not(gcp_security_storage_no_public_access) }
public_access[msg] {
    input.resource.google_storage_bucket_iam_binding[_].members[_] == "allAuthenticatedUsers"
    msg := "Ensure Cloud Storage bucket is not publicly accessible"
}
```

#### Full GCP Terraform Policy List
| Policy | NIST Control Family |
|--------|-------------------|
| bigquery_no_public_access | AC-3 |
| check_google_container_cluster_node_config | CM-6 |
| compute_disk_encryption_customer_key | SC-28 |
| compute_disk_encryption_required | SC-28 |
| compute_enable_shielded_vm | SI-7 |
| compute_enable_vpc_flow_logs | AU-2 |
| compute_no_default_service_account | AC-6 |
| compute_no_ip_forwarding | SC-7 |
| compute_no_plaintext_vm_disk_keys | SC-28 |
| compute_no_public_ip | SC-7 |
| dns_enable_dnssec | SC-20 |
| dns_key_check | SC-12 |
| gke_enable_auto_repair | SI-2 |
| gke_enable_auto_upgrade | SI-2 |
| gke_enable_ip_aliasing | SC-7 |
| gke_enable_master_networks | SC-7 |
| gke_enable_network_policy | SC-7 |
| gke_enable_private_cluster | SC-7 |
| gke_enable_stackdriver_logging | AU-2 |
| gke_enable_stackdriver_monitoring | AU-6 |
| gke_metadata_endpoints_disabled | CM-6 |
| gke_no_basic_authentication | IA-2 |
| gke_no_client_cert_authentication | IA-2 |
| gke_node_metadata_security | CM-6 |
| gke_node_pool_uses_cos | CM-6 |
| gke_node_shielding_enabled | SI-7 |
| gke_no_public_control_plane | SC-7 |
| gke_use_cluster_labels | CM-8 |
| gke_use_rbac_permissions | AC-3 |
| iam_no_folder_level_default_service_account_assignment | AC-6 |
| iam_no_folder_level_service_account_impersonation | AC-6 |
| iam_no_privileged_service_accounts | AC-6 |
| storage_enable_ubla | AC-3 |
| storage_no_public_access | AC-3 |

### 4.5 Control Family Coverage Summary

| NIST 800-53 Control Family | AWS CF | AWS TF | GCP TF | Total |
|---------------------------|--------|--------|--------|-------|
| AC (Access Control) | 3 | 3 | 9 | 15 |
| AU (Audit & Accountability) | 0 | 3 | 3 | 6 |
| CM (Configuration Management) | 0 | 0 | 5 | 5 |
| CP (Contingency Planning) | 1 | 1 | 0 | 2 |
| IA (Identification & Auth) | 9 | 4 | 2 | 15 |
| MP (Media Protection) | 1 | 0 | 0 | 1 |
| SC (System & Comms Protection) | 12 | 6 | 10 | 28 |
| SI (System & Info Integrity) | 0 | 0 | 5 | 5 |

### 4.6 Translating Rego Policies to Compliance Validation Rules

To translate Rego policies into compliance validation rules:

1. **Extract the resource type** from the package name and type checks
2. **Extract the property checks** — these become the compliance requirements
3. **Map to NIST control** — use the policy name/comment to identify control family
4. **Generate validation schema**:
```json
{
  "rule_id": "aws-cf-sc28-cloudtrail-encryption",
  "framework": "NIST 800-53",
  "control": "SC-28",
  "provider": "AWS",
  "resource_type": "AWS::CloudTrail::Trail",
  "check": "Properties.KMSKeyId exists",
  "severity": "high",
  "remediation": "Add KMSKeyId property to CloudTrail trail"
}
```

---

## 5. Security Policy Templates + Controls Mapping

**Source**: `compliance_corpora/security-policy-templates/`
**Provider**: JupiterOne / LifeOmic Security
**License**: Apache 2.0 (for templates); Framework-specific licenses required for standards

### 5.1 Repository Structure

```
templates/
├── assessments/          # Assessment report templates (HIPAA, compliance)
├── config.json           # Master configuration (129KB)
├── index.md.tmpl         # Index page template
├── mkdocs/               # MkDocs configuration
├── policies/             # 26 policy templates (.md.tmpl)
├── procedures/           # 130+ procedure templates (.md.tmpl)
├── ref/                  # Reference document templates (12 files)
└── standards/            # Framework mapping files (30+ files)
    └── controls-mapping.json  # CRITICAL: Cross-framework mapping
```

### 5.2 Policy Domains (8 Domains)

| Domain ID | Domain Name | Policies |
|-----------|------------|----------|
| AI | Assets and Infrastructure | asset-mgmt, ccm, mdm |
| PA | People and Access | access, hr, rar |
| VM | Vulnerability Management | vuln-mgmt |
| SD | Software Development | sdlc |
| DA | Data and Applications | data-mgmt, data-protection |
| OR | Operations and Response | model, bcdr, breach, system-audit, threat |
| PS | Physical and Datacenter Security | facility |
| GRC | Governance, Risk and Compliance | bcdr, compliance-audit, corp-gov, policy-mgmt, privacy, rar, risk-mgmt, vendor |

### 5.3 All 26 Policies

| Policy ID | Policy Name |
|-----------|------------|
| program | Security Program Overview |
| corp-gov | Corporate Governance |
| policy-mgmt | Policy Management |
| model | Security Architecture and Operating Model |
| rar | Roles, Responsibilities and Training |
| risk-mgmt | Risk Management and Risk Assessment Process |
| compliance-audit | Compliance Audits and External Communications |
| system-audit | System Audits, Monitoring and Assessments |
| hr | HR and Personnel Security |
| access | Access |
| facility | Facility Access and Physical Security |
| asset-mgmt | Asset Inventory Management |
| data-mgmt | Data Management |
| data-protection | Data Protection |
| sdlc | Secure Software Development and Product Security |
| ccm | Configuration and Change Management |
| threat | Threat Detection and Prevention |
| vuln-mgmt | Vulnerability Management |
| mdm | Mobile Device Security and Media Management |
| bcdr | Business Continuity and Disaster Recovery |
| ir | Incident Response |
| breach | Breach Investigation and Notification |
| vendor | Third Party Security and Vendor Risk Management |
| privacy | Privacy Practice and Consent |
| ref | Addendum and References |

### 5.4 Controls Mapping — CRITICAL CROSS-FRAMEWORK REFERENCE

The `controls-mapping.json` maps **143 procedures** to **10 compliance frameworks**:

#### Frameworks Covered
1. **HIPAA** (Health Insurance Portability and Accountability Act)
2. **HITRUST CSF** (Health Information Trust Alliance Common Security Framework)
3. **CSA CCM** (Cloud Security Alliance Cloud Controls Matrix)
4. **PCI DSS** (Payment Card Industry Data Security Standard)
5. **NIST CSF** (NIST Cybersecurity Framework)
6. **ISO/IEC 27002:2013**
7. **CIS Controls v8** (Center for Internet Security Controls)
8. **CMMC ML1** (Cybersecurity Maturity Model Certification Maturity Level 1)
9. **SOC 2** (Service Organization Control 2)
10. **CSF** (generic Cybersecurity Framework reference)

#### Complete Procedure-to-Framework Mapping (Representative Entries)

**Security Program & Architecture:**

| Procedure | HIPAA | CSA CCM | PCI DSS | NIST CSF | ISO 27002 | CIS v8 | CMMC | SOC 2 |
|-----------|-------|---------|---------|----------|-----------|--------|------|-------|
| cp-ism-scope | — | — | — | ID.GV-1, ID.BE-3 | 7.2.1 | — | — | — |
| cp-model-principles | — | AIS-04, GRM-06, IAM-06, IPY-01, STA-03 | 1.3, 1.5, 2.3, 6.3, 6.4, 6.7 | PR.PT-3 | 6.1.6 | 4.6 | SC.1.175 | — |
| cp-model-architecture | — | BCR-04, IPY-05, STA-03 | — | — | 12.1.3 | 12.2, 12.4, 16.8 | — | — |
| cp-model-metrics | 164.308(a)(8) | SEF-05 | — | PR.IP-7 | — | — | — | — |

**Risk Management:**

| Procedure | HIPAA | CSA CCM | PCI DSS | NIST CSF | ISO 27002 | CIS v8 |
|-----------|-------|---------|---------|----------|-----------|--------|
| cp-risk-mgmt | 164.308(a)(1)(ii)(B), 164.308(a)(8) | BCR-09, GRM-02, GRM-04, GRM-10, GRM-11, STA-06 | 6.4, 6.6 | ID.GV-4, ID.RM-1, ID.RA-5, ID.RA-6 | — | — |
| cp-risk-assess | 164.308(a)(1)(ii)(A) | BCR-09, GRM-02, GRM-10 | 2.2, 6.1, 12.2 | ID.RA-1, ID.RA-3, ID.RA-4, ID.RA-6 | — | — |
| cp-risk-mitigation | 164.308(a)(1)(ii)(A), 164.308(a)(8) | GRM-08, GRM-11, MOS-07 | 12.2 | ID.RA-6 | — | — |

**Access Control:**

| Procedure | HIPAA | CSA CCM | PCI DSS | NIST CSF | ISO 27002 | CIS v8 | CMMC |
|-----------|-------|---------|---------|----------|-----------|--------|------|
| cp-access-standards | 164.308(a)(3)(i), 164.312(a)(1) + more | AIS-04, BCR-04, HRS-07, IAM-02/04/08/12, IVS-09/11, MOS-14 | 7.1, 8.1-8.5 | PR.AC-1, PR.AC-4 | 9.1.1, 9.2.1, 9.2.2 | 4.7, 6.1, 12.8 | IA.1.077, IA.1.076, AC.1.002, AC.1.001 |
| cp-access-mfa | 164.308(a)(3)(i), 164.312(a)(1) | BCR-04, IAM-02, IAM-12 | 8.1-8.3 | PR.AC-1 | — | 6.3-6.5 | — |
| cp-access-rbac | 164.308(a)(3)(i/ii)(B), 164.312(a)(1) | GRM-06, HRS-07, IAM-02 | — | — | — | 6.8 | — |

**Data Protection:**

| Procedure | HIPAA | CSA CCM | PCI DSS | NIST CSF | ISO 27002 | CIS v8 |
|-----------|-------|---------|---------|----------|-----------|--------|
| cp-data-protection | 164.312(a)(2)(iv), 164.312(e)(2)(ii) | DSI-01-07, EKM-01-04 | 3.1-3.7, 4.1-4.3 | PR.DS-1, PR.DS-2, PR.DS-5 | 10.1.1, 14.1.2/3 | 3.1-3.12 |
| cp-data-classification | 164.308(a)(7)(ii)(A) | DSI-01, DSI-04 | 3.1 | ID.AM-5, PR.DS-5 | 8.2.1 | 3.7, 3.13 |
| cp-data-lifecycle | — | DSI-05, DSI-07 | 3.1 | — | 8.2.2 | 3.1, 3.4 |
| cp-data-backup | 164.308(a)(7)(ii)(A), 164.310(d)(2)(iv) | BCR-11 | — | PR.IP-4 | 12.3.1 | 11.1-11.5 |
| cp-data-deletion | 164.310(d)(2)(i/ii) | DSI-07 | 3.1 | PR.IP-6 | — | 3.5 |

**Incident Response & Breach:**

| Procedure | HIPAA | CSA CCM | PCI DSS | NIST CSF | CIS v8 |
|-----------|-------|---------|---------|----------|--------|
| cp-ir-process | 164.308(a)(6)(i/ii) | SEF-02 | 12.10, 12.10.1, 12.10.5 | RS.RP-1, RS.AN-1/2, RS.CO-1-4, RS.MI-1/2, DE.AE-2/4 | 17.1-17.9 |
| cp-ir-playbook | 164.308(a)(6)(ii), 164.308(a)(7)(ii)(E) | BCR-02, SEF-02 | 12.10, 12.10.4 | RS.AN-4 | 17.4-17.6 |
| cp-breach-investigate | 164.308(a)(6)(ii) | SEF-02/03/04 | 12.10, 12.10.1 | RC.CO-1/2/3, RS.CO-2/3, RS.RP-1 | — |

**Software Development (SDLC):**

| Procedure | HIPAA | CSA CCM | PCI DSS | NIST CSF | ISO 27002 | CIS v8 |
|-----------|-------|---------|---------|----------|-----------|--------|
| cp-sdlc-dev | 164.312(c)(1/2) | AAC-03 | 6.3, 6.4 | PR.IP-2 | 12.1.4, 14.2.1/2 | 16.1-16.14 |
| cp-sdlc-scm | — | — | — | PR.DS-6 | — | 16.3 |
| cp-sdlc-foss | — | — | 6.3 | — | — | 16.4, 16.5 |
| cp-sdlc-sast | — | — | 6.3 | — | — | 16.12 |
| cp-sdlc-pentest | — | — | 6.6, 11.3 | — | — | 16.13, 16.14 |

### 5.5 Additional Standards Files in the Repository

The `templates/standards/` directory contains extensive framework definition files:

| File | Size | Description |
|------|------|-------------|
| controls-mapping.json | 108 KB | Cross-framework procedure mapping (10 frameworks) |
| scf.json | 269 KB | Secure Controls Framework |
| sunstone-merged-moderate.json | 238 KB | Sunstone merged moderate controls |
| hipaa.json | 43 KB | HIPAA requirements |
| iso-iec-27001-2022.json | 118 KB | ISO/IEC 27001:2022 controls |
| iso-iec-27001-2013.json | 102 KB | ISO/IEC 27001:2013 controls |
| csa-ccm.json | 81 KB | CSA Cloud Controls Matrix |
| pci-dss.json | 60 KB | PCI DSS requirements |
| thsa.json | 61 KB | THSA (Texas Health Services Authority) |
| iso-27002-2022.json | 49 KB | ISO 27002:2022 controls |
| iso-iec-27002-2013.json | 45 KB | ISO/IEC 27002:2013 controls |
| iso-iec-27002-2005.json | 39 KB | ISO/IEC 27002:2005 controls |
| security-program.json | 39 KB | Security program definition |
| gdpr-example.json | 11 KB | GDPR example mapping |
| cmmc-level1.json | 5 KB | CMMC Level 1 |
| cmmc-ml1.json | 8 KB | CMMC Maturity Level 1 |

Plus subdirectories for: `aws/`, `cis-aws-foundations/`, `cis-azure-foundations/`, `cis-controls/`, `cis-gcp-foundations/`, `cis-oci-foundations/`, `fedramp/`, `hipaa/`, `nist-800-53/`, `nist-csf/`, `pci-dss/`, `soc2/`, `thsa/`

### 5.6 Utility Tools

| Tool | Purpose |
|------|---------|
| `compliance.py` | Whoosh-based full-text search across standards/procedures mapping |
| `build-summary.js` | Builds the summary.md document |
| `control-req-mapper.js` | Maps controls to requirements |
| `internal-standard.js` | Internal standard processing |
| `parser-fedramp.js` | FedRAMP standard parser |
| `parser-soc2.js` | SOC 2 standard parser |
| `parser-thsa.js` | THSA standard parser |
| `setup-compliance-script.py` | Compliance setup automation |

### 5.7 Config Organization Variables

The `config.json` defines organization-level variables:
- Company info (name, email domain, website, mailing address)
- Key personnel (CEO, COO, CTO, Security Officer, Privacy Officer)
- Tool configuration (source control, ticketing, CI system, HR system, IdP)
- Compliance toggles: `needStandardHIPAA`, `needStandardHITRUST`, `needStandardGDPR`, `needStandardNIST`, `needStandardPCI`

---

## 6. TechOps Documentation Templates

**Source**: `compliance_corpora/techops/`
**Published**: AIES 2025 (AAAI/ACM Conference on AI, Ethics, and Society)
**Focus**: EU AI Act compliance documentation for AI/ML systems
**Rendering**: MkDocs-based blueprint

### 6.1 Three-Level Template Architecture

TechOps provides documentation at three levels to support separation of ownership:

| Level | Template | EU AI Act Articles | Owner |
|-------|----------|-------------------|-------|
| **Application** | `template/application documentation.md` (480 lines) | Art. 5-14, Annex IV | AI System Provider |
| **Model** | `template/model documentation.md` (498 lines) | Art. 11, 13, Annex IV | Model Developer |
| **Data** | `template/data documentation.md` (508 lines) | Art. 10, 11, 13, Annex IV | Data Owner |

### 6.2 Application Documentation Template Sections

Maps directly to EU AI Act articles:

| Section | EU AI Act Reference | Content |
|---------|-------------------|---------|
| **General Information** | Art. 11; Annex IV §1,2,3 | Purpose, intended use, sector, KPIs, ethical implications, operational environment |
| **Risk Classification** | Art. 5 (Prohibited), Art. 6-7 (High-Risk), Art. 50 (Limited) | Risk level determination with reasoning |
| **Application Functionality** | Art. 11; Annex IV §1,2,3 | Model capabilities, input/output specs, system architecture overview |
| **Models and Datasets** | Art. 11; Annex IV §2(d) | Links to model/dataset documentation (TechOps docs) |
| **Deployment** | Art. 11; Annex IV §1(b,c,d,g,h) | Infrastructure, APIs, integration, deployment plan |
| **Lifecycle Management** | Art. 11; Annex IV §6 | Monitoring, versioning, change logs, metrics, audit trails |
| **Risk Management System** | Art. 9, Art. 11 | Risk assessment methodology, identified risks, mitigation measures |
| **Human Oversight** | Art. 14 | Oversight mechanisms, human-in-the-loop requirements |
| **Transparency & Information** | Art. 13 | User-facing documentation, disclosure requirements |
| **Data Governance** | Art. 10 | Data quality, bias assessment, privacy measures |
| **Accuracy, Robustness, Cybersecurity** | Art. 15 | Performance metrics, security measures, testing |
| **Logging & Traceability** | Art. 12 | Automatic logging, audit capabilities |
| **Conformity Assessment** | Art. 43 | Assessment procedures and documentation |

### 6.3 Model Documentation Template Sections

| Section | EU AI Act Reference | Content |
|---------|-------------------|---------|
| **Overview** | Art. 11 §1 | Model type, description, status, links, developers |
| **Version Details** | Art. 11; Annex IV §1(c) | Version tracking, artifacts (weights, configs) |
| **Intended and Known Usage** | Art. 11; Annex IV §1(f) | Intended use, domain, out-of-scope uses, known applications with risk levels |
| **Model Architecture** | Art. 11; Annex IV §2(b,c) | Architecture, hyperparameters, training methodology, compute resources |
| **Data Collection** | Art. 11; Annex IV §2(d) | Training data description, preprocessing, labeling |
| **Evaluation** | Art. 11; Annex IV §2(e) | Metrics, benchmarks, validation methodology |
| **Fairness Analysis** | Art. 11; Annex IV §2(f) | Bias assessment, demographic performance, mitigation |
| **Limitations** | Art. 11; Annex IV §2(g) | Known limitations, failure modes, edge cases |
| **Environmental Impact** | Annex IV §2(h) | Carbon footprint, compute efficiency |

### 6.4 Data Documentation Template Sections

| Section | EU AI Act Reference | Content |
|---------|-------------------|---------|
| **Overview** | Art. 11; Annex IV §1, §2(d) | Description, status, links, developers |
| **Data Versioning** | Art. 11 §2(d) | Version control tools (DVC, Git-LFS) |
| **Metadata/Schema Versioning** | Art. 11; Annex IV §3 | Data dictionary, schema tracking |
| **Known Usages** | Art. 11; Annex IV §3 | Models and applications using this dataset |
| **Dataset Characteristics** | Art. 11; Annex IV §2(d) | Data types, size, instances, features, labels, geography, date |
| **Data Origin** | GDPR + AI Act | Sources, third-party data, ethical sourcing |
| **Provenance** | Art. 10 | Collection methodology, processing history |
| **Data Quality** | Art. 10 | Quality metrics, validation, cleaning |
| **Sensitive Data** | Art. 10; GDPR | PII handling, consent, anonymization |
| **Bias & Fairness** | Art. 10 | Representation analysis, known biases |

### 6.5 Worked Examples

The repository includes three complete examples:

1. **SafeSiteAI** (Application): Fictional high-risk AI system for construction worker safety detection using real-time video analytics and sensor fusion
2. **AlisNet** (Model): Neural network for segmenting human silhouettes in photos
3. **VOC Skin Tones** (Data): Skin tones dataset for fairness evaluation of downstream computer vision models

### 6.6 Rendering Blueprint

```bash
# Install with uv
uv sync && uv run pre-commit install

# Render documentation locally
uv run mkdocs serve

# Deploy
# See mkdocs deployment documentation
```

---

## 7. Cross-Corpus Analysis

### 7.1 Framework Coverage Matrix

| Framework | EU AI Act Layer | compl-ai | GDPR Guide | Rego NIST | Security Policy Templates | TechOps |
|-----------|:-:|:-:|:-:|:-:|:-:|:-:|
| **EU AI Act** | ✅ Primary | ✅ Benchmarks | — | — | — | ✅ Templates |
| **GDPR** | ✅ Checklist | — | ✅ Primary | — | ✅ DPA template | ✅ Data docs |
| **NIST 800-53** | — | — | — | ✅ Primary (77 policies) | ✅ Mapping | — |
| **NIST CSF** | — | — | — | — | ✅ Full mapping | — |
| **HIPAA** | — | — | — | — | ✅ Full mapping + BAA | — |
| **ISO/IEC 27001/27002** | — | — | — | — | ✅ Full mapping (2005/2013/2022) | — |
| **PCI DSS** | — | — | — | — | ✅ Full mapping | — |
| **SOC 2** | — | — | — | — | ✅ Full mapping | — |
| **CSA CCM** | — | — | — | — | ✅ Full mapping | — |
| **HITRUST CSF** | — | — | — | — | ✅ Mapping | — |
| **CIS Controls v8** | — | — | — | — | ✅ Full mapping | — |
| **CMMC** | — | — | — | — | ✅ Level 1 mapping | — |
| **FedRAMP** | — | — | — | — | ✅ Parser + data | — |
| **SCF** | — | — | — | — | ✅ Full (269KB) | — |

### 7.2 Gaps Identified

1. **EU AI Act ↔ NIST 800-53 mapping**: No direct mapping exists between EU AI Act requirements and NIST 800-53 controls. This is a significant gap for organizations subject to both.

2. **EU AI Act ↔ ISO 27001 mapping**: While TechOps covers EU AI Act documentation and Security Policy Templates cover ISO 27001, there's no explicit cross-walk.

3. **Operational Rego policies for EU AI Act**: The Rego policies only cover infrastructure compliance (NIST 800-53). No Rego policies exist for EU AI Act technical requirements (Article 5 prohibited practices, risk classification, etc.).

4. **GDPR ↔ EU AI Act integration**: While both are covered separately, the intersection (AI systems processing personal data) lacks a unified validation framework.

5. **Dynamic/runtime compliance validation**: All corpora focus on static analysis and documentation. No runtime monitoring or continuous compliance validation tools are included.

6. **GCP CloudFormation equivalent**: Only Terraform policies exist for GCP; no CloudFormation equivalent (expected, as GCP doesn't use CloudFormation).

7. **Azure coverage**: No Rego policies exist for Azure resources.

8. **compl-ai ↔ Security Policy Templates integration**: The LLM benchmarks don't connect to organizational security policies.

### 7.3 Recommended Ingestion Priority

| Priority | Source | Rationale |
|----------|--------|-----------|
| **1 (Critical)** | `security-policy-templates/templates/standards/controls-mapping.json` | Cross-framework Rosetta Stone — maps 143 procedures to 10 frameworks. Foundation for any compliance automation. |
| **2 (Critical)** | `eu-ai-act-layer-lite/eu-ai-act-layer-v3.6.0-lite.json` | Structured EU AI Act governance schema — directly machine-parseable. |
| **3 (High)** | `GDPR-Developer-Guide/*.md` (all 17 sheets) | Most comprehensive developer-oriented GDPR guidance. Extract as compliance rules with technical requirements. |
| **4 (High)** | `compl-ai/` (benchmark registry + config) | Complete EU AI Act benchmark mapping. Use to validate AI model compliance. |
| **5 (High)** | `techops/template/` (3 templates) | EU AI Act documentation templates with article-level mapping. Required for conformity assessment. |
| **6 (Medium)** | `rego-cns/` (all .rego files) | NIST 800-53 infrastructure policies. Convert to validation rules for cloud compliance. |
| **7 (Medium)** | `security-policy-templates/templates/standards/*.json` | Individual framework definitions (HIPAA, ISO, PCI, etc.). Use for framework-specific deep dives. |
| **8 (Lower)** | `security-policy-templates/templates/policies/` + `procedures/` | Organizational policy/procedure text templates. Use when building complete compliance programs. |

### 7.4 Unified Compliance Rule Schema Recommendation

Based on all corpora analyzed, a unified compliance rule schema should capture:

```json
{
  "rule_id": "string (unique identifier)",
  "version": "string (semantic version)",
  "source_corpus": "string (eu-ai-act-layer | compl-ai | gdpr-guide | rego-nist | security-policy-templates | techops)",
  "frameworks": [
    {
      "name": "string (EU AI Act | GDPR | NIST 800-53 | HIPAA | ISO 27001 | PCI DSS | SOC 2 | etc.)",
      "references": ["string (Article 5 | 164.308(a)(1) | SC-28 | etc.)"]
    }
  ],
  "category": "string (data-protection | access-control | risk-management | transparency | fairness | security | documentation | governance)",
  "subcategory": "string (encryption | authentication | data-minimization | bias-testing | etc.)",
  "description": "string (human-readable description of the compliance requirement)",
  "technical_requirement": "string (specific technical implementation needed)",
  "validation_type": "string (static-analysis | runtime-check | documentation-review | benchmark-evaluation | manual-audit)",
  "validation_method": {
    "type": "string (rego-policy | llm-benchmark | checklist | data-scan | code-review)",
    "specification": "object (policy-specific parameters)"
  },
  "severity": "string (critical | high | medium | low)",
  "applicability": {
    "system_types": ["string (high-risk-ai | general-purpose-ai | web-application | cloud-infrastructure | etc.)"],
    "cloud_providers": ["string (aws | gcp | azure | any)"],
    "data_types": ["string (personal-data | sensitive-data | health-data | financial-data | any)"]
  },
  "remediation": "string (how to fix non-compliance)",
  "evidence_required": ["string (list of evidence types needed to prove compliance)"],
  "review_frequency": "string (continuous | daily | weekly | monthly | quarterly | annually)",
  "owner_role": "string (DPO | CISO | ML Engineer | DevOps | Legal | etc.)"
}
```

This schema unifies:
- EU AI Act artifacts (from eu-ai-act-layer-lite)
- LLM benchmark evaluation results (from compl-ai)
- GDPR developer rules (from GDPR-Developer-Guide)
- Infrastructure validation policies (from rego-cns)
- Organizational control procedures (from security-policy-templates)
- Documentation compliance checks (from techops)

---

*End of Compliance Corpora Knowledge Base*
