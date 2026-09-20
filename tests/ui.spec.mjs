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
window.completionDings = 0;
window.AudioContext = class {
  state = 'running'; currentTime = 0; destination = {};
  createGain() { return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect() {} }; }
  createOscillator() { return { frequency: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect() {}, start() { window.completionDings += 1; }, stop() {} }; }
  resume() { return Promise.resolve(); }
  close() { return Promise.resolve(); }
};
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
  await expect(page.getByRole('button', { name: /Drop a track/ })).toBeEnabled()
  await page.getByRole('button', { name: 'Paramètres' }).click()
  await expect(page.getByRole('dialog', { name: 'Paramètres de comparaison.' })).toBeVisible()
  await page.locator('#auto-target-0').fill('-8')
  await page.getByRole('button', { name: 'Revenir aux paramètres conseillés' }).click()
  await expect(page.locator('#auto-target-0')).toHaveValue('-7')
  await page.getByRole('button', { name: 'Fermer les paramètres' }).click()
  await page.getByRole('button', { name: /Aide/ }).click()
  await expect(page.getByRole('dialog', { name: 'Masterisez sans quitter votre ordinateur.' })).toBeVisible()
  await expect(page.getByText('Projets audio à l’origine du moteur')).toBeVisible()
  await expect(page.getByRole('link', { name: 'Bakuage' })).toHaveAttribute('href', 'https://github.com/ai-mastering/bakuage')
  await page.getByRole('link', { name: 'Bakuage' }).click()
  expect(await page.evaluate(() => window.testCalls.some(x => x.cmd === 'open_help_link' && x.args.project === 'bakuage'))).toBe(true)
  await expect(page.getByText('Ce que fait le moteur')).toBeVisible()
  await page.getByRole('dialog').evaluate(element => { element.scrollTop = element.scrollHeight })
  await expect(page.locator('.help-close')).toBeVisible()
  await page.getByRole('button', { name: 'Fermer l’aide' }).click()
  await expect(page.getByRole('dialog', { name: 'Masterisez sans quitter votre ordinateur.' })).toHaveCount(0)
  await expect(page.locator('#loudness')).toHaveCount(0)
  await expect(page.locator('#intensity')).toHaveCount(0)
  await expect(page.getByRole('switch')).toHaveCount(0)
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
  await expect(page.getByRole('progressbar')).toBeVisible()
  await expect(page.getByRole('button', { name: 'MASTER TRACK' })).toHaveCount(0)
  await expect(page.locator('#loudness')).toHaveCount(0)
  expect(await page.evaluate(() => window.testCalls.find(x=>x.cmd==='start_mastering').args.options)).toEqual({input:'C:\\Audio\\track.wav',loudness:-8.5,intensity:1,preserveBass:true})
  await page.evaluate(() => window.testEmit('mastering-progress',42))
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','38')
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
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','90')
  await page.evaluate(() => window.finishMaster({output:'C:\\Audio\\track_mastered_-8.5dB_i1.00_bass-on.wav',source:{integratedLufs:-10,truePeakDbtp:-1,peakFactorDb:8,bassRatioDb:-4},master:{integratedLufs:-8,truePeakDbtp:-1,peakFactorDb:7,bassRatioDb:-5}}))
  await expect(page.getByText('✓ Master complete')).toBeVisible()
  await expect.poll(() => page.evaluate(() => window.completionDings)).toBe(1)
  await expect(page.locator('.manual-score')).toContainText('82 → 99')
  await page.getByRole('button', {name:/Open folder/}).click()
  expect(await page.evaluate(() => window.testCalls.some(x=>x.cmd==='open_output_folder'))).toBe(true)
  await page.screenshot({path:'tests/artifacts/succeeded.png', fullPage:true})
  await page.getByRole('button', { name: 'Start over' }).click()
  await expect(page.getByRole('button', { name: /Drop a track/ })).toBeVisible()
  await expect(page.getByText('Master complete')).toHaveCount(0)
})

