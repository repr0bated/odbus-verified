#!/usr/bin/env node

/**
 * Puppeteer Authentication Script for Chatbot
 * 
 * Automates Gemini Code Assist enterprise login and extracts auth tokens
 */

const puppeteer = require('puppeteer');

async function authenticateChatbot() {
  console.log('🚀 Starting Puppeteer authentication for chatbot...');
  
  const browser = await puppeteer.launch({
    headless: true,
    args: ['--no-sandbox', '--disable-setuid-sandbox']
  });
  
  try {
    const page = await browser.newPage();
    page.on('console', msg => console.log('PAGE:', msg.text()));
    
    console.log('📱 Navigating to Code Assist...');
    await page.goto('https://codeassist.google.com', { waitUntil: 'networkidle2' });
    
    // Look for enterprise/project setup
    console.log('🔍 Looking for enterprise authentication...');
    
    // Try to find and interact with enterprise login
    try {
      await page.waitForSelector('button:has-text("Enterprise")', { timeout: 5000 });
      await page.click('button:has-text("Enterprise")');
      console.log('✅ Clicked Enterprise login');
    } catch (e) {
      console.log('⚠️  Enterprise button not found, trying project input...');
    }
    
    // Look for project input
    try {
      const projectInput = await page.$('input[placeholder*="project" i]');
      if (projectInput) {
        await projectInput.type('420281222188');
        console.log('✅ Entered GCP project: 420281222188');
        
        // Look for submit/authenticate button
        const submitBtn = await page.$('button:has-text("Authenticate")') || 
                         await page.$('button:has-text("Connect")') ||
                         await page.$('button:has-text("Submit")');
        if (submitBtn) {
          await submitBtn.click();
          console.log('✅ Submitted authentication');
        }
      }
    } catch (e) {
      console.log('⚠️  Project input not found');
    }
    
    // Wait for authentication
    console.log('⏳ Waiting for authentication to complete...');
    await page.waitForTimeout(10000);
    
    // Extract auth tokens
    console.log('🔑 Extracting authentication data...');
    const authData = await page.evaluate(() => {
      const data = {};
      
      // Get all localStorage items
      for (let i = 0; i < localStorage.length; i++) {
        const key = localStorage.key(i);
        data[key] = localStorage.getItem(key);
      }
      
      return data;
    });
    
    console.log('📋 Extracted auth data keys:', Object.keys(authData));
    
    // Save to file for chatbot
    const fs = require('fs');
    const authFile = '/etc/op-dbus/chatbot-auth.json';
    
    const authPayload = {
      timestamp: new Date().toISOString(),
      gcp_project: '420281222188',
      auth_data: authData,
      puppeteer_automated: true
    };
    
    fs.writeFileSync(authFile, JSON.stringify(authPayload, null, 2));
    console.log(`💾 Saved authentication to: ${authFile}`);
    
    return authData;
    
  } finally {
    await browser.close();
  }
}

authenticateChatbot().catch(console.error);
