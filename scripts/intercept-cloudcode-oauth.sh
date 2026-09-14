#!/bin/bash
# Intercept Google Cloud Code OAuth callback

PORT=42393
LOG_FILE="/tmp/cloudcode-oauth-$(date +%s).log"

echo "=== Cloud Code OAuth Interceptor ==="
echo "Listening on http://localhost:$PORT"
echo "Logging to: $LOG_FILE"
echo ""
echo "Now trigger the OAuth flow in VSCode Cloud Code..."
echo "Press Ctrl+C to stop"
echo ""

# Start netcat listener that captures the callback
while true; do
    {
        # Read the HTTP request
        REQUEST=$(timeout 1 cat)
        
        if [[ -n "$REQUEST" ]]; then
            echo "=== CAPTURED REQUEST ===" | tee -a "$LOG_FILE"
            echo "$REQUEST" | tee -a "$LOG_FILE"
            echo "" | tee -a "$LOG_FILE"
            
            # Extract the authorization code
            CODE=$(echo "$REQUEST" | grep -oP 'code=\K[^&\s]+' | head -1)
            STATE=$(echo "$REQUEST" | grep -oP 'state=\K[^&\s]+' | head -1)
            
            if [[ -n "$CODE" ]]; then
                echo "✓ Authorization Code: $CODE" | tee -a "$LOG_FILE"
                echo "✓ State: $STATE" | tee -a "$LOG_FILE"
                echo "" | tee -a "$LOG_FILE"
                
                # Send success response
                cat <<EOF
HTTP/1.1 200 OK
Content-Type: text/html
Connection: close

<!DOCTYPE html>
<html>
<head><title>OAuth Intercepted</title></head>
<body>
<h1>✓ OAuth Flow Intercepted!</h1>
<p>Authorization code captured. Check terminal for details.</p>
<p>You can close this window.</p>
</body>
</html>
EOF
            else
                # Send response anyway
                cat <<EOF
HTTP/1.1 200 OK
Content-Type: text/html
Connection: close

<!DOCTYPE html>
<html>
<head><title>Request Captured</title></head>
<body>
<h1>Request Captured</h1>
<p>Check terminal for details.</p>
</body>
</html>
EOF
            fi
        fi
    } | nc -l -p $PORT -q 1
    
    # Small delay before next listen
    sleep 0.1
done
