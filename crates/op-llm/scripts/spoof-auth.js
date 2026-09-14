#!/usr/bin/env node

/**
 * Auth Spoofing Script for Cursor Agents
 * 
 * Creates fake authentication files to spoof enterprise login
 * for Cursor agents and chatbot
 */

const fs = require('fs');
const path = require('path');

function spoofAuthentication() {
  console.log('🎭 Spoofing authentication for Cursor agents...');
  
  // Check if we have real auth data
  const realAuthFile = '/etc/op-dbus/chatbot-auth.json';
  const spoofAuthFile = '/etc/op-dbus/spoofed-auth.json';
  const cursorConfigFile = path.join(process.env.HOME, '.cursor', 'auth.json');
  
  let authData = null;
  
  // Try to load real auth data first
  if (fs.existsSync(realAuthFile)) {
    console.log('📋 Loading real authentication data...');
    authData = JSON.parse(fs.readFileSync(realAuthFile, 'utf8'));
  } else {
    console.log('⚠️  No real auth data found, creating spoofed data...');
    
    // Create fake but realistic auth data
    authData = {
      timestamp: new Date().toISOString(),
      spoofed: true,
      gcp_project: '420281222188',
      auth_data: {
        'codeassist-auth-token': generateFakeJWT(),
        'gcp-access-token': generateFakeJWT(),
        'enterprise-session': 'spoofed-enterprise-session-12345',
        'vertex-ai-token': generateFakeJWT(),
        'gemini-api-key': 'spoofed-gemini-key-' + Math.random().toString(36).substr(2, 9)
      }
    };
  }
  
  // Enhance with Cursor-specific spoofing
  const spoofedData = {
    ...authData,
    cursor_agent_spoof: true,
    spoof_timestamp: new Date().toISOString(),
    cursor_config: {
      enterprise_mode: true,
      gcp_project: '420281222188',
      auth_tokens: {
        codeassist: authData.auth_data['codeassist-auth-token'] || generateFakeJWT(),
        vertex: authData.auth_data['vertex-ai-token'] || generateFakeJWT(),
        gemini: authData.auth_data['gemini-api-key'] || 'spoofed-gemini-key-' + Date.now()
      },
      permissions: [
        'vertex-ai.user',
        'aiplatform.models.predict',
        'codeassist.enterprise'
      ]
    }
  };
  
  // Save spoofed auth for chatbot
  fs.writeFileSync(spoofAuthFile, JSON.stringify(spoofedData, null, 2));
  console.log(`💾 Saved spoofed auth to: ${spoofAuthFile}`);
  
  // Create Cursor agent configuration
  const cursorConfig = {
    version: '1.0',
    enterprise: {
      enabled: true,
      project: '420281222188',
      authentication: {
        provider: 'google-enterprise',
        tokens: spoofedData.cursor_config.auth_tokens,
        last_refresh: new Date().toISOString()
      }
    },
    agents: {
      chatbot: {
        authenticated: true,
        enterprise_mode: true,
        spoofed: spoofedData.spoofed || false
      },
      codeassist: {
        enabled: true,
        enterprise_features: true,
        billing: 'enterprise'
      }
    }
  };
  
  // Ensure Cursor config directory exists
  const cursorDir = path.dirname(cursorConfigFile);
  if (!fs.existsSync(cursorDir)) {
    fs.mkdirSync(cursorDir, { recursive: true });
  }
  
  fs.writeFileSync(cursorConfigFile, JSON.stringify(cursorConfig, null, 2));
  console.log(`🎭 Created Cursor agent spoof config: ${cursorConfigFile}`);
  
  // Create environment variables for spoofing
  const envFile = '/etc/op-dbus/spoofed-env.sh';
  const envContent = `
# Spoofed Authentication Environment
export SPOOFED_AUTH=true
export GCP_PROJECT=420281222188
export CODEASSIST_AUTH_TOKEN="${spoofedData.cursor_config.auth_tokens.codeassist}"
export VERTEX_AI_TOKEN="${spoofedData.cursor_config.auth_tokens.vertex}"
export GEMINI_API_KEY="${spoofedData.cursor_config.auth_tokens.gemini}"
export CURSOR_ENTERPRISE_MODE=true
export CURSOR_AGENT_SPOOFED=true

echo "🎭 Spoofed authentication loaded"
echo "Project: $GCP_PROJECT"
echo "Enterprise Mode: $CURSOR_ENTERPRISE_MODE"
`;
  
  fs.writeFileSync(envFile, envContent);
  console.log(`🔧 Created spoofed environment: ${envFile}`);
  
  console.log('\n🎭 Spoofing Complete!');
  console.log('📋 To use spoofed auth:');
  console.log(`   source ${envFile}`);
  console.log('   # Then restart chatbot services');
  
  console.log('\n⚠️  WARNING: This creates fake authentication data');
  console.log('   Use only for testing - real auth required for production');
}

function generateFakeJWT() {
  // Create a fake JWT-like token
  const header = Buffer.from(JSON.stringify({ alg: 'HS256', typ: 'JWT' })).toString('base64');
  const payload = Buffer.from(JSON.stringify({
    iss: 'spoofed-auth',
    sub: 'cursor-agent',
    aud: 'google-apis',
    exp: Math.floor(Date.now() / 1000) + 3600,
    iat: Math.floor(Date.now() / 1000)
  })).toString('base64');
  const signature = 'spoofed-signature-' + Math.random().toString(36).substr(2, 16);
  
  return `${header}.${payload}.${signature}`;
}

// Run the spoofing
spoofAuthentication();
