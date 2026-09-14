# Python Testing Patterns

## pytest Fundamentals
```python
# Test discovery: test_*.py or *_test.py
def test_function():
    assert result == expected

class TestClass:
    def test_method(self):
        assert condition
```

## Fixtures
```python
@pytest.fixture
def database():
    db = create_test_db()
    yield db
    db.cleanup()

@pytest.fixture(scope="session")
def expensive_resource():
    return load_once()

def test_with_fixture(database):
    database.query(...)
```

## Parametrization
```python
@pytest.mark.parametrize("input,expected", [
    (1, 2),
    (2, 4),
    (3, 6),
])
def test_double(input, expected):
    assert double(input) == expected
```

## Mocking
```python
from unittest.mock import Mock, patch, MagicMock

def test_with_mock():
    mock_service = Mock()
    mock_service.call.return_value = "result"
    
@patch('module.external_function')
def test_patched(mock_func):
    mock_func.return_value = "mocked"
```

## Async Testing
```python
@pytest.mark.asyncio
async def test_async_function():
    result = await async_operation()
    assert result
```

## Coverage
- `pytest --cov=mypackage --cov-report=html`
- Target 80%+ line coverage
- 100% for critical paths

## Best Practices
1. One assertion per test (ideally)
2. Use descriptive test names
3. Arrange-Act-Assert pattern
4. Test edge cases and errors
5. Keep tests fast and isolated
