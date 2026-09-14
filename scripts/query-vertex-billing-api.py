#!/usr/bin/env python3
"""
Query Vertex AI billing charges from GCP using the Cloud Billing API.
Requires: pip install google-cloud-billing google-auth
"""

import os
import sys
from datetime import datetime, timedelta
from google.cloud import billing_v1
from google.auth import default
import json

def query_vertex_billing(billing_account_id: str, start_date: str, end_date: str):
    """Query Vertex AI charges from Cloud Billing API."""
    
    # Initialize the billing client
    try:
        client = billing_v1.CloudBillingClient()
    except Exception as e:
        print(f"❌ Error initializing billing client: {e}")
        print("   Make sure you have google-cloud-billing installed: pip install google-cloud-billing")
        return
    
    # Convert dates
    start = datetime.strptime(start_date, "%Y-%m-%d")
    end = datetime.strptime(end_date, "%Y-%m-%d")
    
    print(f"🔍 Querying Vertex AI billing charges")
    print(f"   Billing Account: {billing_account_id}")
    print(f"   Date Range: {start_date} to {end_date}")
    print()
    
    # Get projects linked to billing account
    try:
        project_client = billing_v1.CloudBillingClient()
        parent = f"billingAccounts/{billing_account_id}"
        
        # List projects
        print("📋 Projects linked to billing account:")
        projects = []
        try:
            # Note: This API might not be available in all cases
            # We'll try alternative methods
            pass
        except Exception as e:
            print(f"   Note: Could not list projects via API: {e}")
        
        # Alternative: Use gcloud to get projects
        import subprocess
        result = subprocess.run(
            ["gcloud", "billing", "projects", "list", 
             "--billing-account", billing_account_id,
             "--format", "value(projectId)"],
            capture_output=True,
            text=True
        )
        if result.returncode == 0:
            projects = [p.strip() for p in result.stdout.strip().split("\n") if p.strip()]
            for proj in projects:
                print(f"   - {proj}")
        print()
        
    except Exception as e:
        print(f"⚠️  Warning: {e}")
    
    # Query using Cloud Billing Reports API
    print("📊 Querying billing data...")
    print("   Note: Detailed billing queries require BigQuery export or Cloud Console.")
    print("   The Cloud Billing API has limited query capabilities.")
    print()
    
    # Try to use the Cost Table API if available
    try:
        # This requires the billing.accounts.getBillingInfo permission
        billing_account_name = f"billingAccounts/{billing_account_id}"
        account = client.get_billing_account(name=billing_account_name)
        print(f"✅ Billing account found: {account.display_name}")
        print(f"   Open: {account.open}")
        print()
    except Exception as e:
        print(f"⚠️  Could not access billing account details: {e}")
        print()
    
    print("💡 For detailed Vertex AI charges, use one of these methods:")
    print()
    print("1. BigQuery Billing Export (Recommended):")
    print("   - Set up at: https://console.cloud.google.com/billing/{}/exports".format(billing_account_id))
    print("   - Then query with SQL (see query-vertex-billing.sh)")
    print()
    print("2. Cloud Console:")
    print("   - Visit: https://console.cloud.google.com/billing/{}/reports".format(billing_account_id))
    print("   - Filter: Service = 'Vertex AI'")
    print("   - Date range: {} to {}".format(start_date, end_date))
    print("   - Export to CSV")
    print()
    print("3. gcloud CLI (if billing export configured):")
    print("   gcloud bq query --use_legacy_sql=false \\")
    print("     'SELECT * FROM `PROJECT.DATASET.gcp_billing_export_v1_*`")
    print("      WHERE service.description LIKE \"%Vertex%\"")
    print("      AND _TABLE_SUFFIX BETWEEN \"20251223\" AND \"{}\"'".format(
        datetime.now().strftime("%Y%m%d")
    ))

if __name__ == "__main__":
    billing_account = sys.argv[1] if len(sys.argv) > 1 else "016C53-111148-0513AB"
    start_date = sys.argv[2] if len(sys.argv) > 2 else "2025-12-23"
    end_date = sys.argv[3] if len(sys.argv) > 3 else datetime.now().strftime("%Y-%m-%d")
    
    query_vertex_billing(billing_account, start_date, end_date)
