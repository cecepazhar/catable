import fs from 'node:fs';
import path from 'node:path';

async function main() {
  const outDir = '/home/cecepazhar/Product/catable/screenshots';
  fs.mkdirSync(outDir, { recursive: true });

  const verRes = await fetch('http://127.0.0.1:9222/json/version');
  const ver = await verRes.json();
  const browserWsUrl = ver.webSocketDebuggerUrl;

  const browserWs = new WebSocket(browserWsUrl);
  await new Promise((res) => (browserWs.onopen = res));

  let id = 1;
  function sendBrowser(method, params = {}) {
    return new Promise((resolve) => {
      const msgId = id++;
      const handler = (event) => {
        const msg = JSON.parse(event.data);
        if (msg.id === msgId) {
          browserWs.removeEventListener('message', handler);
          resolve(msg.result);
        }
      };
      browserWs.addEventListener('message', handler);
      browserWs.send(JSON.stringify({ id: msgId, method, params }));
    });
  }

  const { targetId } = await sendBrowser('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await sendBrowser('Target.attachToTarget', { targetId, flatten: true });

  function sendSession(method, params = {}) {
    return new Promise((resolve) => {
      const msgId = id++;
      const handler = (event) => {
        const msg = JSON.parse(event.data);
        if (msg.id === msgId) {
          browserWs.removeEventListener('message', handler);
          resolve(msg.result);
        }
      };
      browserWs.addEventListener('message', handler);
      browserWs.send(JSON.stringify({ id: msgId, sessionId, method, params }));
    });
  }

  await sendSession('Page.enable');
  await sendSession('Emulation.setDeviceMetricsOverride', {
    width: 1280,
    height: 800,
    deviceScaleFactor: 1,
    mobile: false,
  });

  const targets = [
    { url: 'http://127.0.0.1:5174/', file: '01_connection_manager_general.png' },
    { url: 'http://127.0.0.1:5174/?tab=ssh', file: '02_connection_manager_ssh_tunnel.png' },
    { url: 'http://127.0.0.1:5174/table-browser', file: '03_tableplus_workspace_browser.png' },
    { url: 'http://127.0.0.1:5174/sql-studio', file: '04_tableplus_sql_studio.png' },
    { url: 'http://127.0.0.1:5174/schema-erd', file: '05_tableplus_schema_erd.png' },
    { url: 'http://127.0.0.1:5174/settings', file: '06_settings_license.png' },
  ];

  for (const item of targets) {
    console.log(`Navigating to ${item.url}...`);
    await sendSession('Page.navigate', { url: item.url });
    // wait 2 seconds for SvelteKit and animations to settle
    await new Promise((r) => setTimeout(r, 2000));
    const shot = await sendSession('Page.captureScreenshot', { format: 'png' });
    const buf = Buffer.from(shot.data, 'base64');
    const dest = path.join(outDir, item.file);
    fs.writeFileSync(dest, buf);
    console.log(`Saved: ${dest} (${buf.length} bytes)`);
  }

  browserWs.close();
  process.exit(0);
}

main().catch(console.error);
