# Manual Steps to Add MX Record for 3tched.com

The Cloudflare API won't let us disable Email Routing programmatically. You need to do this via the dashboard.

## Step-by-Step Instructions

### 1. Go to Cloudflare Dashboard
Open: https://dash.cloudflare.com/

### 2. Select 3tched.com Domain
Click on the `3tched.com` domain from your list

### 3. Navigate to Email Routing
- Click on **"Email"** in the left sidebar
- Or click on **"Email Routing"**

### 4. Disable Email Routing
- You'll see "Email Routing is enabled"
- Click the **"Disable Email Routing"** button
- Confirm when prompted

### 5. Delete Old MX Records (if they don't auto-delete)
- Go to **DNS** → **Records**
- Look for MX records pointing to:
  - `route1.mx.cloudflare.net`
  - `route2.mx.cloudflare.net`
  - `route3.mx.cloudflare.net`
- Delete all three of these records

### 6. Add New MX Record
Click **"Add record"** and enter:
- **Type**: MX
- **Name**: @ (or leave blank for root domain)
- **Mail server**: mail.3tched.com
- **Priority**: 10
- **TTL**: Auto (or 3600)

Click **Save**

### 7. Verify
After 1-2 minutes, run:
```bash
dig MX 3tched.com +short
```

You should see:
```
10 mail.3tched.com.
```

## Alternative: Run Script Again
After disabling Email Routing in the dashboard, run:
```bash
./scripts/update-dns-3tched.sh
```

This will automatically add the MX record.

## Why This Happened
Cloudflare Email Routing is a free email forwarding service that automatically manages MX records. Once it's enabled, the API prevents modifications to protect the service. You must disable it through the dashboard first.
