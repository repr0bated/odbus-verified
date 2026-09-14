# Code Review Excellence

## Review Checklist
### Correctness
- Does it solve the stated problem?
- Are edge cases handled?
- Are there race conditions?
- Is error handling appropriate?

### Design
- Does it follow SOLID principles?
- Is the abstraction level appropriate?
- Are there unnecessary dependencies?
- Is it testable?

### Security
- Input validation present?
- SQL injection risks?
- XSS vulnerabilities?
- Sensitive data exposure?

### Performance
- N+1 queries?
- Unnecessary allocations?
- Missing caching opportunities?
- Appropriate data structures?

## Giving Feedback
### Good Feedback
```
"Consider using a Set here instead of Array for O(1) lookups.
Currently this is O(n) per check, which could be slow with 
large datasets."
```

### Bad Feedback
```
"This is wrong."
"Use a Set."
```

## Review Workflow
1. **Understand Context**: Read the PR description and linked issues
2. **Big Picture First**: Architecture and design decisions
3. **Then Details**: Implementation specifics
4. **Be Constructive**: Suggest alternatives, explain why
5. **Prioritize**: Mark must-fix vs nice-to-have

## Praise Good Work
- Acknowledge clever solutions
- Note improvements over previous code
- Recognize effort on tests/docs

## Common Patterns to Flag
- Magic numbers/strings
- Deep nesting (>3 levels)
- Long functions (>50 lines)
- Missing error handling
- Commented-out code
- TODO without ticket reference
- Inconsistent naming
