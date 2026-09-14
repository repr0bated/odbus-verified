# Debugging Strategies

## Systematic Approach
1. **Reproduce**: Create minimal reproduction
2. **Isolate**: Narrow down the problem area
3. **Hypothesize**: Form theory about cause
4. **Test**: Verify hypothesis
5. **Fix**: Implement solution
6. **Verify**: Confirm fix doesn't break other things

## Binary Search Debugging
```python
# If bug appeared between commit A and B
git bisect start
git bisect bad HEAD
git bisect good <commit-A>
# Git will checkout commits for testing
git bisect good  # or bad
# Continue until culprit found
```

## Print Debugging (Strategic)
```python
import logging

logger = logging.getLogger(__name__)

def process_order(order):
    logger.debug(f"Processing order: {order.id}, items: {len(order.items)}")
    
    for item in order.items:
        logger.debug(f"  Item: {item.sku}, qty: {item.quantity}")
        
    result = calculate_total(order)
    logger.debug(f"Order {order.id} total: {result}")
    return result
```

## Profiling
```python
# CPU profiling
import cProfile
cProfile.run('main()', 'output.prof')

# Memory profiling
from memory_profiler import profile

@profile
def memory_heavy_function():
    ...

# py-spy for production
# py-spy record -o profile.svg --pid 12345
```

## Debugger Usage
```python
# Python debugger
import pdb; pdb.set_trace()  # or breakpoint()

# Commands:
# n - next line
# s - step into
# c - continue
# p variable - print variable
# l - list code
# w - where (stack trace)
```

## Common Bug Patterns
- Off-by-one errors
- Null/None reference
- Race conditions
- Resource leaks
- Integer overflow
- Encoding issues
- Timezone problems
- Floating point precision

## Rubber Duck Debugging
Explain the problem out loud:
1. What should happen?
2. What actually happens?
3. What have you tried?
4. What assumptions are you making?
