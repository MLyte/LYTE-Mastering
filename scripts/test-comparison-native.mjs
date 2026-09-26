import { chromium } from '@playwright/test';
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';

const root = path.resolve(import.meta.dirname, '..');
const appPath = process.env.LYTE_TEST_APP || path.join(root, 'src-tauri/target/release/lyte-mastering.exe');
const work = fs.mkdtempSync(path.join(root, 'tests/artifacts/comparison-'));
const originals = new Map();
const ffmpeg = path.join(root, 'bin/ffmpeg.exe');
for (const [name, codec] of [['track.wav', 'pcm_s24le'], ['track_mastered_-8dB_i1.00_bass-on.wav', 'pcm_f64le'], ['track_mastered_-9dB_i1.00_bass-on.flac', 'flac'], ['track_mastered_-10dB_i1.00_bass-on.mp3', 'libmp3lame']]) {
  execFileSync(ffmpeg, ['-v', 'error', '-f', 'lavfi', '-i', 'sine=frequency=440:duration=4:sample_rate=48000', '-ac', '2', '-c:a', codec, path.join(work, name)], { windowsHide: true });
  originals.set(path.join(work, name), fs.readFileSync(path.join(work, name)));
}
const app = spawn(appPath, [], { cwd: path.dirname(appPath), windowsHide: true, env: { ...process.env, WEBVIEW2_USER_DATA_FOLDER: path.join(work, 'webview'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9238' }, stdio: 'ignore' });
let browser;
try {
  for (let attempt = 0; attempt < 30; attempt++) {
    try { browser = await chromium.connectOverCDP('http://127.0.0.1:9238'); break; }
    catch { if (app.exitCode !== null) throw new Error(`Application exited: ${app.exitCode}`); await new Promise(resolve => setTimeout(resolve, 500)); }
  }
  assert.ok(browser, 'Native WebView2 unavailable');
  let page;
  for (let attempt = 0; attempt < 30; attempt++) {
    page = browser.contexts()[0].pages().find(page => page.url().includes('tauri.localhost'));
    if (page) break;
    await new Promise(resolve => setTimeout(resolve, 500));
  }
  assert.ok(page, 'LYTE page unavailable');
  await page.getByRole('button', { name: 'Comparer les masters' }).waitFor();
  await page.evaluate(folder => localStorage.setItem('lyte-comparison-library-v1', JSON.stringify({ root: folder })), work);
  await page.getByRole('button', { name: 'Comparer les masters' }).click();
  await page.getByRole('button', { name: 'Actualiser' }).click();
  await page.locator('.library-file').first().waitFor();
  for (let index = 0; index < 4; index++) {
    await page.locator('.library-file').nth(index).getByRole('button', { name: 'Ajouter à l’écoute' }).click();
    await page.waitForFunction(count => document.querySelectorAll('.deck').length === count || document.querySelector('.compare-error'), index + 1);
    assert.deepEqual(await page.locator('.compare-error').allTextContents(), []);
  }
  console.log('Loaded:', await page.locator('.deck audio').evaluateAll(elements => elements.map(audio => ({ src: audio.src, readyState: audio.readyState, error: audio.error?.message }))));
  await page.getByRole('button', { name: 'Lecture', exact: true }).click();
  await page.waitForTimeout(800);
  console.log('Playback:', await page.locator('.deck audio').evaluateAll(elements => elements.map(audio => ({ src: audio.src, paused: audio.paused, currentTime: audio.currentTime, error: audio.error?.message }))));
  assert.deepEqual(await page.locator('.compare-error').allTextContents(), []);
  assert.ok(await page.locator('.deck audio').evaluateAll(elements => elements.every(audio => !audio.paused && audio.currentTime > 0)), 'All decks should advance');
  await page.locator('.deck input[type=radio]').first().check();
  await page.waitForTimeout(200);
  assert.deepEqual(await page.locator('.compare-error').allTextContents(), []);
  assert.ok(await page.locator('.deck audio').evaluateAll(elements => elements.every(audio => !audio.paused && audio.currentTime > 0)), 'Switching must keep playback running');
  await page.getByRole('button', { name: 'Pause', exact: true }).click();
  const previewPaths = await page.locator('.deck audio').evaluateAll(elements => elements.map(audio => decodeURIComponent(new URL(audio.src).pathname.slice(1))));
  for (let index = 0; index < 4; index++) await page.locator('.deck').first().getByRole('button', { name: /Retirer/ }).click();
  await page.waitForTimeout(500);
  for (const preview of previewPaths) assert.equal(fs.existsSync(preview), false, 'Temporary playback must be removed');
  for (const [source, original] of originals) assert.deepEqual(fs.readFileSync(source), original, 'Original audio must be unchanged');
  console.log('Native WAV24/WAV64/FLAC/MP3 playback, switching, cleanup and source preservation passed.');
} finally {
  await browser?.close().catch(() => {});
  if (app.exitCode === null) app.kill();
}
