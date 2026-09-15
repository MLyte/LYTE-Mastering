import { test, expect } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'

const assets = fs.readdirSync('dist/assets')
const js = fs.readFileSync(path.join('dist/assets', assets.find(x => x.endsWith('.js'))), 'utf8')
const css = fs.readFileSync(path.join('dist/assets', assets.find(x => x.endsWith('.css'))), 'utf8')
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export\s*\{[^}]*\};?/g, '')
// Browser-only IPC fixtures. These tests do NOT exercise PhaseLimiter or claim a DSP result.
async function mount(page) {
  await page.route('**/*', route => route.fulfill({ contentType: 'text/html', body: String.raw`<!doctype html><html lang="en"><head><meta charset="UTF-8"><style>${css}</style></head><body><div id="app"></div><script type="module">
${mocks}
window.testCalls = [];
mockWindows('main');
mockIPC((cmd, args) => {
  window.testCalls.push({ cmd, args });
  if(cmd === 'plugin:dialog|open') return 'C:\\Audio\\track.wav';
  if(cmd === 'inspect_track') return { path: args.path, name: 'track.wav', extension: 'WAV' };
  if(cmd === 'start_mastering') return new Promise((resolve,reject) => {window.finishMaster = resolve; window.failMaster = reject;});
  if(cmd === 'start_auto_mastering') return new Promise(resolve => {window.finishAuto = resolve;});
  if(cmd === 'export_auto_master') return 'C:\\Audio\\hard-techno_-7dB_auto.wav';
}, { shouldMockEvents: true });
window.testEmit = (event,payload) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event,payload});
${js}
</script></body></html>` }));
  await page.goto('https://lyte.test')
  await expect(page.getByRole('button', { name: /Drop a track/ })).toBeEnabled()
}

test('idle layout, defaults and compact window', async ({ page }) => {
  await mount(page)
  await expect(page.getByRole('button', { name: 'MASTER TRACK' })).toBeDisabled()
  await expect(page.locator('#loudness')).toHaveValue('-9')
  await expect(page.locator('#intensity')).toHaveValue('1')
  await expect(page.getByRole('switch')).not.toBeChecked()
  await page.screenshot({path:'tests/artifacts/idle.png', fullPage:true})
  await page.setViewportSize({width:380,height:760})
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})

test('selection, exact settings, real event wiring and readable failure', async ({ page }) => {
  await mount(page)
  await page.getByRole('button', { name: /Drop a track/ }).click()
  await expect(page.getByText('track.wav', {exact:true})).toBeVisible()
  await page.locator('#loudness').fill('-8.5')
  await page.getByRole('switch').check()
  await page.getByRole('button', {name:'MASTER TRACK'}).click()
  await expect(page.getByRole('button', {name:'MASTERING…'})).toBeDisabled()
  await expect(page.locator('#loudness')).toBeDisabled()
  expect(await page.evaluate(() => window.testCalls.find(x=>x.cmd==='start_mastering').args.options)).toEqual({input:'C:\\Audio\\track.wav',loudness:-8.5,intensity:1,preserveBass:true})
  await page.evaluate(() => window.testEmit('mastering-progress',42))
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','42')
  await page.screenshot({path:'tests/artifacts/processing.png', fullPage:true})
  await page.evaluate(() => window.failMaster('PhaseLimiter was not found. Place phase_limiter.exe and its DLLs in /bin.'))
  await expect(page.getByRole('alert')).toContainText('PhaseLimiter was not found')
  await expect(page.getByRole('button', {name:'MASTER TRACK'})).toBeEnabled()
})

test('native drop event, completion and constrained folder command', async ({ page }) => {
  await mount(page)
  await page.evaluate(() => window.testEmit('tauri://drag-drop',{paths:['C:\\Audio\\track.wav'],position:{x:100,y:100}}))
  await expect(page.getByText('track.wav', {exact:true})).toBeVisible()
  await page.getByRole('button', {name:'MASTER TRACK'}).click()
  await page.evaluate(() => window.testEmit('mastering-progress',100))
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','99')
  await page.evaluate(() => window.finishMaster('C:\\Audio\\track_mastered_-8.5dB_i1.00_bass-on.wav'))
  await expect(page.getByText('✓ Master complete')).toBeVisible()
  await page.getByRole('button', {name:/Open folder/}).click()
  expect(await page.evaluate(() => window.testCalls.some(x=>x.cmd==='open_output_folder'))).toBe(true)
  await page.screenshot({path:'tests/artifacts/succeeded.png', fullPage:true})
})

test('Auto Hard Techno selects a measured variant and exports only on choice', async ({ page }) => {
  await mount(page)
  await page.getByRole('button', { name: /Drop a track/ }).click()
  await expect(page.getByText('track.wav', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: /Auto — Hard Techno/ }).click()
  await page.getByRole('button', { name: 'MASTER TRACK' }).click()
  expect(await page.evaluate(() => window.testCalls.some(x => x.cmd === 'start_auto_mastering'))).toBe(true)
  await page.evaluate(() => window.finishAuto({
    sessionId:'session-1', sourcePath:'C:\\Temp\\source.wav', source:{integratedLufs:-10,truePeakDbtp:-1,peakFactorDb:8,bassRatioDb:-4}, recommendedId:'v2', recommendation:'Recommended from measured loudness.',
    variants:[
      {id:'v0',targetDb:-11,path:'C:\\Temp\\v0.wav',measurements:{integratedLufs:-9,truePeakDbtp:-1,peakFactorDb:7,bassRatioDb:-4},peakFactorLossDb:1,bassChangeDb:0,eligible:true,note:'Within guardrails.'},
      {id:'v1',targetDb:-9,path:'C:\\Temp\\v1.wav',measurements:{integratedLufs:-8,truePeakDbtp:-1,peakFactorDb:6,bassRatioDb:-4},peakFactorLossDb:2,bassChangeDb:0,eligible:true,note:'Within guardrails.'},
      {id:'v2',targetDb:-7,path:'C:\\Temp\\v2.wav',measurements:{integratedLufs:-7.8,truePeakDbtp:-1,peakFactorDb:5.5,bassRatioDb:-4},peakFactorLossDb:2.5,bassChangeDb:0,eligible:true,note:'Within guardrails.'},
      {id:'v3',targetDb:-5,path:'C:\\Temp\\v3.wav',measurements:{integratedLufs:-7,truePeakDbtp:-1,peakFactorDb:3,bassRatioDb:-1},peakFactorLossDb:5,bassChangeDb:3,eligible:false,note:'Outside guardrails.'}
    ]
  }))
  await expect(page.getByText('Auto results ready')).toBeVisible()
  await expect(page.getByText('Recommended')).toBeVisible()
  await page.getByRole('button', { name: 'Export selected master' }).click()
  expect(await page.evaluate(() => window.testCalls.some(x => x.cmd === 'export_auto_master'))).toBe(true)
})