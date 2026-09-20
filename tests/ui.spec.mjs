import { test, expect } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'

const assets = fs.readdirSync('dist/assets')
const js = fs.readFileSync(path.join('dist/assets', assets.find(x => x.endsWith('.js'))), 'utf8')
const css = fs.readFileSync(path.join('dist/assets', assets.find(x => x.endsWith('.css'))), 'utf8')
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js', 'utf8').replace(/export\s*\{[^}]*\};?/g, '')
const bands = [-6, -9, -12, -18]
const measurement = (lufs = -5) => ({ integratedLufs:lufs, truePeakDbtp:-1, peakFactorDb:6, bassRatioDb:-5, bandEnergyDb:bands, aacTruePeakDbtp:-.2 })
const variant = (id, label, lufs, similarity) => ({ id, profileId:id, profileLabel:label, targetLufs:-5, engineReferenceDb:-5, achievedDeltaLu:lufs + 5, attempts:2, path:`C:\\Temp\\${id}.wav`, measurements:measurement(lufs), segmentMeasurements:measurement(lufs), preserveBass:id !== 'aggressive', peakFactorLossDb:id === 'aggressive' ? 4.2 : 1.2, bassChangeDb:id === 'aggressive' ? 1.5 : .2, aacRisk:id === 'aggressive', diagnostics:[`${label} diagnostic.`], referenceSimilarity:similarity })

async function mount(page) {
  await page.route('**/*', route => route.fulfill({ contentType:'text/html', body:String.raw`<!doctype html><html><head><style>${css}</style></head><body><div id="app"></div><script type="module">
${mocks}
window.testCalls=[]; window.dialogCalls=0; window.completionDings=0;
window.testMeasurement=(lufs=-5)=>({integratedLufs:lufs,truePeakDbtp:-1,peakFactorDb:6,bassRatioDb:-5,bandEnergyDb:[-6,-9,-12,-18],aacTruePeakDbtp:-.2});
window.testVariant=(id,label,lufs,similarity)=>({id,profileId:id,profileLabel:label,targetLufs:-5,engineReferenceDb:-5,achievedDeltaLu:lufs+5,attempts:2,path:'C:\\Temp\\'+id+'.wav',measurements:window.testMeasurement(lufs),segmentMeasurements:window.testMeasurement(lufs),preserveBass:id!=='aggressive',peakFactorLossDb:id==='aggressive'?4.2:1.2,bassChangeDb:id==='aggressive'?1.5:.2,aacRisk:id==='aggressive',diagnostics:[label+' diagnostic.'],referenceSimilarity:similarity});
window.AudioContext=class { state='running'; currentTime=0; destination={}; createGain(){return {gain:{setValueAtTime(){},exponentialRampToValueAtTime(){}},connect(){}}}; createOscillator(){return {frequency:{setValueAtTime(){}},connect(){},start(){window.completionDings+=1},stop(){}}}; resume(){return Promise.resolve()}; close(){return Promise.resolve()} };
mockWindows('main'); mockIPC((cmd,args)=>{ window.testCalls.push({cmd,args}); if(cmd==='plugin:dialog|open'){ window.dialogCalls++; return window.dialogCalls === 1 ? 'C:\\Audio\\track.wav' : 'C:\\Audio\\reference.wav'; } if(cmd==='inspect_track') return {path:args.path,name:args.path.includes('reference')?'reference.wav':'track.wav',extension:'WAV'}; if(cmd==='inspect_waveform') return {durationSeconds:180,peaks:Array.from({length:240},(_,i)=>.18+Math.abs(Math.sin(i*.31))*.82)}; if(cmd==='start_mastering') return new Promise((resolve,reject)=>{window.finishMaster=resolve;window.failMaster=reject}); if(cmd==='start_auto_mastering') return new Promise(resolve=>{window.finishAuto=resolve}); if(cmd==='export_auto_master') return 'C:\\Audio\\track_hard-techno_dense_target--5.0LUFS_auto.wav'; },{shouldMockEvents:true});
window.testEmit=(event,payload)=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event,payload});
${js}</script></body></html>` }))
  await page.goto('https://lyte.test')
}

test('help describes measured Auto targets and local references', async ({ page }) => {
  await mount(page)
  await page.getByRole('button', { name:'Aide' }).click()
  await expect(page.getByRole('dialog', { name:'Masterisez sur votre ordinateur.' })).toBeVisible()
  await expect(page.getByText('Measured Hard Techno target')).toHaveCount(0)
  await expect(page.getByText('Mesures Auto')).toBeVisible()
  await page.getByRole('button', { name:/Fermer/ }).click()
  await expect(page.getByRole('dialog')).toHaveCount(0)
})