test('Auto Hard Techno selects a measured variant and exports only on choice', async ({ page }) => {
  await mount(page)
  await page.getByRole('button', { name: /Drop a track/ }).click()
  await expect(page.getByText('track.wav', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: /Auto — Hard Techno/ }).click()
  await page.getByRole('button', { name: 'ANALYSE & MASTER' }).click()
  expect(await page.evaluate(() => window.testCalls.find(x => x.cmd === 'start_auto_mastering').args.options)).toEqual({ input: 'C:\\Audio\\track.wav', targets: [-7, -5, -4] })
  await expect(page.getByText('4 adaptive variants')).toHaveCount(0)
  await expect(page.locator('#intensity')).toHaveCount(0)
  await expect(page.getByRole('switch')).toHaveCount(0)
  await page.evaluate(() => window.testEmit('mastering-progress',100))
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','23')
  await page.evaluate(() => window.testEmit('mastering-progress',0))
  await page.evaluate(() => window.testEmit('mastering-progress',100))
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','45')
  await page.evaluate(() => window.testEmit('mastering-progress',0))
  await page.evaluate(() => window.testEmit('mastering-progress',100))
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','68')
  await page.evaluate(() => window.testEmit('mastering-progress',0))
  await page.evaluate(() => window.testEmit('mastering-progress',100))
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','90')
  await page.evaluate(() => window.finishAuto({
    sessionId:'session-1', sourcePath:'C:\\Temp\\source.wav', source:{integratedLufs:-10,truePeakDbtp:-1,peakFactorDb:8,bassRatioDb:-4}, recommendedId:'v2', recommendation:'Recommended from measured loudness.',
    variants:[
      {id:'v0',targetDb:-11,path:'C:\\Temp\\v0.wav',measurements:{integratedLufs:-9,truePeakDbtp:-1,peakFactorDb:7,bassRatioDb:-4},preserveBass:false,peakFactorLossDb:1,bassChangeDb:0,eligible:true,note:'Within guardrails.'},
      {id:'v1',targetDb:-9,path:'C:\\Temp\\v1.wav',measurements:{integratedLufs:-8,truePeakDbtp:-1,peakFactorDb:6,bassRatioDb:-4},preserveBass:true,peakFactorLossDb:2,bassChangeDb:0,eligible:true,note:'Within guardrails.'},
      {id:'v2',targetDb:-7,path:'C:\\Temp\\v2.wav',measurements:{integratedLufs:-7.8,truePeakDbtp:-1,peakFactorDb:5.5,bassRatioDb:-4},preserveBass:false,peakFactorLossDb:2.5,bassChangeDb:0,eligible:true,note:'Within guardrails.'},
      {id:'v3',targetDb:-5,path:'C:\\Temp\\v3.wav',measurements:{integratedLufs:-7,truePeakDbtp:-1,peakFactorDb:3,bassRatioDb:-1},preserveBass:false,peakFactorLossDb:5,bassChangeDb:3,eligible:false,note:'Outside guardrails.'}
    ]
  }))
  await expect(page.getByText('Auto results ready')).toBeVisible()
  await expect.poll(() => page.evaluate(() => window.completionDings)).toBe(1)
  await expect(page.getByRole('button', { name: /Drop a track/ })).toHaveCount(0)
  await expect(page.locator('.mode-switch')).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Start over' })).toBeVisible()
  await expect(page.locator('.professionality')).toHaveCount(4)
  await expect(page.getByText('Ranked results', { exact: true })).toBeVisible()
  await expect(page.locator('.variant').first()).toContainText('#1')
  await expect(page.locator('.variant').first()).toContainText('Best match')
  await expect(page.locator('.professionality').first()).toContainText('82 → 93')
  await expect(page.locator('.professionality').first()).toContainText('+11 pts · +13 %')
  await expect(page.locator('.variant').last()).toContainText('#4')
  await expect(page.getByText('Recommended', { exact: true })).toBeVisible()
  await expect(page.locator('.mastering-profile')).toHaveCount(4)
  await expect(page.locator('.mastering-profile').first()).toContainText('Energy')
  await expect(page.locator('.variant small').filter({ hasText: 'Bass off' })).toHaveCount(3)
  await expect(page.locator('.variant small').filter({ hasText: 'Bass on' })).toHaveCount(1)
  await page.getByRole('button', { name: 'Export selected master' }).click()
  expect(await page.evaluate(() => window.testCalls.some(x => x.cmd === 'export_auto_master'))).toBe(true)
  await expect(page.getByRole('button', { name: '✓ Master exported' })).toBeDisabled()
})
