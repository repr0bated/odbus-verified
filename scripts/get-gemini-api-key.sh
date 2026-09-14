#!/bin/bash
set -euo pipefail

# Get Gemini API key for use with Antigravity proxy

PROJECT_ID="geminidev-479406"

echo "=== Gemini API Key Setup ==="
echo "Project: $PROJECT_ID"
echo ""

# Check if gcloud is installed
if ! command -v gcloud &> /dev/null; then
    echo "❌ gcloud CLI not found"
    echo ""
    echo "Install it from: https://cloud.google.com/sdk/docs/install"
    echo ""
    echo "Or use Google AI Studio (easier):"
    echo "  1. Visit: https://aistudio.google.com/app/apikey"
    echo "  2. Click 'Create API Key'"
    echo "  3. Copy the key and add it to Antigravity"
    exit 1
fi

echo "Choose API key method:"
echo ""
echo "1. Google AI Studio (Recommended - Free tier available)"
echo "   - Visit: https://aistudio.google.com/app/apikey"
echo "   - No GCP project needed"
echo "   - Instant setup"
echo ""
echo "2. Vertex AI (GCP Project required)"
echo "   - Uses project: $PROJECT_ID"
echo "   - Better for production"
echo "   - More control"
echo ""
read -p "Choose (1 or 2): " choice

if [[ "$choice" == "1" ]]; then
    echo ""
    echo "=== Google AI Studio Setup ==="
    echo ""
    echo "1. Open: https://aistudio.google.com/app/apikey"
    echo "2. Click 'Create API Key'"
    echo "3. Copy the API key"
    echo ""
    echo "Then in Antigravity:"
    echo "  - Provider Type: Google Gemini / GLM"
    echo "  - API Key: [paste your key]"
    echo "  - Endpoint: https://generativelanguage.googleapis.com/v1beta"
    echo ""
    
elif [[ "$choice" == "2" ]]; then
    echo ""
    echo "=== Vertex AI Setup ==="
    echo ""
    
    # Check if logged in
    if ! gcloud auth list --filter=status:ACTIVE --format="value(account)" &> /dev/null; then
        echo "Logging in to GCP..."
        gcloud auth login
    fi
    
    # Set project
    echo "Setting project to $PROJECT_ID..."
    gcloud config set project "$PROJECT_ID"
    
    # Enable APIs
    echo "Enabling required APIs..."
    gcloud services enable aiplatform.googleapis.com \
        generativelanguage.googleapis.com \
        --project="$PROJECT_ID"
    
    # Create service account
    SA_NAME="antigravity-gemini"
    SA_EMAIL="${SA_NAME}@${PROJECT_ID}.iam.gserviceaccount.com"
    
    echo "Creating service account: $SA_NAME..."
    if gcloud iam service-accounts describe "$SA_EMAIL" &> /dev/null; then
        echo "✓ Service account already exists"
    else
        gcloud iam service-accounts create "$SA_NAME" \
            --display-name="Antigravity Gemini API" \
            --project="$PROJECT_ID"
    fi
    
    # Grant permissions
    echo "Granting permissions..."
    gcloud projects add-iam-policy-binding "$PROJECT_ID" \
        --member="serviceAccount:$SA_EMAIL" \
        --role="roles/aiplatform.user" \
        --condition=None
    
    # Create key
    KEY_FILE="$HOME/.config/antigravity/gemini-key.json"
    mkdir -p "$(dirname "$KEY_FILE")"
    
    echo "Creating service account key..."
    gcloud iam service-accounts keys create "$KEY_FILE" \
        --iam-account="$SA_EMAIL"
    
    echo ""
    echo "✓ Setup complete!"
    echo ""
    echo "Service Account: $SA_EMAIL"
    echo "Key File: $KEY_FILE"
    echo ""
    echo "Configure Antigravity:"
    echo "  - Provider Type: Google Vertex AI"
    echo "  - Project ID: $PROJECT_ID"
    echo "  - Service Account Key: $KEY_FILE"
    echo "  - Region: us-central1"
    echo "  - Endpoint: https://us-central1-aiplatform.googleapis.com/v1"
    echo ""
    
    # Test the setup
    echo "Testing API access..."
    gcloud auth activate-service-account --key-file="$KEY_FILE"
    
    if gcloud ai models list --region=us-central1 --project="$PROJECT_ID" &> /dev/null; then
        echo "✓ API access verified!"
    else
        echo "⚠ Could not verify API access. Check permissions."
    fi
    
else
    echo "Invalid choice"
    exit 1
fi

echo ""
echo "=== Next Steps ==="
echo ""
echo "1. Add the API key/credentials to Antigravity"
echo "2. Click 'Start Service' in Antigravity"
echo "3. Test with: ./scripts/quick-test-gemini-proxy.sh"
echo ""
