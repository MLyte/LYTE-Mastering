import { chromium } from '@playwright/test';
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const appPath = process.env.LYTE_TEST_APP || path.join(root, 'src-tauri/target/release/lyte-mastering.exe');
const artifacts = path.join(root, 'tests/artifacts/native');
fs.mkdirSync(artifacts, { recursive: true });
const work = fs.mkdtempSync(path.join(artifacts, 'audio-'));
const ffmpeg = path.join(root, 'bin/ffmpeg.exe');
const format = process.env.LYTE_TEST_FORMAT || 'wav';
if (!['wav', 'flac', 'mp3'].includes(format)) throw new Error('Unsupported test format');
const fixture = path.join(work, 'test track.' + format);
// A deterministic test signal, not a mastering implementation or a user's private audio.
if (process.env.LYTE_TEST_SOURCE) {
  execFileSync(ffmpeg, ['-hide_banner','-loglevel','error','-i',process.env.LYTE_TEST_SOURCE,'-c:a',format === 'wav' ? 'pcm_s24le' : format === 'flac' ? 'flac' : 'libmp3lame',fixture], { windowsHide:true });
} else execFileSync(ffmpeg, ['-hide_banner', '-loglevel', 'error', '-f', 'lavfi', '-i', 'aevalsrc=0.06*sin(2*PI*110*t)+0.03*sin(2*PI*440*t)+0.015*sin(2*PI*1760*t)|0.06*sin(2*PI*110*t)+0.025*sin(2*PI*554.37*t)+0.015*sin(2*PI*2200*t):s=48000:d=8', '-c:a', format === 'wav' ? 'pcm_s24le' : format === 'flac' ? 'flac' : 'libmp3lame', fixture], { windowsHide: true });
const stdout = fs.openSync(path.join(work, 'app-stdout.log'), 'w');
const stderr = fs.openSync(path.join(work, 'app-stderr.log'), 'w');
// Temporary WebView2 debugging endpoint for this test only. No app HTTP backend.
const app = spawn(appPath, [], { cwd: path.dirname(appPath), windowsHide: true, env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9237' }, stdio: ['ignore', stdout, stderr] });
let browser;
try {
  for (let attempt = 0; attempt < 45; attempt++) {
    try { browser = await chromium.connectOverCDP('http://127.0.0.1:9237'); break; }
    catch { if (app.exitCode !== null) throw new Error(`Application exited: ${app.exitCode}`); await new Promise(r => setTimeout(r, 1000)); }
  }
  if (!browser) throw new Error('WebView2 test connection unavailable');
  const context = browser.contexts()[0];
  let page;
  for (let attempt = 0; attempt < 30; attempt++) {
    page = context.pages().find(p => p.url().includes('tauri.localhost'));
    if (page) break;
    await new Promise(r => setTimeout(r, 500));
  }
  if (!page) throw new Error('LYTE webview not found');
  await page.getByRole('heading', { name: 'Make your track ready.' }).waitFor();
  await page.waitForFunction(() => !document.querySelector('button.drop').disabled);
  await page.evaluate(async () => {
    window.nativeProgress = [];
    const callback = window.__TAURI_INTERNALS__.transformCallback(e => window.nativeProgress.push(e.payload));
    await window.__TAURI_INTERNALS__.invoke('plugin:event|listen', { event: 'mastering-progress', target: { kind: 'Any' }, handler: callback });
  });
  await page.evaluate(input => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', { event: 'tauri://drag-drop', payload: { paths: [input], position: { x: 100, y: 100 } } }), fixture);
  await page.getByText('test track.' + format, { exact: true }).waitFor();
  await page.locator('#loudness').fill('-8.5');
  await page.locator('#intensity').fill('1');
  await page.getByRole('switch').check();
  const start = Date.now();
  await page.getByRole('button', { name: 'MASTER TRACK' }).click();
  await page.waitForFunction(() => document.body.innerText.includes('Master complete') || document.querySelector('[role=alert]'), null, { timeout: 300000 });
  const error = await page.getByRole('alert').allTextContents();
  if (error.length) throw new Error(error.join('\n'));
  const output = fixture.replace(/\.(wav|flac|mp3)$/, '_mastered.wav');
  if (!fs.existsSync(output)) throw new Error('Output WAV missing');
  const progress = await page.evaluate(() => window.nativeProgress);
  if (!progress.some(v => v > 0 && v < 100)) throw new Error('No intermediate native progress event');
  execFileSync(ffmpeg, ['-v', 'error', '-i', output, '-f', 'null', '-'], { windowsHide: true });
  await page.screenshot({ path: path.join(work, 'complete.png'), fullPage: true });
  await page.getByRole('button', { name: 'MASTER TRACK' }).click();
  await page.getByRole('alert').filter({ hasText: 'already exists' }).waitFor();
  const report = { app: appPath, input: fixture, output, seconds: (Date.now() - start) / 1000, progressCount: progress.length, progressMin: Math.min(...progress), progressMax: Math.max(...progress), bytes: fs.statSync(output).size, noOverwrite: true };
  fs.writeFileSync(path.join(work, 'report.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  // The test process is stopped below; production permissions stay unchanged.
} finally {
  if (browser) await browser.close().catch(() => {});
  if (app.exitCode === null) app.kill();
  fs.closeSync(stdout); fs.closeSync(stderr);
}
