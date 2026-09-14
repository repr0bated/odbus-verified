#!/usr/bin/env python3
"""
Setup Gemini OAuth Authentication

Reads the client_secret_*.json file, performs the OAuth flow to get a refresh token,
and saves it as ~/.config/gcloud/application_default_credentials.json.

Then enables Vertex AI OAuth mode for the 3tched system.

Usage:
    python3 scripts/setup-gemini-oauth.py

Prerequisites:
    - Python 3.7+
    - The client_secret_*.json file in the repo root
    - If using localhost redirect: add http://localhost:8085 to your
      Google Cloud Console > APIs & Services > Credentials > OAuth 2.0 Client ID
      > Authorized redirect URIs
"""

import json
import http.server
import socketserver
import threading
import urllib.parse
import urllib.request
import os
import sys
import time
from pathlib import Path

# =============================================================================
# CONFIGURATION
# =============================================================================

CLIENT_SECRETS_FILE = Path(__file__).parent.parent / "client_secret_470884133715-2mt41dvna0jvg3bmub91iliomlaoi746.apps.googleusercontent.com.json"
ADC_FILE = Path.home() / ".config" / "gcloud" / "application_default_credentials.json"
LOCALHOST_PORT = 8085
LOCALHOST_REDIRECT = f"http://localhost:{LOCALHOST_PORT}"
SCOPES = "https://www.googleapis.com/auth/cloud-platform"

# =============================================================================
# LOAD CLIENT SECRETS
# =============================================================================

def load_client_secrets():
    if not CLIENT_SECRETS_FILE.exists():
        print(f"ERROR: Client secrets file not found: {CLIENT_SECRETS_FILE}")
        print("Make sure the client_secret_*.json file is in the repo root.")
        sys.exit(1)
    
    with open(CLIENT_SECRETS_FILE) as f:
        data = json.load(f)
    
    web = data.get('web', {})
    return {
        'client_id': web['client_id'],
        'client_secret': web['client_secret'],
        'auth_uri': web['auth_uri'],
        'token_uri': web['token_uri'],
        'redirect_uris': web.get('redirect_uris', []),
    }

# =============================================================================
# OAUTH FLOW
# =============================================================================

auth_code = None
server_running = True

class OAuthHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass  # Suppress request logs
    
    def do_GET(self):
        global auth_code, server_running
        parsed = urllib.parse.urlparse(self.path)
        query = urllib.parse.parse_qs(parsed.query)
        
        if 'code' in query:
            auth_code = query['code'][0]
            self.send_response(200)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            self.wfile.write(b"""
                <html><body style="font-family:sans-serif;text-align:center;padding-top:50px;">
                <h1 style="color:green;">Authorization Successful!</h1>
                <p>You can close this window and return to the terminal.</p>
                </body></html>
            """)
            server_running = False
        elif 'error' in query:
            self.send_response(400)
            self.send_header('Content-Type', 'text/html')
            self.end_headers()
            error = query['error'][0]
            self.wfile.write(f"""
                <html><body style="font-family:sans-serif;text-align:center;padding-top:50px;">
                <h1 style="color:red;">Authorization Failed</h1>
                <p>Error: {error}</p>
                </body></html>
            """.encode())
            server_running = False
        else:
            self.send_response(400)
            self.end_headers()
            self.wfile.write(b"Missing authorization code.")

def start_local_server(port):
    with socketserver.TCPServer(("", port), OAuthHandler) as httpd:
        httpd.timeout = 1
        while server_running:
            try:
                httpd.handle_request()
            except Exception:
                break

def build_auth_url(secrets, redirect_uri):
    params = {
        'client_id': secrets['client_id'],
        'redirect_uri': redirect_uri,
        'scope': SCOPES,
        'response_type': 'code',
        'access_type': 'offline',
        'prompt': 'consent',
    }
    return f"{secrets['auth_uri']}?{urllib.parse.urlencode(params)}"

def exchange_code_for_tokens(secrets, code, redirect_uri):
    data = urllib.parse.urlencode({
        'code': code,
        'client_id': secrets['client_id'],
        'client_secret': secrets['client_secret'],
        'redirect_uri': redirect_uri,
        'grant_type': 'authorization_code',
    }).encode()
    
    req = urllib.request.Request(secrets['token_uri'], data=data, method='POST')
    req.add_header('Content-Type', 'application/x-www-form-urlencoded')
    
    try:
        with urllib.request.urlopen(req) as response:
            return json.loads(response.read().decode())
    except urllib.error.HTTPError as e:
        body = e.read().decode()
        print(f"\nERROR: Token exchange failed ({e.code})")
        print(f"Response: {body}")
        return None

def save_adc(creds, secrets):
    ADC_FILE.parent.mkdir(parents=True, exist_ok=True)
    
    adc = {
        'client_id': secrets['client_id'],
        'client_secret': secrets['client_secret'],
        'refresh_token': creds['refresh_token'],
        'type': 'authorized_user',
    }
    
    if 'quota_project_id' in secrets:
        adc['quota_project_id'] = secrets['quota_project_id']
    
    with open(ADC_FILE, 'w') as f:
        json.dump(adc, f, indent=2)
    
    ADC_FILE.chmod(0o600)
    print(f"\nSaved credentials to: {ADC_FILE}")
    print("  (Permissions set to 600 for security)")

