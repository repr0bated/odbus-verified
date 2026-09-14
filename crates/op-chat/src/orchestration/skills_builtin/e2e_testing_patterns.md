# E2E Testing Patterns

## Playwright Setup
```typescript
import { test, expect } from '@playwright/test';

test('user login flow', async ({ page }) => {
  await page.goto('/login');
  await page.fill('[name="email"]', 'user@example.com');
  await page.fill('[name="password"]', 'password');
  await page.click('button[type="submit"]');
  await expect(page).toHaveURL('/dashboard');
});
```

## Page Object Model
```typescript
class LoginPage {
  constructor(private page: Page) {}
  
  async login(email: string, password: string) {
    await this.page.fill('[name="email"]', email);
    await this.page.fill('[name="password"]', password);
    await this.page.click('button[type="submit"]');
  }
}
```

## Test Patterns
### Wait Strategies
```typescript
await page.waitForSelector('.loaded');
await page.waitForResponse(resp => resp.url().includes('/api'));
await expect(element).toBeVisible({ timeout: 5000 });
```

### Network Interception
```typescript
await page.route('**/api/**', route => {
  route.fulfill({ json: mockData });
});
```

## Cypress Alternative
```javascript
cy.visit('/login')
cy.get('[data-cy="email"]').type('user@example.com')
cy.get('[data-cy="submit"]').click()
cy.url().should('include', '/dashboard')
```

## Best Practices
1. Use data-testid attributes for selectors
2. Implement retry logic for flaky tests
3. Run tests in parallel
4. Use visual regression testing
5. Test critical user journeys first
6. Keep E2E tests focused and fast
