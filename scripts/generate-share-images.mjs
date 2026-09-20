import { chromium } from '@playwright/test';
import fs from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
const output = fileURLToPath(new URL('../showcase/assets/', import.meta.url));
await fs.mkdir(output, { recursive: true });
const browser = await chromium.launch({headless:true, channel:process.env.PLAYWRIGHT_CHANNEL || "msedge"});
try {
 const page = await browser.newPage({viewport:{width:1200,height:630},deviceScaleFactor:1});
 const logo = await fs.readFile(new URL('../showcase/favicon.svg',import.meta.url),'utf8');
 for (const lang of ['fr','en']) {
  const fr = lang === 'fr';
  const bars = Array.from({length:58},(_,i)=>`<i style="height:${18+Math.abs(Math.sin(i*1.7)*Math.cos(i*.23))*145}px"></i>`).join('');
  await page.setContent(`<!doctype html><html lang="${lang}"><meta charset="utf-8"><style>
   *{box-sizing:border-box}body{margin:0;width:1200px;height:630px;background:#f1f5f7;color:#17212b;font-family:'Segoe UI',sans-serif;padding:55px 64px;overflow:hidden}.brand{display:flex;align-items:center;gap:15px;font-size:27px;font-weight:700;letter-spacing:1px}.brand svg{width:49px;height:49px}.tag{position:absolute;right:64px;top:64px;font-size:18px;letter-spacing:2px;color:#47616a}.layout{display:grid;grid-template-columns:650px 1fr;align-items:center;gap:26px;margin-top:53px}h1{font-size:65px;line-height:1.08;letter-spacing:-3px;margin:0;font-weight:750}h1 em{font-family:Georgia,serif;font-weight:400;color:#315d6a}p{font-size:24px;color:#49616b;line-height:1.5;margin:24px 0 0}.audio{background:#101b20;border:1px solid #38545b;border-radius:22px;padding:28px 24px;box-shadow:12px 16px 0 #dde7ea;transform:rotate(2deg)}.label{font-size:13px;letter-spacing:2px;color:#b9dfd4}.wave{height:173px;display:flex;align-items:center;justify-content:center;gap:3px;margin-top:12px}.wave i{width:3px;background:#a4d7c7;border-radius:4px}.mode{border-top:1px solid #38545b;padding-top:18px;color:#d8f3e9;font-size:17px}.foot{position:absolute;bottom:51px;left:64px;right:64px;display:flex;align-items:center;justify-content:space-between;border-top:1px solid #cbd9df;padding-top:23px;font-size:19px;color:#49616b}.pill{background:#d8f3e9;padding:9px 17px;border-radius:8px;color:#234d42;font-weight:650}
  </style><div class="brand">${logo}LYTE Mastering</div><div class="tag">WINDOWS x64</div><div class="layout"><div><h1>${fr?'Mastering audio.<br><em>Gratuit. Local.</em>':'Audio mastering.<br><em>Free. Local.</em>'}</h1><p>${fr?'Votre son. Votre ordinateur.<br>Sans compte ni abonnement.':'Your sound. Your computer.<br>No account. No subscription.'}</p></div><div class="audio"><div class="label">LYTE / LOCAL AUDIO</div><div class="wave">${bars}</div><div class="mode">Auto — Hard Techno</div></div></div><div class="foot"><span>mastering.mathieuluyten.be</span><span class="pill">WAV · FLAC · MP3</span></div></html>`);
  await page.screenshot({path:output+`lyte-mastering-share-${lang}.png`});
 }
 for (const [name,size] of [['favicon-96',96],['apple-touch-icon',180]]) {
  await page.setViewportSize({width:size,height:size});
  await page.setContent(`<style>body{margin:0}svg{display:block;width:${size}px;height:${size}px}</style>${logo}`);
  await page.screenshot({path:output+name+'.png',omitBackground:true});
 }
 console.log('Generated FR/EN share images (1200 x 630) and PNG icons.');
} finally { await browser.close(); }

