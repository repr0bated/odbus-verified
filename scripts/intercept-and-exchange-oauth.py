#!/usr/bin/env python3
"""
Intercept Google Cloud Code OAuth flow and exchange code for tokens
"""
import http.server
import socketserver
import urllib.parse
import json
import requests
from datetime import datetime

# OAuth configuration from Cloud Code
CLIENT_ID = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com"
PORT = 42393
LOG_FILE = f"/tmp/cloudcode-oauth-{int(datetime.now().timestamp())}.json"

class OAuthHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        """Custom logging"""
        pass
    
    def do_GET(self):
        """Handle OAuth callback"""
        parsed = urllib.parse.urlparse(self.path)
        params = urllib.parse.parse_qs(parsed.query)
        
        print(f"\n{'='*60}")
        print(f"[{datetime.now().isoformat()}] OAuth Callback Received")
        print(f"{'='*60}")
        
        # Extract parameters
        auth_code = params.get('code', [None])[0]
        state = params.get('state', [None])[0]
        error = params.get('error', [None])[0]
        
        result = {
            'timestamp': datetime.now().isoformat(),
            'path': self.path,
            'params': {k: v[0] for k, v in params.items()},
            'headers': dict(self.headers)
        }
        
        if error:
            print(f"❌ Error: {error}")
            result['error'] = error
            self.send_error_response(f"OAuth Error: {error}")
        elif auth_code:
            print(f"✓ Authorization Code: {auth_code[:20]}...{auth_code[-20:]}")
            print(f"✓ State: {state}")
            
            # Try to exchange for tokens (will fail without client_secret, but shows the attempt)
            print(f"\n{'='*60}")
            print("Attempting token exchange...")
            print(f"{'='*60}")
            
            token_data = self.exchange_code_for_token(auth_code)
            result['token_exchange'] = token_data
            
            if token_data.get('access_token'):
                print(f"✓ Access Token: {token_data['access_token'][:30]}...")
                print(f"✓ Refresh Token: {token_data.get('refresh_token', 'N/A')[:30]}...")
                print(f"✓ Expires In: {token_data.get('expires_in')} seconds")
                print(f"✓ Scope: {token_data.get('scope')}")
            else:
                print(f"⚠ Token exchange failed (expected - need client_secret)")
                print(f"  Response: {json.dumps(token_data, indent=2)}")
            
            self.send_success_response(auth_code, token_data)
        else:
            print("⚠ No authorization code found")
            result['warning'] = 'No code parameter'
            self.send_error_response("No authorization code received")
        
        # Save to log file
        with open(LOG_FILE, 'w') as f:
            json.dump(result, f, indent=2)
        
        print(f"\n✓ Full details saved to: {LOG_FILE}")
        print(f"{'='*60}\n")
    
    def exchange_code_for_token(self, code):
        """Exchange authorization code for access token"""
        # Extract redirect_uri from the original request
        redirect_uri = f"http://localhost:{PORT}/oauth2redirect"
        
        # Note: This will fail without client_secret, but we can see the attempt
        token_url = "https://oauth2.googleapis.com/token"
        
        data = {
            'code': code,
            'client_id': CLIENT_ID,
            'redirect_uri': redirect_uri,
            'grant_type': 'authorization_code',
            # client_secret would go here, but Cloud Code uses PKCE
            # so it might work without it
        }
        
        try:
            response = requests.post(token_url, data=data, timeout=10)
            return response.json()
        except Exception as e:
            return {'error': str(e)}
    
    def send_success_response(self, code, token_data):
        """Send success HTML response"""
        has_token = 'access_token' in token_data
        
        html = f"""<!DOCTYPE html>
<html>
<head>
    <title>OAuth Intercepted</title>
    <style>
        body {{ font-family: monospace; padding: 40px; background: #1e1e1e; color: #d4d4d4; }}
        .success {{ color: #4ec9b0; }}
        .warning {{ color: #ce9178; }}
        .code {{ background: #2d2d2d; padding: 10px; border-radius: 4px; overflow-x: auto; }}
        h1 {{ color: #4ec9b0; }}
    </style>
</head>
<body>
    <h1>✓ OAuth Flow Intercepted!</h1>
    
    <h2>Authorization Code:</h2>
    <div class="code">{code[:50]}...{code[-50:]}</div>
    
    <h2 class="{'success' if has_token else 'warning'}">
        {'✓ Token Exchange Successful!' if has_token else '⚠ Token Exchange (Expected Failure)'}
    </h2>
    
    {'<div class="code">' + json.dumps(token_data, indent=2) + '</div>' if token_data else ''}
    
    <p>Check terminal for full details.</p>
    <p>Log saved to: <code>{LOG_FILE}</code></p>
    <p style="color: #858585;">You can close this window.</p>
</body>
</html>"""
        
        self.send_response(200)
        self.send_header('Content-type', 'text/html')
        self.end_headers()
        self.wfile.write(html.encode())
    
    def send_error_response(self, message):
        """Send error HTML response"""
        html = f"""<!DOCTYPE html>
<html>
<head>
    <title>OAuth Error</title>
    <style>
        body {{ font-family: monospace; padding: 40px; background: #1e1e1e; color: #d4d4d4; }}
        .error {{ color: #f48771; }}
    </style>
</head>
<body>
    <h1 class="error">❌ OAuth Error</h1>
    <p>{message}</p>
    <p>Check terminal for details.</p>
</body>
</html>"""
        
        self.send_response(200)
        self.send_header('Content-type', 'text/html')
        self.end_headers()
        self.wfile.write(html.encode())

def main():
    print(f"""
{'='*60}
Google Cloud Code OAuth Interceptor
{'='*60}
Project: geminidev-479406
Client ID: {CLIENT_ID}
Listening on: http://localhost:{PORT}
Log file: {LOG_FILE}

Waiting for OAuth callback...
Press Ctrl+C to stop
{'='*60}
""")
    
    with socketserver.TCPServer(("", PORT), OAuthHandler) as httpd:
        try:
            httpd.serve_forever()
        except KeyboardInterrupt:
            print("\n\n✓ Interceptor stopped")
            print(f"✓ Log saved to: {LOG_FILE}")

if __name__ == "__main__":
    main()
