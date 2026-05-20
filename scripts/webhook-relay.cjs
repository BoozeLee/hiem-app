#!/usr/bin/env node
/**
 * HIEM Webhook Relay
 *
 * Receives GitHub webhook events and persists them to JSON lines files
 * so the Tauri desktop app can read them from disk.
 *
 * Usage:
 *   node scripts/webhook-relay.js           # port 9411, webhook path /webhooks/hiem
 *   WEBHOOK_SECRET=<secret> node scripts/webhook-relay.js  # enable HMAC signature check
 *   PORT=9430 node scripts/webhook-relay.js # custom port
 *
 * Endpoint
 *   POST /webhooks/hiem   Content-Type: application/json
 *
 * Events persisted to:
 *   /tmp/hiem-webhooks/<event_type>/*.json   (one file per delivery)
 */

const fs   = require('fs');
const http = require('http');
const crypto = require('crypto');
const path = require('path');

const PORT    = parseInt(process.env.PORT      || '9411', 10);
const PATH    = process.env.WEBHOOK_PATH      || '/webhooks/hiem';
const SECRET  = process.env.WEBHOOK_SECRET   || '';  // HMAC-SHA256 secret
const ROOT    = path.join(process.env.TMPDIR || '/tmp', 'hiem-webhooks');

function emit(event) {
  const ts = new Date().toISOString().replace(/[:.]/g, '-');
  const dir  = path.join(ROOT, event.type || 'unknown');
  const file = path.join(dir, `${ts}-${event.delivery || crypto.randomUUID().slice(0, 8)}.json`);
  try {
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(file, JSON.stringify({ received_at: new Date().toISOString(), ...event }, null, 2), 'utf-8');
    console.log(`[webhook] ${event.type ?? '?'} → ${file}`);
  } catch (e) {
    console.error('[webhook] write error:', e.message);
  }
}

function isValidSignature(body, sigHeader) {
  if (!SECRET || !sigHeader) return true; // HMAC disabled → pass
  // GitHub sends: sha256=<hex>
  const [, sigHex] = sigHeader.split('=');
  if (!sigHex) return false;
  const hmac = crypto.createHmac('sha256', SECRET).update(body);
  const ourHex = hmac.digest('hex');
  try {
    return crypto.timingSafeEqual(Buffer.from(ourHex), Buffer.from(sigHex));
  } catch (_) {
    return false;
  }
}

const server = http.createServer((req, res) => {
  if (req.method === 'POST' && req.url === PATH) {
    const chunks = [];
    req.on('data', chunk => chunks.push(chunk));
    req.on('end', () => {
      const body = Buffer.concat(chunks);
      const sig  = req.headers['x-hub-signature-256'] || req.headers['x-hub-signature'] || '';

      if (!isValidSignature(body, sig)) {
        res.writeHead(401, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: 'Invalid signature' }));
        return;
      }

      let event = {};
      try {
        event = JSON.parse(body.toString('utf-8'));
      } catch (_) {
        res.writeHead(400, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify({ error: 'Invalid JSON body' }));
        return;
      }

      const eventType = req.headers['x-github-event'] || 'unknown';
      const delivery  = req.headers['x-github-delivery'] || null;

      emit({ type: eventType, delivery, payload: event });

      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ ok: true, event: eventType, delivery }));
    });
  } else if (req.method === 'GET' && req.url === PATH || req.url === '/') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ service: 'HIEM Webhook Relay', path: PATH, port: PORT, root: ROOT }));
  } else {
    res.writeHead(404);
    res.end('Not found');
  }
});

server.listen(PORT, () => {
  console.log(`[webhook] HIEM relay → http://0.0.0.0:${PORT}${PATH}`);
  console.log(`[webhook] webhook root  → ${ROOT}`);
  if (SECRET) console.log('[webhook] HMAC-SHA256 signature verification ENABLED');
  else  console.log('[webhook] HMAC verification OFF (set WEBHOOK_SECRET to enable)');
  console.log();
});