def print_env_setup():
    print("\n" + "="*70)
    print("NEXT STEPS: Configure Environment Variables")
    print("="*70)
    print("""
Add these to your shell profile (~/.bashrc, ~/.zshrc) or export them now:

  export GOOGLE_GENAI_USE_VERTEXAI=true
  export GOOGLE_CLOUD_PROJECT=codeassist-492619
  export GOOGLE_CLOUD_LOCATION=us-central1

For the Gemini CLI provider (optional):
  export ENABLE_GEMINI_CLI_PROVIDER=true

For API key fallback (recommended to keep set):
  export GEMINI_API_KEY=your_api_key_here

To verify OAuth is working, run:
  cargo test --workspace --all-targets --all-features
""")

# =============================================================================
# MAIN
# =============================================================================

def main():
    print("="*70)
    print("Gemini OAuth Setup for 3tched Architecture")
    print("="*70)
    
    secrets = load_client_secrets()
    print(f"\nLoaded OAuth client: {secrets['client_id']}")
    print(f"Project: codeassist-492619")
    
    # Check if localhost is a registered redirect
    localhost_registered = LOCALHOST_REDIRECT in secrets['redirect_uris']
    
    if localhost_registered:
        redirect_uri = LOCALHOST_REDIRECT
        print(f"\nlocalhost:{LOCALHOST_PORT} is registered. Starting auto-capture flow...")
        
        # Start server in background
        global server_running, auth_code
        server_running = True
        auth_code = None
        server_thread = threading.Thread(target=start_local_server, args=(LOCALHOST_PORT,))
        server_thread.daemon = True
        server_thread.start()
        
        auth_url = build_auth_url(secrets, redirect_uri)
        print(f"\nOpening authorization URL...\n{auth_url}\n")
        
        # Try to open browser
        os.system(f'xdg-open "{auth_url}" 2>/dev/null || open "{auth_url}" 2>/dev/null || echo "Please open the URL manually"')
        
        # Wait for code
        timeout = 120
        for i in range(timeout):
            if auth_code:
                break
            print(f"\rWaiting for authorization... {timeout - i}s remaining", end='', flush=True)
            time.sleep(1)
        print()
        
        server_running = False
        server_thread.join(timeout=2)
    else:
        print("\nNOTE: localhost is NOT in the registered redirect URIs.")
        print("Registered URIs:", secrets['redirect_uris'])
        print("\nOption 1: Add http://localhost:8085 to Google Cloud Console")
        print("Option 2: Use manual flow (visit one of your registered URIs)")
        
        # Fallback: manual flow
        print("\nFor manual flow, we need to use a registered redirect URI.")
        print("Visit this URL in a browser on a machine that can reach the redirect:")
        
        # Use the first registered redirect
        redirect_uri = secrets['redirect_uris'][0]
        auth_url = build_auth_url(secrets, redirect_uri)
        print(f"\n{auth_url}\n")
        print(f"After authorization, you'll be redirected to {redirect_uri}")
        print("Copy the 'code' parameter from the redirect URL and paste it below.\n")
        auth_code = input("Authorization code: ").strip()
    
    if not auth_code:
        print("ERROR: No authorization code received.")
        sys.exit(1)
    
    print(f"\nExchanging code for tokens...")
    tokens = exchange_code_for_tokens(secrets, auth_code, redirect_uri)
    
    if not tokens:
        print("\nFailed to get tokens. Common issues:")
        print("  - Redirect URI mismatch (must exactly match what's registered)")
        print("  - Code already used (codes are single-use)")
        print("  - Clock skew (ensure system time is correct)")
        sys.exit(1)
    
    if 'refresh_token' not in tokens:
        print("\nWARNING: No refresh_token in response!")
        print("This means you previously authorized this app.")
        print("To force a new refresh_token, revoke access at:")
        print("  https://myaccount.google.com/permissions")
        print("Then run this script again.")
        sys.exit(1)
    
    print(f"\nSuccess! Got access token (expires in {tokens.get('expires_in', 'unknown')}s)")
    print(f"Refresh token acquired.")
    
    save_adc(tokens, secrets)
    print_env_setup()
    
    # Verify the file
    print("\n" + "="*70)
    print("Verification")
    print("="*70)
    print(f"\nADC file contents (redacted):")
    with open(ADC_FILE) as f:
        adc = json.load(f)
    print(f"  type: {adc['type']}")
    print(f"  client_id: {adc['client_id'][:20]}...")
    print(f"  refresh_token: {adc['refresh_token'][:20]}...")
    print(f"\nThe GeminiClient in crates/op-llm/src/gemini.rs will now use OAuth mode")
    print(f"when GOOGLE_GENAI_USE_VERTEXAI=true is set.")

if __name__ == '__main__':
    main()
