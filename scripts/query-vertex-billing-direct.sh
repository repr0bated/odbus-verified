#!/bin/bash
set -euo pipefail

START_DATE="2025-12-23"
END_DATE=$(date +%Y-%m-%d)
BILLING_ACCOUNT="${1:-016F60-17F166-2427D3}"

echo "🔍 Querying Vertex AI billing charges"
echo "   Billing Account: $BILLING_ACCOUNT"
echo "   Date Range: $START_DATE to $END_DATE"
echo ""

echo "📋 Projects linked to billing account:"
PROJECTS=$(gcloud billing projects list --billing-account="$BILLING_ACCOUNT" --format="value(projectId)" 2>/dev/null || echo "")
if [ -z "$PROJECTS" ]; then
    echo "   ⚠️  No projects found or cannot access billing account"
    exit 1
fi

echo "$PROJECTS" | while read -r PROJECT; do
    echo "   - $PROJECT"
done
echo ""

echo "📊 Checking Vertex AI usage in projects..."
for PROJECT in $PROJECTS; do
    echo ""
    echo "=== Project: $PROJECT ==="
    if gcloud services list --project="$PROJECT" --enabled --filter="name:aiplatform.googleapis.com" --format="value(name)" 2>/dev/null | grep -q aiplatform; then
        echo "   ✅ Vertex AI API is enabled"
    else
        echo "   ⚠️  Vertex AI API not enabled"
    fi
done

echo ""
echo "💡 For detailed billing charges, visit:"
echo "   https://console.cloud.google.com/billing/$BILLING_ACCOUNT/reports"
echo "   Filter: Service = Vertex AI, Date = $START_DATE to $END_DATE"
