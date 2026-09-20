<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { currentMonitor, getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";

type Status = "Idle" | "Ready" | "Processing" | "Succeeded" | "Failed";
type Track = { path: string; name: string; extension: string };
type Segment = { startSeconds: number; durationSeconds: number };
type Waveform = { durationSeconds: number; peaks: number[] };
type Measurements = { integratedLufs: number; truePeakDbtp: number; peakFactorDb: number; bassRatioDb: number; bandEnergyDb: number[]; aacTruePeakDbtp?: number };
type Variant = { id: string; profileId: string; profileLabel: string; targetLufs: number; engineReferenceDb: number; achievedDeltaLu: number; attempts: number; path: string; measurements: Measurements; segmentMeasurements: Measurements; preserveBass: boolean; peakFactorLossDb: number; bassChangeDb: number; aacRisk: boolean; diagnostics: string[]; referenceSimilarity?: number };
type AutoResult = { sessionId: string; sourcePath: string; source: Measurements; sourceSegment: Measurements; targetLufs: number; referencePath?: string; referenceSegment?: Measurements; referenceStartSeconds?: number; usingDefaultReference?: boolean; variants: Variant[]; recommendedId: string; recommendation: string };
type MasterResult = { output: string; source: Measurements; master: Measurements };

const status = ref<Status>("Idle");
const track = ref<Track | null>(null);
const reference = ref<Track | null>(null);
const sourceWaveform = ref<Waveform | null>(null);
const referenceWaveform = ref<Waveform | null>(null);
const progress = ref(0);
const error = ref("");
const output = ref("");
const dragging = ref(false);
const connected = ref(false);
const loading = ref(false);
const helpOpen = ref(false);
const helpTrigger = ref<HTMLButtonElement | null>(null);
const helpCloseButton = ref<HTMLButtonElement | null>(null);
const mode = ref<"manual" | "auto">("manual");
const auto = ref<AutoResult | null>(null);
const manual = ref<MasterResult | null>(null);
const selected = ref("");
const exporting = ref(false);
const exportedVariantIds = ref<string[]>([]);
const previewPlayer = ref<HTMLAudioElement | null>(null);
const settings = reactive({ loudness: -9, intensity: 1, preserveBass: false, dynamicBass: true, softClipDb: 0.5 });
const autoSettings = reactive({ targetLufs: -5, sourceStart: 0, sourceDuration: 30, referenceStart: 0, referenceDuration: 30 });
const busy = computed(() => status.value === "Processing");
const hasResult = computed(() => Boolean(auto.value || manual.value));
const stage = computed<"import" | "setup" | "processing" | "result">(() => !track.value ? "import" : busy.value ? "processing" : hasResult.value ? "result" : "setup");
const sourceSegment = computed<Segment>(() => ({ startSeconds: autoSettings.sourceStart, durationSeconds: autoSettings.sourceDuration }));
const referenceSegment = computed<Segment>(() => ({ startSeconds: autoSettings.referenceStart, durationSeconds: autoSettings.referenceDuration }));
try {
  const saved = JSON.parse(localStorage.getItem("lyte-settings") || "null");
  if (saved) {
    for (const key of ["loudness", "intensity", "softClipDb"] as const) if (Number.isFinite(saved[key])) settings[key] = saved[key];
    for (const key of ["preserveBass", "dynamicBass"] as const) if (typeof saved[key] === "boolean") settings[key] = saved[key];
    if (Number.isFinite(saved.targetLufs) && saved.targetLufs >= -9 && saved.targetLufs <= -2) autoSettings.targetLufs = saved.targetLufs;
    if (saved.mode === "auto" || saved.mode === "manual") mode.value = saved.mode;
  }
} catch {}
watch([settings, autoSettings, mode], () => { try { localStorage.setItem("lyte-settings", JSON.stringify({ ...settings, targetLufs: autoSettings.targetLufs, mode: mode.value })); } catch {} }, { deep: true });

const cleanup: UnlistenFn[] = [];
let completionSoundContext: AudioContext | undefined;
function fail(reason: unknown) { error.value = String(reason); status.value = "Failed"; }
function resetResults() { auto.value = null; manual.value = null; selected.value = ""; exportedVariantIds.value = []; exporting.value = false; previewPlayer.value?.pause(); }
function restart() { if (busy.value || loading.value) return; track.value = null; reference.value = null; sourceWaveform.value = null; referenceWaveform.value = null; status.value = "Idle"; error.value = ""; output.value = ""; progress.value = 0; resetResults(); }
async function load(path: string) { if (busy.value || loading.value) return; loading.value = true; error.value = ""; output.value = ""; progress.value = 0; resetResults(); try { track.value = await invoke<Track>("inspect_track", { path }); sourceWaveform.value = await invoke<Waveform>("inspect_waveform", { path }); autoSettings.sourceStart = 0; status.value = "Ready"; } catch (e) { track.value = null; sourceWaveform.value = null; fail(e); } finally { loading.value = false; } }
async function choose() { try { const path = await open({ multiple: false, directory: false, filters: [{ name: "Audio", extensions: ["wav", "flac", "mp3"] }] }); if (typeof path === "string") await load(path); } catch (e) { fail(e); } }
async function chooseReference() { try { const path = await open({ multiple: false, directory: false, filters: [{ name: "Reference audio", extensions: ["wav", "flac", "mp3"] }] }); if (typeof path === "string") { reference.value = await invoke<Track>("inspect_track", { path }); referenceWaveform.value = await invoke<Waveform>("inspect_waveform", { path }); autoSettings.referenceStart = 0; error.value = ""; } } catch (e) { reference.value = null; referenceWaveform.value = null; fail(e); } }
function primeCompletionSound() { const Ctor = window.AudioContext ?? (window as Window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext; if (!Ctor) return; try { completionSoundContext ??= new Ctor(); if (completionSoundContext.state === "suspended") void completionSoundContext.resume(); } catch {} }
function completionSound() { const Ctor = window.AudioContext ?? (window as Window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext; if (!Ctor) return; try { completionSoundContext ??= new Ctor(); const play = () => { const c = completionSoundContext!; const gain = c.createGain(); const oscillator = c.createOscillator(); const now = c.currentTime; oscillator.frequency.setValueAtTime(880, now); gain.gain.setValueAtTime(.001, now); gain.gain.exponentialRampToValueAtTime(.5, now + .015); gain.gain.exponentialRampToValueAtTime(.001, now + .2); oscillator.connect(gain); gain.connect(c.destination); oscillator.start(now); oscillator.stop(now + .22); }; if (completionSoundContext.state === "suspended") void completionSoundContext.resume().then(play); else play(); } catch {} }
async function master() {
  if (!track.value || busy.value || loading.value) return;
  primeCompletionSound(); status.value = "Processing"; progress.value = 0; error.value = ""; output.value = ""; resetResults();
  try {
    if (mode.value === "auto") {
      auto.value = await invoke<AutoResult>("start_auto_mastering", { options: { input: track.value.path, targetLufs: autoSettings.targetLufs, sourceSegment: sourceSegment.value, reference: reference.value ? { input: reference.value.path, segment: referenceSegment.value } : null } });
      selected.value = auto.value.recommendedId;
    } else {
      manual.value = await invoke<MasterResult>("start_mastering", { options: { input: track.value.path, ...settings } }); output.value = manual.value.output;
    }
    progress.value = 100; status.value = "Succeeded"; completionSound();
  } catch (e) { fail(e); }
}
async function cancel() { try { await invoke("cancel_mastering"); } catch (e) { fail(e); } }
async function exportSelected() { if (!auto.value || !selected.value || exporting.value || exportedVariantIds.value.includes(selected.value)) return; exporting.value = true; error.value = ""; try { output.value = await invoke<string>("export_auto_master", { sessionId: auto.value.sessionId, variantId: selected.value }); exportedVariantIds.value = [...exportedVariantIds.value, selected.value]; } catch (e) { fail(e); } finally { exporting.value = false; } }
async function openFolder() { try { await invoke("open_output_folder"); } catch (e) { fail(e); } }
async function openHelpLink(project: "bakuage" | "phaselimiter") { try { await invoke("open_help_link", { project }); } catch (e) { fail(e); } }
function preview(path: string, lufs: number, start = 0) { const player = previewPlayer.value; if (!player) return fail("Preview is not ready yet. Please try again."); const values = auto.value ? [auto.value.source.integratedLufs, ...auto.value.variants.map(v => v.measurements.integratedLufs), auto.value.referenceSegment?.integratedLufs].filter((value): value is number => Number.isFinite(value)) : [lufs]; player.pause(); player.src = convertFileSrc(path); player.volume = Math.pow(10, Math.min(0, Math.min(...values) - lufs) / 20); player.onloadedmetadata = () => { player.currentTime = Math.max(0, start); void player.play().catch(() => fail("Preview could not start. Run mastering again and retry.")); }; player.load(); }
function stopPreview() { previewPlayer.value?.pause(); }
function display(value: number | undefined) { return Number.isFinite(value) ? Number(value).toFixed(1) : "—"; }
function formatTime(seconds: number) { const rounded = Math.max(0, Math.round(seconds)); return Math.floor(rounded / 60) + ":" + String(rounded % 60).padStart(2, "0"); }
function waveformPath(waveform: Waveform | null) {
  if (!waveform?.peaks.length) return "";
  return waveform.peaks.map((peak, index) => {
    const x = ((index + 0.5) / waveform.peaks.length) * 100;
    const height = Math.max(4, Math.min(46, peak * 46));
    return "M " + x.toFixed(2) + " " + (50 - height).toFixed(2) + " V " + (50 + height).toFixed(2);
  }).join(" ");
}
function selectionStyle(waveform: Waveform | null, start: number, duration: number) {
  if (!waveform?.durationSeconds) return { left: "0%", width: "0%" };
  const safeDuration = Math.min(duration, waveform.durationSeconds);
  const safeStart = Math.min(Math.max(0, start), Math.max(0, waveform.durationSeconds - safeDuration));
  return { left: ((safeStart / waveform.durationSeconds) * 100) + "%", width: Math.min(100, (safeDuration / waveform.durationSeconds) * 100) + "%" };
}
const sourceWavePath = computed(() => waveformPath(sourceWaveform.value));
const referenceWavePath = computed(() => waveformPath(referenceWaveform.value));
const sourceSelectionStyle = computed(() => selectionStyle(sourceWaveform.value, autoSettings.sourceStart, autoSettings.sourceDuration));
const referenceSelectionStyle = computed(() => selectionStyle(referenceWaveform.value, autoSettings.referenceStart, autoSettings.referenceDuration));
function clampPassage(kind: "source" | "reference") {
  const waveform = kind === "source" ? sourceWaveform.value : referenceWaveform.value;
  if (!waveform) return;
  const duration = kind === "source" ? autoSettings.sourceDuration : autoSettings.referenceDuration;
  const maxStart = Math.max(0, waveform.durationSeconds - Math.min(duration, waveform.durationSeconds));
  if (kind === "source") autoSettings.sourceStart = Math.min(Math.max(0, autoSettings.sourceStart), maxStart);
  else autoSettings.referenceStart = Math.min(Math.max(0, autoSettings.referenceStart), maxStart);
}
function setPassage(kind: "source" | "reference", event: PointerEvent) {
  const waveform = kind === "source" ? sourceWaveform.value : referenceWaveform.value;
  if (!waveform) return;
  const duration = kind === "source" ? autoSettings.sourceDuration : autoSettings.referenceDuration;
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const position = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
  const maxStart = Math.max(0, waveform.durationSeconds - Math.min(duration, waveform.durationSeconds));
  const start = Math.min(maxStart, Math.max(0, position * waveform.durationSeconds - duration / 2));
  if (kind === "source") autoSettings.sourceStart = Math.round(start * 10) / 10;
  else autoSettings.referenceStart = Math.round(start * 10) / 10;
}
function nudgePassage(kind: "source" | "reference", seconds: number) {
  if (kind === "source") autoSettings.sourceStart += seconds;
  else autoSettings.referenceStart += seconds;
  clampPassage(kind);
}
function closeHelp() { helpOpen.value = false; void nextTick(() => helpTrigger.value?.focus()); }
function onHelpKeydown(event: KeyboardEvent) { if (event.key === "Escape") closeHelp(); }
let resizeObserver: ResizeObserver | undefined; let resizeScheduled = false;
function scheduleWindowFit() { if (resizeScheduled) return; resizeScheduled = true; requestAnimationFrame(() => { resizeScheduled = false; void fitWindowToContent(); }); }
async function fitWindowToContent() { try { const window = getCurrentWindow(); if (await window.isMaximized()) return; const scale = await window.scaleFactor(); const current = (await window.innerSize()).toLogical(scale); const monitor = await currentMonitor(); const max = monitor ? monitor.workArea.size.toLogical(monitor.scaleFactor).height - 48 : Infinity; const documentHeight = Math.max(document.documentElement.scrollHeight, document.body.scrollHeight, Math.ceil(document.querySelector("main")?.scrollHeight ?? 0)); const height = Math.min(Math.max(680, documentHeight + 16), Math.max(680, max)); if (Math.abs(current.height - height) > 4) await window.setSize(new LogicalSize(Math.max(800, current.width), height)); } catch {} }
onMounted(async () => { try { cleanup.push(await listen<number>("mastering-progress", e => { if (busy.value) progress.value = Math.max(progress.value, Math.min(90, Math.round(e.payload * .9))); })); cleanup.push(await getCurrentWebview().onDragDropEvent(e => { dragging.value = !busy.value && (e.payload.type === "over" || e.payload.type === "enter"); if (e.payload.type === "drop" && !busy.value && !loading.value) { dragging.value = false; if (e.payload.paths.length !== 1) fail("Please drop one audio file at a time."); else void load(e.payload.paths[0]); } })); connected.value = true; } catch { error.value = "Desktop connection unavailable. Launch LYTE using npm run dev."; } const main = document.querySelector("main"); resizeObserver = new ResizeObserver(scheduleWindowFit); if (main) resizeObserver.observe(main); scheduleWindowFit(); });
watch(stage, async () => { await nextTick(); scheduleWindowFit(); }, { flush: "post" });
watch([sourceWaveform, () => autoSettings.sourceDuration], () => clampPassage("source"));
watch([referenceWaveform, () => autoSettings.referenceDuration], () => clampPassage("reference"));
watch(helpOpen, async isOpen => { if (isOpen) { await nextTick(); helpCloseButton.value?.focus(); } });
onUnmounted(() => { cleanup.forEach(fn => fn()); resizeObserver?.disconnect(); previewPlayer.value?.pause(); void completionSoundContext?.close(); });
</script>

<template>
  <main>
    <audio ref="previewPlayer" preload="auto" @ended="stopPreview" />
    <header><div class="brand"><span class="mark" aria-hidden="true">L</span><h1>LYTE <span>Mastering</span></h1></div><span class="local"><i></i> LOCAL AUDIO</span></header>
    <div class="utility-links"><button ref="helpTrigger" class="help-link" type="button" :aria-expanded="helpOpen" aria-controls="help" @click="helpOpen = true">Aide</button></div>
    <Teleport v-if="helpOpen" to="body"><div class="help-backdrop" @click.self="closeHelp"><section id="help" class="help" role="dialog" aria-modal="true" aria-labelledby="help-title" tabindex="-1" @keydown="onHelpKeydown"><div class="help-heading"><div><p class="eyebrow">GUIDE RAPIDE</p><h2 id="help-title">Masterisez sur votre ordinateur.</h2></div><button ref="helpCloseButton" class="help-close" type="button" aria-label="Fermer l’aide" @click="closeHelp">×</button></div><p>LYTE traite vos fichiers localement avec PhaseLimiter. Aucun morceau ni réglage n’est envoyé en ligne.</p><ol class="help-steps"><li><strong>Manual</strong> conserve les réglages directs de PhaseLimiter.</li><li><strong>Auto — Hard Techno</strong> vise un LUFS mesuré, rend Fidèle, Dense et Agressif, puis affiche les compromis.</li><li>Une <strong>référence locale</strong> est optionnelle : elle compare deux passages sans copier son égalisation.</li></ol><div class="help-tech"><strong>Mesures Auto</strong><ul><li>LUFS, true peak, facteur de crête, grave 30–150 Hz et quatre bandes d’énergie.</li><li>Chaque profil peut recalibrer sa consigne jusqu’à trois fois pour s’approcher de la cible à ±0,3 LUFS.</li><li>La sortie reste plafonnée à −1 dBTP ; la simulation AAC est un avertissement de diffusion.</li></ul></div><div class="help-tech"><strong>Projets audio à l’origine du moteur</strong><ul><li><a href="https://github.com/ai-mastering/bakuage" @click.prevent="openHelpLink('bakuage')">Bakuage</a></li><li><a href="https://github.com/ai-mastering/phaselimiter" @click.prevent="openHelpLink('phaselimiter')">PhaseLimiter</a></li></ul></div></section></div></Teleport>
    <section v-if="stage === 'import'" class="intro"><p class="eyebrow">THE FINAL TOUCH</p><h2>Make your track ready.</h2><p>Local mastering. Powered by PhaseLimiter.</p></section>
    <button v-if="stage === 'import'" class="drop" :class="{ dragging }" :disabled="loading || !connected" @click="choose"><strong>Drop a track here</strong><span>or <u>choose a file</u></span><small>WAV · FLAC · MP3</small></button>
    <section v-else class="track-summary" aria-label="Selected track"><div><span>TRACK READY</span><strong class="filename">{{ track?.name }}</strong><small>{{ track?.extension }} · Ready to master</small></div><button class="text-button" :disabled="busy" @click="restart">Start over</button></section>
    <template v-if="stage === 'setup'">
      <div class="mode-switch" role="group" aria-label="Mastering mode"><button :class="{ active: mode === 'manual' }" @click="mode = 'manual'">Manual</button><button :class="{ active: mode === 'auto' }" @click="mode = 'auto'">Auto — Hard Techno</button></div>
      <fieldset><legend class="sr-only">Mastering settings</legend>
        <template v-if="mode === 'manual'"><div class="control"><label for="loudness">PhaseLimiter reference</label><output>{{ settings.loudness.toFixed(1) }} <span>dB</span></output><input id="loudness" v-model.number="settings.loudness" type="range" min="-20" max="0" step="0.1" /><div class="scale"><span>−20 dB</span><span>0 dB</span></div></div><div class="control"><label for="intensity">Mastering intensity</label><output>{{ settings.intensity.toFixed(2) }}</output><input id="intensity" v-model.number="settings.intensity" type="range" min="0" max="1" step="0.01" /></div><label class="bass"><span>Preserve bass</span><input id="bass" v-model="settings.preserveBass" type="checkbox" role="switch" /><span class="toggle" aria-hidden="true"></span></label><label class="bass"><span>Dynamic low end</span><input id="dynamic-bass" v-model="settings.dynamicBass" type="checkbox" role="switch" /><span class="toggle" aria-hidden="true"></span></label><div class="control"><label for="soft-clip">Soft clip</label><output>{{ settings.softClipDb.toFixed(1) }} <span>dB</span></output><input id="soft-clip" v-model.number="settings.softClipDb" type="range" min="0" max="2" step="0.1" /></div></template>
        <template v-else>
          <div class="auto-note"><strong>Measured Hard Techno target</strong><span>Three profiles aim for the same output LUFS. A built-in Hard Techno reference ranks their dynamics and tonal balance.</span></div>
          <div class="control"><label for="auto-target">Target loudness</label><output>{{ autoSettings.targetLufs.toFixed(1) }} <span>LUFS</span></output><input id="auto-target" v-model.number="autoSettings.targetLufs" type="range" min="-9" max="-2" step="0.1" /><div class="scale"><span>−9 LUFS</span><span>−2 LUFS</span></div><button class="reference-target" type="button" @click="autoSettings.targetLufs = -2.9">Use integrated Hard Techno level · −2.9 LUFS</button></div>
          <section class="passage">
            <div class="passage-heading"><strong>Your passage</strong><output>{{ formatTime(autoSettings.sourceStart) }} – {{ formatTime(autoSettings.sourceStart + autoSettings.sourceDuration) }}</output></div>
            <small id="source-passage-help">Click the waveform to place the selected passage. Choose 15–60 seconds; use left and right arrows for fine adjustment.</small>
            <button v-if="sourceWaveform" data-testid="source-waveform" class="waveform-picker" type="button" aria-label="Choose your passage on the waveform" aria-describedby="source-passage-help" @pointerdown="setPassage('source', $event)" @keydown.left.prevent="nudgePassage('source', -1)" @keydown.right.prevent="nudgePassage('source', 1)">
              <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true"><path :d="sourceWavePath" /></svg>
              <span class="waveform-selection" :style="sourceSelectionStyle"></span>
            </button>
            <p v-else class="waveform-loading">Preparing waveform…</p>
            <label class="passage-duration">Duration <output>{{ autoSettings.sourceDuration }} s</output><input v-model.number="autoSettings.sourceDuration" type="range" min="15" max="60" step="1" /></label>
          </section>
          <section class="passage reference-setup">
            <div><strong>Professional reference <em>optional</em></strong><small>{{ reference ? 'Your reference will replace the built-in Hard Techno reference.' : 'Built-in Hard Techno reference active.' }}</small></div>
            <button class="text-button" type="button" @click="chooseReference">{{ reference ? 'Replace reference' : 'Choose your own reference' }}</button>
            <template v-if="reference">
              <div class="passage-heading"><strong>Reference passage</strong><output>{{ formatTime(autoSettings.referenceStart) }} – {{ formatTime(autoSettings.referenceStart + autoSettings.referenceDuration) }}</output></div>
              <small id="reference-passage-help">Click the waveform to choose the comparison passage; use left and right arrows for fine adjustment.</small>
              <button v-if="referenceWaveform" data-testid="reference-waveform" class="waveform-picker" type="button" aria-label="Choose the reference passage on the waveform" aria-describedby="reference-passage-help" @pointerdown="setPassage('reference', $event)" @keydown.left.prevent="nudgePassage('reference', -1)" @keydown.right.prevent="nudgePassage('reference', 1)">
                <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true"><path :d="referenceWavePath" /></svg>
                <span class="waveform-selection" :style="referenceSelectionStyle"></span>
              </button>
              <p v-else class="waveform-loading">Preparing waveform…</p>
              <label class="passage-duration">Reference duration <output>{{ autoSettings.referenceDuration }} s</output><input v-model.number="autoSettings.referenceDuration" type="range" min="15" max="60" step="1" /></label>
              <button class="text-button remove-reference" type="button" @click="reference = null; referenceWaveform = null">Use built-in reference</button>
            </template>
          </section>
        </template>
      </fieldset>
      <button class="primary" :disabled="loading || !connected" @click="master">{{ mode === 'auto' ? 'RENDER 3 PROFILES' : 'MASTER TRACK' }}<span aria-hidden="true">↗</span></button>
    </template>
    <section class="result" aria-live="polite" :aria-busy="busy"><template v-if="busy"><div class="result-row"><strong>{{ mode === 'auto' ? 'Calibrating three Hard Techno profiles…' : 'Mastering…' }}</strong><span>{{ Math.floor(progress) }}%</span></div><progress :value="progress" max="100" /><p>{{ mode === 'auto' ? 'Each profile may render up to three times to approach the requested LUFS.' : 'Working on your track. This may take a few minutes.' }}</p><button class="secondary" @click="cancel">Cancel mastering</button></template><template v-else-if="auto"><div class="result-row"><strong class="success">✓ Three profiles ready</strong><span>Target {{ display(auto.targetLufs) }} LUFS · −1 dBTP ceiling</span></div><aside class="recommendation"><strong>Recommendation</strong><p>{{ auto.recommendation }}</p></aside><div class="source-row"><span>Source</span><b>{{ display(auto.source.integratedLufs) }} LUFS</b><button @click="preview(auto.sourcePath, auto.source.integratedLufs, autoSettings.sourceStart)">Play passage</button></div><div v-if="auto.referencePath && auto.referenceSegment" class="source-row"><span>{{ auto.usingDefaultReference ? 'Hard Techno reference' : 'Reference' }}</span><b>{{ display(auto.referenceSegment.integratedLufs) }} LUFS</b><button @click="preview(auto.referencePath!, auto.referenceSegment!.integratedLufs, 0)">Play passage</button></div><div class="ranking-heading"><strong>Compare at matched volume</strong><span>All three remain exportable.</span></div><div class="variants"><div v-for="variant in auto.variants" :key="variant.id" class="variant" :class="{ selected: selected === variant.id, winner: variant.id === auto.recommendedId }"><input :id="`variant-${variant.id}`" v-model="selected" type="radio" name="variant" :value="variant.id" /><label :for="`variant-${variant.id}`"><div class="variant-title"><strong>{{ variant.profileLabel }} <em v-if="variant.id === auto.recommendedId">Recommended</em></strong><span v-if="variant.referenceSimilarity !== undefined" class="score">Ref {{ variant.referenceSimilarity }}/100</span></div><small>Target {{ display(variant.targetLufs) }} · achieved {{ display(variant.measurements.integratedLufs) }} LUFS · {{ variant.achievedDeltaLu >= 0 ? '+' : '' }}{{ display(variant.achievedDeltaLu) }} LU · {{ variant.attempts }} attempt{{ variant.attempts > 1 ? 's' : '' }}</small><div class="metric-grid"><span>True peak <b>{{ display(variant.measurements.truePeakDbtp) }} dBTP</b></span><span>Impact <b>{{ variant.peakFactorLossDb >= 0 ? '−' : '+' }}{{ display(Math.abs(variant.peakFactorLossDb)) }} dB</b></span><span>Low end <b>{{ variant.bassChangeDb >= 0 ? '+' : '' }}{{ display(variant.bassChangeDb) }} dB</b></span><span :class="{ unsafe: variant.aacRisk }">AAC <b>{{ display(variant.measurements.aacTruePeakDbtp) }} dBTP</b></span></div><ul class="diagnostics"><li v-for="note in variant.diagnostics" :key="note">{{ note }}</li></ul></label><button type="button" @click="preview(variant.path, variant.measurements.integratedLufs, autoSettings.sourceStart)">Play</button></div></div><p class="metric-note">The reference score compares the selected passages only. It is a measurement aid, never a quality grade or tonal copy.</p><div class="actions"><button class="secondary" @click="stopPreview">Stop preview</button><button class="secondary export" :disabled="!selected || exporting || exportedVariantIds.includes(selected)" @click="exportSelected">{{ exporting ? 'Exporting…' : exportedVariantIds.includes(selected) ? '✓ Master exported' : 'Export selected master' }}</button></div><template v-if="output"><p class="filename">{{ output.split(/[\\/]/).pop() }}</p><button class="secondary" @click="openFolder">Open folder ↗</button></template></template><template v-else-if="manual"><div class="result-row"><strong class="success">✓ Master complete</strong><span>WAV</span></div><p class="metric-note">Export: {{ display(manual.master.integratedLufs) }} LUFS · {{ display(manual.master.truePeakDbtp) }} dBTP<span v-if="manual.master.aacTruePeakDbtp !== undefined"> · AAC {{ display(manual.master.aacTruePeakDbtp) }} dBTP</span>.</p><p class="filename">{{ output.split(/[\\/]/).pop() }}</p><button class="secondary" @click="openFolder">Open folder ↗</button></template><p v-else-if="!error" class="hint">{{ track ? 'Your master will be saved beside the original.' : 'Choose a track to get started.' }}</p><p v-if="error" role="alert" class="error">{{ error }}</p></section>
    <footer><span>PHASELIMITER ENGINE</span><span>Always on your computer.</span></footer>
  </main>
</template>
