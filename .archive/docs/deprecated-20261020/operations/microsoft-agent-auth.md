# Microsoft Agent Auth

This flow gives agents a delegated Microsoft identity without handing credentials to the agent.

## App registration

1. Create a Microsoft Entra app registration.
2. Choose an account type that includes personal Microsoft accounts.
3. Enable public client flows for the app.
4. Add delegated Microsoft Graph permissions:
   - `offline_access`
   - `openid`
   - `profile`
   - `User.Read`
5. Add any extra delegated scopes the agent actually needs, for example `Mail.Read` or `Files.Read`.

## First login

Run the device-code helper with a stable label per account:

```bash
cargo run --bin microsoft-device-login -- login \
  --client-id YOUR_APP_ID \
  --label jeremy-gmail \
  --expected-email jeremy.alan.hobson@gmail.com
```

Repeat for the Outlook account:

```bash
cargo run --bin microsoft-device-login -- login \
  --client-id YOUR_APP_ID \
  --label jeremy-outlook \
  --expected-email YOUR_OUTLOOK_ADDRESS
```

The command prints the Microsoft verification URL and device code. Complete sign-in in the browser window that Microsoft opens or on the URL it prints.

## Token storage

Token files are written to:

```text
~/.config/op-dbus/microsoft/<label>.json
```

They are stored with mode `0600` on Unix.

## Agent environment

Point an agent at a token file:

```bash
export MICROSOFT_AUTH_TOKEN_FILE=~/.config/op-dbus/microsoft/jeremy-gmail.json
```

To print the exact export line:

```bash
cargo run --bin microsoft-device-login -- print-env --label jeremy-gmail
```

## Refresh and inspection

Refresh an existing token file:

```bash
cargo run --bin microsoft-device-login -- refresh --label jeremy-gmail
```

Inspect the resolved account:

```bash
cargo run --bin microsoft-device-login -- whoami --label jeremy-gmail
```

## Notes

- This is delegated auth. The user still completes Microsoft sign-in in the browser.
- The helper saves refreshable tokens so agents do not need to re-run the browser flow every time.
- If an agent needs additional Microsoft Graph scopes later, run the login command again with extra `--scope` flags.
