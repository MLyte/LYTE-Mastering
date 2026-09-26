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
  await page.route('**/*', route => route.fulfill({ contentType:'text/html', body:String.raw`<!doctype html><html><head><meta charset="utf-8"><style>${css}</style></head><body><div id="app"></div><script type="module">
${mocks}
window.testCalls=[]; window.dialogCalls=0; window.completionDings=0;
window.testMeasurement=(lufs=-5)=>({integratedLufs:lufs,truePeakDbtp:-1,peakFactorDb:6,bassRatioDb:-5,bandEnergyDb:[-6,-9,-12,-18],aacTruePeakDbtp:-.2});
window.testVariant=(id,label,lufs,similarity)=>({id,profileId:id,profileLabel:label,targetLufs:-5,engineReferenceDb:-5,achievedDeltaLu:lufs+5,attempts:2,path:'C:\\Temp\\'+id+'.wav',measurements:window.testMeasurement(lufs),segmentMeasurements:window.testMeasurement(lufs),preserveBass:id!=='aggressive',peakFactorLossDb:id==='aggressive'?4.2:1.2,bassChangeDb:id==='aggressive'?1.5:.2,aacRisk:id==='aggressive',diagnostics:[label+' diagnostic.'],referenceSimilarity:similarity});
window.AudioContext=class { state='running'; currentTime=0; destination={}; createGain(){return {gain:{setValueAtTime(){},exponentialRampToValueAtTime(){}},connect(){}}}; createOscillator(){return {frequency:{setValueAtTime(){}},connect(){},start(){window.completionDings+=1},stop(){}}}; resume(){return Promise.resolve()}; close(){return Promise.resolve()} };
mockWindows('main'); window.__TAURI_INTERNALS__.convertFileSrc=(path)=>'asset://localhost/'+encodeURIComponent(path); mockIPC((cmd,args)=>{ window.testCalls.push({cmd,args}); if(cmd==='plugin:dialog|open'){ window.dialogCalls++; return window.dialogCalls === 1 ? 'C:\\Audio\\track.wav' : 'C:\\Audio\\reference.wav'; } if(cmd==='scan_audio_folder') return {root:args.root,files:window.comparisonFiles,scannedEntries:32,errors:[],canceled:false}; if(cmd==='analyze_comparison_track') return window.comparisonAnalysis(args.path); if(cmd==='cancel_library_scan'||cmd==='revoke_comparison_track') return null; if(cmd==='inspect_track') return {path:args.path,name:args.path.includes('reference')?'reference.wav':'track.wav',extension:'WAV'}; if(cmd==='inspect_waveform') return {durationSeconds:180,peaks:Array.from({length:240},(_,i)=>.18+Math.abs(Math.sin(i*.31))*.82)}; if(cmd==='start_mastering') return new Promise((resolve,reject)=>{window.finishMaster=resolve;window.failMaster=reject}); if(cmd==='start_auto_mastering') return new Promise(resolve=>{window.finishAuto=resolve}); if(cmd==='export_auto_master') return 'C:\\Audio\\track_hard-techno_dense_target--5.0LUFS_auto.wav'; },{shouldMockEvents:true});
window.comparisonFiles=[
  {path:'C:\\Audio\\track.wav',relativePath:'track.wav',name:'track.wav',extension:'WAV',projectHint:'track',kind:'source',profile:null,bytes:1000,modifiedAtMs:1000},
  ...['faithful','dense','aggressive','faithful','dense'].map((profile,index)=>({path:'C:\\Audio\\track_hard-techno_'+profile+'_actual_-'+(6+index)+'.0LUFS_auto'+(index>2?'_'+(index-1):'')+'.wav',relativePath:'track_hard-techno_'+profile+'_actual_-'+(6+index)+'.0LUFS_auto'+(index>2?'_'+(index-1):'')+'.wav',name:'track_'+profile+'_'+index+'.wav',extension:'WAV',projectHint:'track',kind:'master',profile,bytes:1200,modifiedAtMs:10000-index*1000})),
  {path:'C:\\Audio\\loose.wav',relativePath:'loose.wav',name:'loose.wav',extension:'WAV',projectHint:null,kind:'unclassified',profile:null,bytes:900,modifiedAtMs:500}
];
window.comparisonAnalysis=(path)=>({playbackPath:'C:\\Temp\\comparison-'+path.split(String.fromCharCode(92)).pop(),track:{path,name:path.split(String.fromCharCode(92)).pop(),extension:'WAV'},waveform:{durationSeconds:180,peaks:Array.from({length:240},(_,i)=>.2+Math.abs(Math.sin(i*.3))*.7)},measurements:window.testMeasurement(path.includes('faithful')?-6:path.includes('dense')?-7:-8)});
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

test('local comparator scans only on request, groups masters and keeps playback running', async ({ page }) => {
  await page.setViewportSize({ width:1920, height:1080 })
  await mount(page)
  await page.evaluate(() => {
    window.testPlayCalls = 0
    HTMLMediaElement.prototype.load = function () {}
    HTMLMediaElement.prototype.play = function () { window.testPlayCalls += 1; return Promise.resolve() }
    HTMLMediaElement.prototype.pause = function () {}
  })
  expect(await page.evaluate(() => window.testCalls.some(x => x.cmd === 'scan_audio_folder'))).toBe(false)
  await page.getByRole('button', { name:'Comparer les masters' }).click()
  await page.getByRole('button', { name:'Choisir un dossier' }).click()
  await expect(page.locator('.library-summary')).toContainText('1 projet(s) détecté(s)')
  await expect(page.locator('.library-file')).toHaveCount(6)
  await expect(page.locator('.library-file').first().locator('.file-title strong')).toHaveText('track_faithful_0.wav')
  await expect(page.locator('.file-title strong').first()).toHaveCSS('white-space', 'normal')
  expect(await page.locator('.compare-shell').evaluate(element => element.getBoundingClientRect().width)).toBeGreaterThan(1500)
  await expect(page.locator('.library-file').first().locator('.file-title span')).toContainText('Modifié le')
  expect(await page.evaluate(() => window.testCalls.find(x => x.cmd === 'scan_audio_folder').args.root)).toBe('C:\\Audio\\track.wav')

  await page.getByText('Fichiers à classer (1)').click()
  await page.getByLabel('Projet de loose.wav').selectOption({ label:'track' })
  await expect(page.locator('.library-file')).toHaveCount(7)

  for (let index = 0; index < 4; index += 1) {
    await page.locator('.library-file').nth(index).getByRole('button', { name:'Ajouter à l’écoute' }).click()
    await expect(page.locator('.deck')).toHaveCount(index + 1)
  }
  await expect(page.locator('.library-file').nth(4).getByRole('button', { name:'Ajouter à l’écoute' })).toBeDisabled()
  await page.locator('.deck').nth(0).getByRole('button', { name:/Retirer/ }).click()
  await expect(page.locator('.library-file').nth(4).getByRole('button', { name:'Ajouter à l’écoute' })).toBeEnabled()
  await page.locator('.library-file').nth(4).getByRole('button', { name:'Ajouter à l’écoute' }).click()
  await expect(page.locator('.deck')).toHaveCount(4)
  await expect(page.getByRole('button', { name:/Volume égalisé/ })).toHaveAttribute('aria-pressed', 'true')
  await expect(page.getByText(/spécifique à cet encodeur; il ne certifie pas tous les services de diffusion/)).toBeVisible()
  await page.getByRole('button', { name:'Lecture' }).click()
  await expect(page.getByRole('button', { name:'Pause' })).toBeEnabled()
  await expect(page.locator('.compare-error')).toHaveCount(0)
  expect(await page.evaluate(() => window.testPlayCalls)).toBe(4)
  expect(await page.locator('.deck audio').first().getAttribute('src')).toContain('comparison-')
  await page.getByRole('button', { name:'Pause' }).click()
  await page.getByRole('button', { name:'Retirer de la bibliothèque' }).click()
  await expect(page.locator('.unlink-confirmation')).toContainText('resteront sur le disque')
  await page.getByRole('button', { name:'Retirer le projet' }).click()
  await expect(page.locator('.library-summary')).toContainText('0 projet(s) détecté(s)')
  await expect(page.locator('.library-summary')).toContainText('7 à classer')
  await expect(page.getByText('Fichiers à classer (7)')).toBeVisible()
})

test('comparator identifies a failed track and stops every deck', async ({ page }) => {
  await mount(page)
  await page.evaluate(() => {
    window.testPauseCalls = 0
    Object.defineProperty(HTMLMediaElement.prototype, 'error', { configurable:true, get:() => null })
    HTMLMediaElement.prototype.load = function () {}
    HTMLMediaElement.prototype.play = function () {
      return this.src.includes('faithful') ? Promise.reject(new DOMException('Audio device unavailable', 'NotReadableError')) : Promise.resolve()
    }
    HTMLMediaElement.prototype.pause = function () { window.testPauseCalls += 1 }
  })
  await page.getByRole('button', { name:'Comparer les masters' }).click()
  await page.getByRole('button', { name:'Choisir un dossier' }).click()
  for (let index = 0; index < 2; index++) {
    await page.locator('.library-file').nth(index).getByRole('button', { name:'Ajouter à l’écoute' }).click()
    await expect(page.locator('.deck')).toHaveCount(index + 1)
  }
  await page.getByRole('button', { name:'Lecture', exact:true }).click()
  await expect(page.getByRole('alert')).toContainText('track_faithful_0.wav')
  await expect(page.getByRole('alert')).toContainText('Audio device unavailable')
  await expect(page.getByRole('button', { name:'Lecture', exact:true })).toBeEnabled()
  expect(await page.evaluate(() => window.testPauseCalls)).toBeGreaterThanOrEqual(2)
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
  // Completing one engine pass must not imply that all Auto profiles are ready.
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','11')
  await page.evaluate(() => window.testEmit('mastering-progress', 0))
  await page.evaluate(() => window.testEmit('mastering-progress', 100))
  await expect(page.getByRole('progressbar')).toHaveAttribute('value','22')
  await page.evaluate(() => window.finishAuto({sessionId:'session',sourcePath:'C:\\Temp\\source.wav',source:window.testMeasurement(-10),sourceSegment:window.testMeasurement(-7),targetLufs:-4.5,variants:[window.testVariant('faithful','Fidèle',-4.7),window.testVariant('dense','Dense',-4.5),window.testVariant('aggressive','Agressif',-4.4)],recommendedId:'dense',recommendation:'Dense is selected as the middle trade-off.'}))
  await expect(page.getByText('Three profiles ready')).toBeVisible()
  await expect(page.locator('.variant')).toHaveCount(3)
  await expect(page.getByText('Dense diagnostic.')).toBeVisible()
  await expect(page.getByRole('button', { name:'Export AAC-safe delivery (.m4a)' })).toBeVisible()
  await expect(page.getByText(/AAC 256 kb\/s is a separate delivery copy/)).toBeVisible()
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
