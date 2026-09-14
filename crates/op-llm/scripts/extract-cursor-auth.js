#!/usr/bin/env node

/**
 * Extract Cursor Agent Authentication
 * 
 * Extracts authentication tokens from Cursor agent configuration
 * so CLI can use the same enterprise authentication
 */

const fs = require('fs');
const path = require('path');
const os = require('os');

function extractCursorAuth() {
  console.log('🔍 Extracting Cursor Agent Authentication...');
  
  const homeDir = os.homedir();
  const cursorConfigDir = path.join(homeDir, '.cursor');
  const cursorAuthFile = path.join(cursorConfigDir, 'auth.json');
  const cursorConfigFile = path.join(cursorConfigDir, 'config.json');
  
  let cursorAuth = {};
  let cursorConfig = {};
  
  // Try to read Cursor auth file
  if (fs.existsSync(cursorAuthFile)) {
    try {
      cursorAuth = JSON.parse(fs.readFileSync(cursorAuthFile, 'utf8'));
      console.log('✅ Found Cursor auth file');
    } catch (e) {
      console.log('⚠️  Could not parse Cursor auth file');
    }
  } else {
    console.log('⚠️  No Cursor auth file found');
  }
  
  // Try to read Cursor config file
  if (fs.existsSync(cursorConfigFile)) {
    try {
      cursorConfig = JSON.parse(fs.readFileSync(cursorConfigFile, 'utf8'));
      console.log('✅ Found Cursor config file');
    } catch (e) {
      console.log('⚠️  Could not parse Cursor config file');
    }
  } else {
    console.log('⚠️  No Cursor config file found');
  }
  
  // Extract enterprise authentication data
  const enterpriseData = {
    timestamp: new Date().toISOString(),
    source: 'cursor_agent_extraction',
    enterprise: {
      project: cursorConfig?.enterprise?.project || cursorAuth?.enterprise?.project,
      token: cursorAuth?.enterprise?.token || cursorAuth?.enterprise?.auth_token,
      refresh_token: cursorAuth?.enterprise?.refresh_token,
      expires_at: cursorAuth?.enterprise?.expires_at
    },
    gcp: {
      project_id: cursorConfig?.gcp?.project_id || cursorAuth?.gcp?.project_id,
      access_token: cursorAuth?.gcp?.access_token,
      service_account: cursorAuth?.gcp?.service_account
    },
    vertex: {
      endpoint: cursorConfig?.vertex?.endpoint,
      auth_mode: cursorConfig?.vertex?.auth_mode || 'enterprise'
    }
  };
  
  // Look for additional auth data in Cursor directory
  const cursorFiles = fs.readdirSync(cursorConfigDir).filter(file => 
    file.includes('auth') || file.includes('token') || file.includes('enterprise')
  );
  
  enterpriseData.cursor_files = cursorFiles;
  
  // Extract tokens from all potential locations
  const allTokens = {};
  
  // From auth.json
  extractTokens(cursorAuth, 'auth_json', allTokens);
  
  // From config.json  
  extractTokens(cursorConfig, 'config_json', allTokens);
  
  // From other Cursor files
  cursorFiles.forEach(file => {
    const filePath = path.join(cursorConfigDir, file);
    try {
      const content = JSON.parse(fs.readFileSync(filePath, 'utf8'));
      extractTokens(content, file, allTokens);
    } catch (e) {
      // Skip non-JSON files
    }
  });
  
  enterpriseData.all_tokens = allTokens;
  
  // Save extracted data for CLI to use
  const outputFile = '/etc/op-dbus/cursor-agent-auth.json';
  fs.writeFileSync(outputFile, JSON.stringify(enterpriseData, null, 2));
  console.log(`💾 Saved extracted Cursor auth to: ${outputFile}`);
  
  // Create environment script for CLI
  const envScript = `/etc/op-dbus/cursor-agent-env.sh`;
  const envContent = `
# Cursor Agent Authentication Environment
export CURSOR_AGENT_MODE=true
export ENTERPRISE_AUTH_TOKEN="${enterpriseData.enterprise.token || ''}"
export GCP_PROJECT_ID="${enterpriseData.enterprise.project || enterpriseData.gcp.project_id || ''}"
export VERTEX_ACCESS_TOKEN="${enterpriseData.gcp.access_token || ''}"
export CURSOR_ENTERPRISE_PROJECT="${enterpriseData.enterprise.project || ''}"
export SPOOFED_AS_CURSOR_AGENT=true

echo "🎯 CLI now using Cursor Agent authentication"
echo "Project: $GCP_PROJECT_ID"
echo "Enterprise Token: ${ENTERPRISE_AUTH_TOKEN:0:20}..."
`;
  
  fs.writeFileSync(envScript, envContent);
  console.log(`🔧 Created environment script: ${envScript}`);
  
  console.log('\n🎭 CLI ↔ Cursor Agent Spoofing Complete!');
  console.log('📋 To use:');
  console.log(`   source ${envScript}`);
  console.log('   sudo systemctl restart op-web');
  
  return enterpriseData;
}

function extractTokens(obj, source, tokenMap) {
  if (!obj || typeof obj !== 'object') return;
  
  for (const [key, value] of Object.entries(obj)) {
    if (typeof value === 'string' && isTokenLike(key, value)) {
      if (!tokenMap[key]) tokenMap[key] = [];
      tokenMap[key].push({
        value: value,
        source: source,
        category: categorizeToken(key, value)
      });
    } else if (typeof value === 'object') {
      extractTokens(value, source, tokenMap);
    }
  }
}

function isTokenLike(key, value) {
  const keyLower = key.toLowerCase();
  const valueLower = value.toLowerCase();
  
  // Key patterns
  if (keyLower.includes('token') || 
      keyLower.includes('auth') || 
      keyLower.includes('secret') ||
      keyLower.includes('key') ||
      keyLower.includes('credential')) {
    return true;
  }
  
  // Value patterns (JWT-like)
  if (value.includes('.') && value.split('.').length === 3) {
    return true;
  }
  
  // Long random strings
  if (value.length > 20 && /^[A-Za-z0-9+/=_-]+$/.test(value)) {
    return true;
  }
  
  return false;
}

function categorizeToken(key, value) {
  const keyLower = key.toLowerCase();
  
  if (keyLower.includes('access')) return 'access_token';
  if (keyLower.includes('refresh')) return 'refresh_token';
  if (keyLower.includes('id')) return 'id_token';
  if (keyLower.includes('bearer')) return 'bearer_token';
  if (keyLower.includes('enterprise')) return 'enterprise_token';
  if (keyLower.includes('vertex')) return 'vertex_token';
  if (value.split('.').length === 3) return 'jwt';
  
  return 'unknown';
}

// Run extraction
try {
  const result = extractCursorAuth();
  console.log(`🔑 Extracted ${Object.keys(result.all_tokens).length} token types`);
} catch (error) {
  console.error('❌ Extraction failed:', error.message);
  process.exit(1);
}
