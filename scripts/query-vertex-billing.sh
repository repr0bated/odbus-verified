#!/bin/bash
set -euo pipefail

# Query Vertex AI billing charges from 12/23/25 until now
# Usage: ./scripts/query-vertex-billing.sh [BILLING_ACCOUNT_ID]

START_DATE="2025-12-23"
END_DATE=$(date +%Y-%m-%d)
BILLING_ACCOUNT="${1:-016C53-111148-0513AB}"  # Default to first account if not provided

echo "🔍 Querying Vertex AI billing charges"
echo "   Billing Account: $BILLING_ACCOUNT"
echo "   Date Range: $START_DATE to $END_DATE"
echo ""

# Method 1: Try BigQuery billing export (if configured)
echo "📊 Method 1: Checking BigQuery billing export..."
if gcloud billing accounts describe "$BILLING_ACCOUNT" --format="get(billingAccountName)" &>/dev/null; then
    # Try to find BigQuery dataset
    PROJECTS=$(gcloud projects list --format="value(projectId)" 2>/dev/null || echo "")
    
    if [ -n "$PROJECTS" ]; then
        for PROJECT in $PROJECTS; do
            DATASET=$(gcloud bq datasets list --project="$PROJECT" --format="value(datasetId)" 2>/dev/null | grep -i "billing" | head -1 || echo "")
            if [ -n "$DATASET" ]; then
                echo "   Found billing dataset: $PROJECT.$DATASET"
                echo ""
                echo "   Running BigQuery query..."
                gcloud bq query --use_legacy_sql=false --format=prettyjson --project="$PROJECT" <<EOF
SELECT 
  usage_start_time,
  service.description as service_name,
  sku.description as sku_description,
  location.location as region,
  cost,
  currency,
  usage.amount as usage_amount,
  usage.unit as usage_unit,
  project.name as project_name
FROM \`$PROJECT.$DATASET.gcp_billing_export_v1_*\`
WHERE 
  _TABLE_SUFFIX BETWEEN FORMAT_DATE('%Y%m%d', DATE('$START_DATE')) 
                    AND FORMAT_DATE('%Y%m%d', DATE('$END_DATE'))
  AND service.description LIKE '%Vertex%'
  AND cost > 0
ORDER BY usage_start_time DESC
LIMIT 1000
EOF
                echo ""
                echo "   Summary query..."
                gcloud bq query --use_legacy_sql=false --format=prettyjson --project="$PROJECT" <<EOF
SELECT 
  service.description as service_name,
  sku.description as sku_description,
  SUM(cost) as total_cost,
  currency,
  COUNT(*) as charge_count
FROM \`$PROJECT.$DATASET.gcp_billing_export_v1_*\`
WHERE 
  _TABLE_SUFFIX BETWEEN FORMAT_DATE('%Y%m%d', DATE('$START_DATE')) 
                    AND FORMAT_DATE('%Y%m%d', DATE('$END_DATE'))
  AND service.description LIKE '%Vertex%'
  AND cost > 0
GROUP BY service_name, sku_description, currency
ORDER BY total_cost DESC
EOF
                exit 0
            fi
        done
    fi
fi

# Method 2: Use Cloud Billing API directly
echo "📊 Method 2: Using Cloud Billing API..."
echo "   Note: This requires billing account access and may have limitations"
echo ""

# Get projects linked to billing account
echo "   Projects linked to billing account:"
gcloud billing projects list --billing-account="$BILLING_ACCOUNT" --format="table(projectId,billingAccountName)" 2>/dev/null || echo "   Could not list projects"
echo ""

# Method 3: Use gcloud billing budgets or cost breakdown
echo "📊 Method 3: Checking cost breakdown via Cloud Console export..."
echo ""
echo "   To get detailed Vertex AI charges, you can:"
echo "   1. Visit: https://console.cloud.google.com/billing/$BILLING_ACCOUNT/reports"
echo "   2. Filter by service: 'Vertex AI'"
echo "   3. Date range: $START_DATE to $END_DATE"
echo "   4. Export to CSV"
echo ""

# Method 4: Try using Cloud Asset Inventory (if available)
echo "📊 Method 4: Checking Cloud Asset Inventory..."
PROJECTS=$(gcloud billing projects list --billing-account="$BILLING_ACCOUNT" --format="value(projectId)" 2>/dev/null | head -5 || echo "")
if [ -n "$PROJECTS" ]; then
    for PROJECT in $PROJECTS; do
        echo "   Checking project: $PROJECT"
        # List Vertex AI resources
        gcloud asset search-all-resources \
            --project="$PROJECT" \
            --asset-types="aiplatform.googleapis.com/*" \
            --format="table(name,assetType,location)" 2>/dev/null || true
    done
fi

echo ""
echo "💡 Tip: For the most accurate billing data, use BigQuery billing export."
echo "   Set it up at: https://console.cloud.google.com/billing/$BILLING_ACCOUNT/exports"