test('Manual keeps the direct PhaseLimiter options', async ({ page }) => {
  await mount(page)
  await page.getByRole('button', { name:/Drop a track/ }).click()
  await page.locator('#loudness').fill('-8.5')
  await page.getByRole('button', { name:'MASTER TRACK' }).click()
  expect(await page.evaluate(() => window.testCalls.find(x => x.cmd === 'start_mastering').args.options)).toEqual({input:'C:\\Audio\\track.wav',loudness:-8.5,intensity:1,preserveBass:false,dynamicBass:true,softClipDb:.5})
  await page.evaluate(() => window.finishMaster({output:'C:\\Audio\\master.wav',source:window.testMeasurement(-10),master:window.testMeasurement(-8)}))
  await expect(page.getByText(/Master complete/)).toBeVisible()
})

test('Auto sends the explicit target and three profile result is exportable', async ({ page }) => {
  await mount(page)
  await page.getByRole('button', { name:/Drop a track/ }).click()
  await page.getByRole('button', { name:/Auto/ }).click()
  await expect(page.locator('#auto-target')).toHaveValue('-5')
  await expect(page.locator('#auto-target')).toHaveAttribute('max', '-2')
  await page.getByRole('button', { name:/Use integrated Hard Techno level/ }).click()
  await expect(page.locator('#auto-target')).toHaveValue('-2.9')
  await page.locator('#auto-target').fill('-4.5')
  await page.getByTestId('source-waveform').click({ position: { x: 300, y: 40 } })
  await page.getByRole('button', { name:'RENDER 3 PROFILES' }).click()
  const options = await page.evaluate(() => window.testCalls.find(x => x.cmd === 'start_auto_mastering').args.options)
  expect(options).toMatchObject({input:'C:\\Audio\\track.wav',targetLufs:-4.5,sourceSegment:{durationSeconds:30},reference:null})
  expect(options.sourceSegment.startSeconds).toBeGreaterThan(0)
  await page.evaluate(() => window.testEmit('mastering-progress', 100))
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','90')
  await page.evaluate(() => window.finishAuto({sessionId:'session',sourcePath:'C:\\Temp\\source.wav',source:window.testMeasurement(-10),sourceSegment:window.testMeasurement(-7),targetLufs:-4.5,variants:[window.testVariant('faithful','Fidèle',-4.7),window.testVariant('dense','Dense',-4.5),window.testVariant('aggressive','Agressif',-4.4)],recommendedId:'dense',recommendation:'Dense is selected as the middle trade-off.'}))
  await expect(page.getByText('Three profiles ready')).toBeVisible()
  await expect(page.locator('.variant')).toHaveCount(3)
  await expect(page.getByText('Dense diagnostic.')).toBeVisible()
  await page.getByRole('button', { name:'Export selected master' }).click()
  expect(await page.evaluate(() => window.testCalls.some(x => x.cmd === 'export_auto_master' && x.args.variantId === 'dense'))).toBe(true)
})

test('Auto accepts an optional reference passage and shows its comparison', async ({ page }) => {
  await mount(page)
  await page.getByRole('button', { name:/Drop a track/ }).click()
  await page.getByRole('button', { name:/Auto/ }).click()
  await page.getByRole('button', { name:'Choose your own reference' }).click()
  await expect(page.getByText('Your reference will replace the built-in Hard Techno reference.')).toBeVisible()
  await page.getByTestId('reference-waveform').click({ position: { x: 120, y: 40 } })
  await page.getByRole('button', { name:'RENDER 3 PROFILES' }).click()
  const options = await page.evaluate(() => window.testCalls.find(x => x.cmd === 'start_auto_mastering').args.options)
  expect(options.reference).toEqual({input:'C:\\Audio\\reference.wav',segment:{startSeconds:30,durationSeconds:30}})
  await page.evaluate(() => window.finishAuto({sessionId:'session',sourcePath:'C:\\Temp\\source.wav',source:window.testMeasurement(-10),sourceSegment:window.testMeasurement(-7),targetLufs:-5,referencePath:'C:\\Temp\\reference.wav',referenceSegment:window.testMeasurement(-5),variants:[window.testVariant('faithful','Fidèle',-5,88),window.testVariant('dense','Dense',-5,91),window.testVariant('aggressive','Agressif',-5,82)],recommendedId:'dense',recommendation:'Dense is closest to the selected reference passage.'}))
  await expect(page.getByText('Reference', { exact:true })).toBeVisible()
  await expect(page.getByText('Ref 91/100')).toBeVisible()
})
