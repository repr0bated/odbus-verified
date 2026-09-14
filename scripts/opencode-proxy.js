#!/usr/bin/env node
// OpenAI-compatible proxy that routes to OpenCode serve (port 4201)
// Translates /v1/chat/completions → OpenCode session API
// Translates /v1/models → static free model list

const http = require('http');
const OPENCODE_PORT = 4201;
const PROXY_PORT = process.env.OPENCODE_PROXY_PORT || 4202;

const FREE_MODELS = [
  { id: 'opencode/big-pickle', display: 'Big Pickle (Free)', description: 'OpenCode multi-model router' },
  { id: 'opencode/gpt-5-nano', display: 'GPT-5 Nano (Free)', description: 'Compact reasoning model' },
  { id: 'opencode/kimi-k2.5-free', display: 'Kimi K2.5 (Free)', description: 'Moonshot AI - strong at code & math' },
  { id: 'opencode/minimax-m2.5-free', display: 'MiniMax M2.5 (Free)', description: 'MiniMax general-purpose model' },
];

// Map internal model IDs to friendly display names
const DISPLAY_NAMES = {
  'minimax-m2.5-free': 'MiniMax M2.5',
  'kimi-k2.5-free': 'Kimi K2.5',
  'gpt-5-nano': 'GPT-5 Nano',
  'big-pickle': 'Big Pickle',
  'opencode/minimax-m2.5-free': 'MiniMax M2.5',
  'opencode/kimi-k2.5-free': 'Kimi K2.5',
  'opencode/gpt-5-nano': 'GPT-5 Nano',
  'opencode/big-pickle': 'Big Pickle',
};

function friendlyModelName(rawId) {
  return DISPLAY_NAMES[rawId] || rawId.replace(/^opencode\//, '').replace(/-free$/, '').replace(/-/g, ' ').replace(/\b\w/g, c => c.toUpperCase());
}

function ocRequest(method, path, body) {
  return new Promise((resolve, reject) => {
    const opts = {
      hostname: '127.0.0.1',
      port: OPENCODE_PORT,
      path,
      method,
      headers: { 'Content-Type': 'application/json' },
    };
    const req = http.request(opts, (res) => {
      let data = '';
      res.on('data', (chunk) => data += chunk);
      res.on('end', () => {
        try { resolve(JSON.parse(data)); }
        catch { resolve(data); }
      });
    });
    req.on('error', reject);
    if (body) req.write(JSON.stringify(body));
    req.end();
  });
}

const server = http.createServer(async (req, res) => {
  res.setHeader('Content-Type', 'application/json');
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Headers', '*');
  res.setHeader('Access-Control-Allow-Methods', '*');

  if (req.method === 'OPTIONS') {
    res.writeHead(200);
    return res.end();
  }

  // GET /v1/models
  if (req.url === '/v1/models' && req.method === 'GET') {
    return res.end(JSON.stringify({
      object: 'list',
      data: FREE_MODELS.map(m => ({
        id: m.id,
        object: 'model',
        created: Date.now(),
        owned_by: 'opencode',
        display_name: m.display,
        description: m.description,
        pricing: { prompt: '0', completion: '0' },
      })),
    }));
  }

  // POST /v1/chat/completions
  if (req.url === '/v1/chat/completions' && req.method === 'POST') {
    let body = '';
    for await (const chunk of req) body += chunk;
    let parsed;
    try { parsed = JSON.parse(body); }
    catch { res.writeHead(400); return res.end(JSON.stringify({ error: 'Invalid JSON' })); }

    const model = parsed.model || 'opencode/big-pickle';
    const messages = parsed.messages || [];
    const lastMsg = messages[messages.length - 1];
    const userText = lastMsg ? lastMsg.content : '';

    // Build system context from earlier messages
    let systemParts = [];
    for (const msg of messages) {
      if (msg.role === 'system') {
        systemParts.push(msg.content);
      }
    }

    try {
      // Create session
      const session = await ocRequest('POST', '/session', {});
      const sid = session.id;

      // Map model to opencode provider/model
      const providerID = 'opencode';
      const modelID = model.startsWith('opencode/') ? model : `opencode/${model}`;

      // Combine system + user message
      let fullText = userText;
      if (systemParts.length > 0) {
        fullText = systemParts.join('\n') + '\n\n' + userText;
      }

      // Send message
      const result = await ocRequest('POST', `/session/${sid}/message`, {
        parts: [{ type: 'text', text: fullText }],
        providerID,
        modelID,
      });

      // Extract text from response parts
      let responseText = '';
      if (result.parts) {
        for (const part of result.parts) {
          if (part.type === 'text') responseText += part.text;
        }
      }

      const usage = result.info?.tokens || {};

      // Return OpenAI-compatible response with friendly names
      const rawModel = result.info?.modelID || model;
      return res.end(JSON.stringify({
        id: `chatcmpl-${sid}`,
        object: 'chat.completion',
        created: Math.floor(Date.now() / 1000),
        model: rawModel,
        model_display: friendlyModelName(rawModel),
        provider: 'OpenCode (Free)',
        choices: [{
          index: 0,
          message: { role: 'assistant', content: responseText },
          finish_reason: result.info?.finish || 'stop',
        }],
        usage: {
          prompt_tokens: usage.input || 0,
          completion_tokens: usage.output || 0,
          total_tokens: usage.total || 0,
        },
      }));
    } catch (err) {
      console.error('OpenCode proxy error:', err.message);
      res.writeHead(502);
      return res.end(JSON.stringify({ error: { message: err.message, code: 502 } }));
    }
  }

  // Health check
  if (req.url === '/health' || req.url === '/') {
    return res.end(JSON.stringify({ status: 'ok', provider: 'opencode', models: FREE_MODELS.length }));
  }

  res.writeHead(404);
  res.end(JSON.stringify({ error: 'Not found' }));
});

server.listen(PROXY_PORT, '127.0.0.1', () => {
  console.log(`OpenCode OpenAI-compatible proxy listening on http://127.0.0.1:${PROXY_PORT}`);
  console.log(`Proxying to OpenCode serve at http://127.0.0.1:${OPENCODE_PORT}`);
  console.log(`Free models: ${FREE_MODELS.map(m => m.id).join(', ')}`);
});
