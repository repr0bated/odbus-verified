#!/usr/bin/env node

/**
 * Puppeteer Login Recording Script
 * 
 * Records the actual login flow for authentic authentication
 * Run this while you manually log into Code Assist
 */

const puppeteer = require('puppeteer');
const fs = require('fs');

async function recordLogin() {
  console.log('🎬 Starting login recording session...');
  console.log('📋 Please manually complete the login process in the browser');
  console.log('⏹️  Press Ctrl+C when done to save the authentication data');
  
  const browser = await puppeteer.launch({
    headless: false, // Show browser so user can interact
    args: ['--no-sandbox', '--disable-setuid-sandbox'],
    defaultViewport: { width: 1200, height: 800 }
  });
  
  const page = await browser.newPage();
  
  // Record all actions and data
  const recordedData = {
    timestamp: new Date().toISOString(),
    actions: [],
    finalAuthData: null,
    cookies: [],
    localStorage: {},
    sessionStorage: {}
  };
  
  // Record navigation
  page.on('framenavigated', frame => {
    if (frame === page.mainFrame()) {
      recordedData.actions.push({
        type: 'navigation',
        url: frame.url(),
        timestamp: new Date().toISOString()
      });
    }
  });
  
  // Record clicks
  page.on('click', () => {
    recordedData.actions.push({
      type: 'click',
      timestamp: new Date().toISOString(),
      url: page.url()
    });
  });
  
  // Record input changes
  page.on('input', () => {
    recordedData.actions.push({
      type: 'input',
      timestamp: new Date().toISOString(),
      url: page.url()
    });
  });
  
  console.log('🌐 Opening Code Assist login page...');
  await page.goto('https://codeassist.google.com');
  
  console.log('🎯 Please complete your login process manually:');
  console.log('   1. Click Enterprise/Organization login');
  console.log('   2. Enter your credentials');
  console.log('   3. Complete 2FA if required');
  console.log('   4. Wait for dashboard to load');
  console.log('   5. Press Ctrl+C to finish recording');
  
  // Wait for user to complete login
  process.on('SIGINT', async () => {
    console.log('\n⏹️  Recording stopped, extracting authentication data...');
    
    try {
      // Extract final authentication state
      recordedData.finalAuthData = await page.evaluate(() => {
        const data = {
          url: window.location.href,
          title: document.title,
          cookies: document.cookie,
          localStorage: {},
          sessionStorage: {}
        };
        
        // Get all localStorage
        for (let i = 0; i < localStorage.length; i++) {
          const key = localStorage.key(i);
          data.localStorage[key] = localStorage.getItem(key);
        }
        
        // Get all sessionStorage  
        for (let i = 0; i < sessionStorage.length; i++) {
          const key = sessionStorage.key(i);
          data.sessionStorage[key] = sessionStorage.getItem(key);
        }
        
        return data;
      });
      
      // Get cookies
      recordedData.cookies = await page.cookies();
      
      // Save recording
      const recordingFile = '/etc/op-dbus/login-recording.json';
      fs.writeFileSync(recordingFile, JSON.stringify(recordedData, null, 2));
      console.log(`💾 Saved login recording to: ${recordingFile}`);
      
      // Extract useful auth tokens
      const authTokens = extractAuthTokens(recordedData);
      if (authTokens.length > 0) {
        const authFile = '/etc/op-dbus/chatbot-auth.json';
        fs.writeFileSync(authFile, JSON.stringify({
          timestamp: new Date().toISOString(),
          source: 'recorded_login',
          auth_tokens: authTokens,
          gcp_project: '420281222188'
        }, null, 2));
        console.log(`🔑 Extracted ${authTokens.length} auth tokens to: ${authFile}`);
      }
      
    } catch (error) {
      console.error('❌ Error extracting data:', error.message);
    }
    
    await browser.close();
    process.exit(0);
  });
}

function extractAuthTokens(recording) {
  const tokens = [];
  
  if (recording.finalAuthData) {
    const { localStorage, sessionStorage, cookies } = recording.finalAuthData;
    
    // Look for auth tokens in localStorage
    Object.entries(localStorage).forEach(([key, value]) => {
      if (isAuthToken(key, value)) {
        tokens.push({
          type: 'localStorage',
          key,
          value,
          category: categorizeToken(key, value)
        });
      }
    });
    
    // Look for auth tokens in sessionStorage
    Object.entries(sessionStorage).forEach(([key, value]) => {
      if (isAuthToken(key, value)) {
        tokens.push({
          type: 'sessionStorage', 
          key,
          value,
          category: categorizeToken(key, value)
        });
      }
    });
    
    // Look for auth tokens in cookies
    cookies.split(';').forEach(cookie => {
      const [key, value] = cookie.trim().split('=');
      if (key && value && isAuthToken(key, value)) {
        tokens.push({
          type: 'cookie',
          key,
          value,
          category: categorizeToken(key, value)
        });
      }
    });
  }
  
  return tokens;
}

function isAuthToken(key, value) {
  if (!key || !value) return false;
  
  const keyLower = key.toLowerCase();
  const valueLower = value.toLowerCase();
  
  // Check key patterns
  if (keyLower.includes('token') || 
      keyLower.includes('auth') || 
      keyLower.includes('access') ||
      keyLower.includes('bearer') ||
      keyLower.includes('jwt')) {
    return true;
  }
  
  // Check value patterns (JWT tokens have dots)
  if (value.includes('.') && value.split('.').length === 3) {
    return true;
  }
  
  // Check for long random strings that might be tokens
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
  if (value.split('.').length === 3) return 'jwt';
  if (keyLower.includes('session')) return 'session';
  
  return 'unknown';
}

// Handle graceful shutdown
process.on('SIGTERM', () => {
  console.log('\n👋 Recording terminated');
  process.exit(0);
});

recordLogin().catch(console.error);
