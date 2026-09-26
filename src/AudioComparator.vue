<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

type LibraryFile = { path: string; relativePath: string; name: string; extension: string; projectHint?: string | null; kind: string; profile?: string | null; bytes: number; modifiedAtMs?: number | null };
type LibraryScan = { root: string; files: LibraryFile[]; scannedEntries: number; errors: string[]; canceled: boolean };
type ScanProgress = { scannedEntries: number; audioFiles: number };
type Measurements = { integratedLufs: number; truePeakDbtp: number; peakFactorDb: number; bassRatioDb: number; aacTruePeakDbtp?: number | null };
type Analysis = { playbackPath: string; track: { path: string; name: string; extension: string }; waveform: { durationSeconds: number; peaks: number[] }; measurements: Measurements };
type Slot = { file: LibraryFile; analysis: Analysis };
type Project = { id: string; title: string; files: LibraryFile[] };
const emit = defineEmits<{ close: []; layout: [] }>();

const storeKey = "lyte-comparison-library-v1";
function loadSaved() {
  try { return JSON.parse(localStorage.getItem(storeKey) || "{}") as { root?: string; assignments?: Record<string, string>; customProjects?: Record<string, string> }; }
  catch { return {}; }
}
const saved = loadSaved();
const root = ref(saved.root || "");
const files = ref<LibraryFile[]>([]);
const scanErrors = ref<string[]>([]);
const scanBusy = ref(false);
const scanCanceled = ref(false);
const progress = ref<ScanProgress>({ scannedEntries: 0, audioFiles: 0 });
const scanError = ref("");
const assignments = ref<Record<string, string>>(saved.assignments || {});
const customProjects = ref<Record<string, string>>(saved.customProjects || {});
const selectedProjectId = ref("");
const unlinkConfirmationId = ref("");
const newProjectName = ref("");
const slots = ref<Slot[]>([]);
const activePath = ref("");
const playing = ref(false);
const matched = ref(true);
const playhead = ref(0);
const trackError = ref("");
const analyzing = ref<string[]>([]);
const announce = ref("");
const audioRefs = new Map<string, HTMLAudioElement>();
let unlistenProgress: UnlistenFn | undefined;
const activeSlot = computed(() => slots.value.find(slot => slot.file.path === activePath.value) || null);
const duration = computed(() => activeSlot.value?.analysis.waveform.durationSeconds || 0);
const comparisonTarget = computed(() => slots.value.length ? Math.min(...slots.value.map(slot => slot.analysis.measurements.integratedLufs)) : 0);

function persist() {
  try { localStorage.setItem(storeKey, JSON.stringify({ root: root.value, assignments: assignments.value, customProjects: customProjects.value })); } catch { /* Private browsing can disable local preferences. */ }
}
function normalizedFolder(path: string) { return path.replaceAll("\\", "/").replace(/\/+$/, "").toLocaleLowerCase(); }
function displayProfile(profile: string | null | undefined) {
  return ({ faithful: "Fidèle", dense: "Dense", aggressive: "Agressif", Manual: "Manuel", "Auto ancien": "Auto ancien" } as Record<string, string>)[profile || ""] || profile || "Master";
}
function folderOf(relativePath: string) { const index = Math.max(relativePath.lastIndexOf("/"), relativePath.lastIndexOf("\\")); return index < 0 ? "" : relativePath.slice(0, index); }
function modificationTime(file: LibraryFile) { return file.modifiedAtMs ?? 0; }
function formatModified(timestamp?: number | null) {
  if (timestamp == null) return "Date inconnue";
  return new Date(timestamp).toLocaleString("fr-BE", { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" });
}
function canonicalTitle(title: string) { return title.trim().replace(/\s+\(\d{6}-\d{4}\)$/, "").replace(/\s+\d{6}-\d{4}$/, "").toLocaleLowerCase(); }
function defaultProjectId(file: LibraryFile) { return `${folderOf(file.relativePath).toLocaleLowerCase()}::${(file.projectHint || "").toLocaleLowerCase()}`; }
function projectFor(file: LibraryFile) {
  if (Object.prototype.hasOwnProperty.call(assignments.value, file.path)) return assignments.value[file.path];
  return file.projectHint ? defaultProjectId(file) : "";
}
const projects = computed<Project[]>(() => {
  const map = new Map<string, Project>();
  for (const file of files.value) {
    const id = projectFor(file);
    if (!id) continue;
    const title = customProjects.value[id] || file.projectHint || id.split("::").at(-1) || "Projet audio";
    const project = map.get(id) || { id, title, files: [] };
    project.files.push(file);
    map.set(id, project);
  }
  const automaticTitles = new Set([...map.values()].map(project => canonicalTitle(project.title)));
  for (const [id, title] of Object.entries(customProjects.value)) {
    if (!map.has(id) && !automaticTitles.has(canonicalTitle(title))) map.set(id, { id, title, files: [] });
  }
  const result = [...map.values()];
  for (const project of result) project.files.sort((left, right) => modificationTime(right) - modificationTime(left));
  return result.sort((left, right) => {
    const latestLeft = Math.max(0, ...left.files.map(modificationTime));
    const latestRight = Math.max(0, ...right.files.map(modificationTime));
    return latestRight - latestLeft || left.title.localeCompare(right.title, "fr");
  });
});
const unassigned = computed(() => files.value.filter(file => !projectFor(file)).sort((left, right) => modificationTime(right) - modificationTime(left)));
const projectFiles = computed(() => projects.value.find(project => project.id === selectedProjectId.value)?.files || []);

async function selectFolder() {
  try {
    const selected = await open({ directory: true, multiple: false, title: "Choisir le dossier local des projets audio" });
    if (typeof selected === "string") await scanFolder(selected);
  } catch (error) { scanError.value = String(error); }
}
async function scanFolder(folder = root.value) {
  if (!folder || scanBusy.value) return;
  if (root.value && normalizedFolder(folder) !== normalizedFolder(root.value)) await clearDecks();
  scanBusy.value = true; scanCanceled.value = false; scanError.value = ""; scanErrors.value = [];
  progress.value = { scannedEntries: 0, audioFiles: 0 };
  try {
    const result = await invoke<LibraryScan>("scan_audio_folder", { root: folder });
    const available = new Set(result.files.map(file => file.path));
    for (const slot of [...slots.value]) if (!available.has(slot.file.path)) removeTrack(slot.file.path);
    root.value = result.root; files.value = result.files; scanErrors.value = result.errors; scanCanceled.value = result.canceled;
    if (result.canceled) scanError.value = "Scan interrompu. Les fichiers déjà repérés restent affichés.";
    if (!selectedProjectId.value || !projects.value.some(project => project.id === selectedProjectId.value)) selectedProjectId.value = projects.value[0]?.id || "";
    persist();
  } catch (error) { scanError.value = String(error); }
  finally { scanBusy.value = false; }
}
async function cancelScan() { scanCanceled.value = true; try { await invoke("cancel_library_scan"); } catch {} }
async function onProgress() {
  try { unlistenProgress = await listen<ScanProgress>("library-scan-progress", event => { progress.value = event.payload; }); }
  catch { /* Progress is supplementary; the scan result still reports its totals. */ }
}
function assign(file: LibraryFile, event: Event) {
  const id = (event.target as HTMLSelectElement).value;
  assignments.value = { ...assignments.value, [file.path]: id };
  persist();
  if (id) selectedProjectId.value = id;
}
function createProject() {
  const title = newProjectName.value.trim();
  if (!title) return;
  const id = `manual:${globalThis.crypto?.randomUUID?.() || `${Date.now()}-${Math.random()}`}`;
  customProjects.value = { ...customProjects.value, [id]: title };
  selectedProjectId.value = id; newProjectName.value = ""; persist();
}
function requestUnlinkProject() { unlinkConfirmationId.value = selectedProjectId.value; }
function unlinkProject() {
  const id = unlinkConfirmationId.value;
  const project = projects.value.find(item => item.id === id);
  if (!project) { unlinkConfirmationId.value = ""; return; }
  const nextAssignments = { ...assignments.value };
  for (const file of project.files) nextAssignments[file.path] = "";
  assignments.value = nextAssignments;
  const nextCustomProjects = { ...customProjects.value };
  delete nextCustomProjects[id];
  customProjects.value = nextCustomProjects;
  unlinkConfirmationId.value = "";
  selectedProjectId.value = "";
  persist();
}
function projectLabel(id: string) { return customProjects.value[id] || projects.value.find(project => project.id === id)?.title || id; }
function setAudioRef(path: string, value: unknown) {
  if (value instanceof HTMLAudioElement) audioRefs.set(path, value);
  else audioRefs.delete(path);
}
function setVolume(slot: Slot) {
  const audio = audioRefs.get(slot.file.path);
  if (!audio) return;
  if (slot.file.path !== activePath.value) { audio.volume = 0; return; }
  const delta = matched.value ? comparisonTarget.value - slot.analysis.measurements.integratedLufs : 0;
  audio.volume = Math.min(1, Math.max(0, 10 ** (delta / 20)));
}
function updateVolumes() { for (const slot of slots.value) setVolume(slot); }
function selectDeck(path: string) {
  activePath.value = path; updateVolumes(); announce.value = `Piste active : ${slots.value.find(slot => slot.file.path === path)?.file.name || ""}`;
  const audio = audioRefs.get(path);
  if (audio && playing.value) {
    if (audio.readyState >= 1) audio.currentTime = Math.min(playhead.value, audio.duration || playhead.value);
    void audio.play().catch(() => { trackError.value = "Lecture bloquée par le système. Réessaie avec le bouton Lecture."; });
  }
}
async function addTrack(file: LibraryFile) {
  trackError.value = "";
  if (slots.value.some(slot => slot.file.path === file.path)) { selectDeck(file.path); return; }
  if (slots.value.length >= 4) { trackError.value = "Quatre pistes sont déjà chargées. Retire-en une avant d’en ajouter une autre."; return; }
  analyzing.value = [...analyzing.value, file.path];
  try {
    const analysis = await invoke<Analysis>("analyze_comparison_track", { path: file.path });
    slots.value = [...slots.value, { file, analysis }];
    if (!playing.value) activePath.value = file.path;
    announce.value = `Piste ajoutée : ${file.name}`;
    await nextTick();
    const audio = audioRefs.get(file.path);
    if (audio) { audio.volume = 0; audio.load(); }
    updateVolumes();
  } catch (error) { trackError.value = `${file.name} : ${String(error)}`; }
  finally { analyzing.value = analyzing.value.filter(path => path !== file.path); }
}
async function removeTrack(path: string) {
  releaseAudio(path);
  slots.value = slots.value.filter(slot => slot.file.path !== path);
  if (activePath.value === path) activePath.value = slots.value[0]?.file.path || "";
  updateVolumes();
  if (!slots.value.length) { playing.value = false; playhead.value = 0; }
  await nextTick();
  await invoke("revoke_comparison_track", { path }).catch(() => undefined);
}
async function clearDecks() {
  pause();
  const previous = [...slots.value];
  for (const slot of previous) releaseAudio(slot.file.path);
  slots.value = []; activePath.value = ""; playhead.value = 0;
  await nextTick();
  await Promise.all(previous.map(slot => invoke("revoke_comparison_track", { path: slot.file.path }).catch(() => undefined)));
}
async function play() {
  if (!slots.value.length) return;
  trackError.value = ""; updateVolumes();
  try {
    const audios: HTMLAudioElement[] = [];
    for (const slot of slots.value) {
      const audio = audioRefs.get(slot.file.path);
      if (!audio) throw new Error(`Lecteur indisponible pour ${slot.file.name}.`);
      audios.push(audio);
      try { audio.currentTime = Math.min(playhead.value, audio.duration || playhead.value); } catch { /* The browser keeps a pending seek until media metadata is ready. */ }
    }
    playing.value = true;
    const names = slots.value.map(slot => slot.file.name);
    const results = await Promise.allSettled(audios.map(audio => audio.play()));
    const failures = results.flatMap((result, index) => result.status === "rejected"
      ? [names[index] + " : " + mediaError(audios[index], result.reason)] : []);
    if (failures.length) throw new Error(failures.join(" · "));
  } catch (error) { trackError.value = `La lecture n’a pas démarré : ${String(error)}`; pause(); }
}
function releaseAudio(path: string) {
  const audio = audioRefs.get(path);
  if (!audio) return;
  audio.pause();
  audio.removeAttribute("src");
  audio.load();
}
function mediaError(audio: HTMLAudioElement, error?: unknown) {
  const details: Record<number, string> = {
    1: "chargement interrompu",
    2: "fichier d’écoute temporaire inaccessible",
    3: "décodage audio impossible",
    4: "format audio refusé par le lecteur Windows",
  };
  return audio.error ? details[audio.error.code] || audio.error.message : String(error || "lecture impossible");
}
function playbackError(slot: Slot, event: Event) {
  trackError.value = slot.file.name + " : " + mediaError(event.currentTarget as HTMLAudioElement);
  pause();
}
function pause() { for (const audio of audioRefs.values()) audio.pause(); playing.value = false; }
function stop() { pause(); playhead.value = 0; for (const audio of audioRefs.values()) audio.currentTime = 0; }
function updatePlayhead(path: string, event: Event) {
  if (path !== activePath.value) return;
  const audio = event.currentTarget as HTMLAudioElement;
  playhead.value = audio.currentTime;
}
function seek(event: Event) {
  const seconds = Number((event.target as HTMLInputElement).value); playhead.value = seconds;
  for (const audio of audioRefs.values()) if (audio.readyState >= 1) audio.currentTime = Math.min(seconds, audio.duration || seconds);
}
function formatTime(seconds: number) { const safe = Math.max(0, Math.floor(seconds || 0)); return `${Math.floor(safe / 60)}:${String(safe % 60).padStart(2, "0")}`; }
function waveformStyle(slot: Slot) {
  const duration = slot.analysis.waveform.durationSeconds;
  const offset = duration > 0 ? Math.min(100, playhead.value / duration * 100) : 0;
  return { left: `${offset}%` };
}
function gainLabel(slot: Slot) {
  const delta = matched.value ? comparisonTarget.value - slot.analysis.measurements.integratedLufs : 0;
  return `${delta > 0 ? "+" : ""}${delta.toFixed(1)} dB`;
}
function sizeLabel(bytes: number) { return bytes < 1_048_576 ? `${Math.max(1, Math.round(bytes / 1024))} Ko` : `${(bytes / 1_048_576).toFixed(1)} Mo`; }
function toggleMode(equalized: boolean) { matched.value = equalized; updateVolumes(); }
function ended(path: string) { if (path === activePath.value) pause(); }

onMounted(async () => { await onProgress(); await nextTick(); emit("layout"); });
watch([() => projects.value.length, () => slots.value.length, scanBusy], async () => { await nextTick(); emit("layout"); });
onUnmounted(() => {
  unlistenProgress?.();
  if (scanBusy.value) void invoke("cancel_library_scan");
  for (const slot of slots.value) { releaseAudio(slot.file.path); void invoke("revoke_comparison_track", { path: slot.file.path }); }
});
</script>

<template>
  <main class="compare-shell">
    <header class="compare-header">
      <div class="compare-brand"><span class="compare-mark" aria-hidden="true">L</span><div><p class="compare-eyebrow">LYTE · OUTIL LOCAL</p><h1>Comparer les masters</h1></div></div>
      <button class="compare-close" type="button" @click="emit('close')">Retour au mastering</button>
    </header>

    <section class="library-intro">
      <div><h2>Choisis le rendu qui sonne juste.</h2><p>LYTE parcourt uniquement le dossier que tu choisis. Les fichiers restent sur place; aucune piste n’est envoyée.</p></div>
      <div class="library-actions"><button class="compare-primary" type="button" :disabled="scanBusy" @click="selectFolder">{{ root ? 'Choisir un autre dossier' : 'Choisir un dossier' }}</button><button v-if="root" class="compare-secondary" type="button" :disabled="scanBusy" @click="scanFolder()">Actualiser</button></div>
    </section>
    <p v-if="root" class="library-root" :title="root">Dossier local : <strong>{{ root }}</strong></p>
    <p class="privacy-note">Le scan est local et limité à ce dossier. Il ne déplace, ne renomme ni ne modifie les morceaux. Les versions d’écoute temporaires sont supprimées quand tu retires les pistes.</p>

    <section v-if="scanBusy" class="scan-progress" aria-live="polite">
      <div><strong>Analyse du dossier…</strong><span>{{ progress.scannedEntries.toLocaleString('fr-BE') }} éléments parcourus · {{ progress.audioFiles }} pistes audio repérées</span></div>
      <progress aria-label="Progression du scan du dossier" />
      <button class="compare-secondary" type="button" @click="cancelScan">Annuler le scan</button>
    </section>
    <p v-if="scanError" class="compare-error" role="status">{{ scanError }}</p>
    <details v-if="scanErrors.length" class="scan-errors"><summary>{{ scanErrors.length }} élément(s) illisible(s)</summary><ul><li v-for="error in scanErrors" :key="error">{{ error }}</li></ul></details>

    <section v-if="!scanBusy && files.length" class="library-summary" aria-live="polite">
      <strong>{{ projects.length }} projet(s) détecté(s)</strong><span>{{ files.length }} fichiers audio · {{ unassigned.length }} à classer</span>
    </section>
    <div v-if="!scanBusy && files.length" class="project-layout">
      <nav class="project-list" aria-label="Projets audio">
        <p class="compare-eyebrow">PROJETS</p>
        <button v-for="project in projects" :key="project.id" type="button" class="project-choice" :class="{ active: selectedProjectId === project.id }" :aria-current="selectedProjectId === project.id ? 'true' : undefined" @click="selectedProjectId = project.id">
          <strong>{{ project.title }}</strong><span>{{ project.files.length }} fichier(s)</span>
        </button>
        <p v-if="!projects.length" class="empty-note">Aucun projet détecté par nommage.</p>
        <form class="create-project" @submit.prevent="createProject"><label for="new-project">Créer un projet</label><div><input id="new-project" v-model="newProjectName" maxlength="80" placeholder="Nom du morceau" /><button class="compare-secondary" type="submit" :disabled="!newProjectName.trim()">Créer</button></div></form>
      </nav>
      <section class="project-detail" aria-live="polite">
        <template v-if="selectedProjectId">
          <div class="project-heading"><div><p class="compare-eyebrow">PROJET SÉLECTIONNÉ</p><h2>{{ projectLabel(selectedProjectId) }}</h2></div><div class="project-heading-actions"><span>{{ projectFiles.length }} fichier(s)</span><button class="compare-secondary unlink-project" type="button" @click="requestUnlinkProject">Retirer de la bibliothèque</button></div></div>
          <div v-if="unlinkConfirmationId === selectedProjectId" class="unlink-confirmation" role="status"><p>Le projet sera délié dans LYTE et ses fichiers resteront sur le disque, dans « Fichiers à classer ».</p><div><button class="compare-secondary" type="button" @click="unlinkConfirmationId = ''">Annuler</button><button class="unlink-confirm" type="button" @click="unlinkProject">Retirer le projet</button></div></div>
          <p class="compare-instruction">Coche jusqu’à quatre sources ou rendus pour les écouter. Les exports LYTE présents sur disque restent à leur emplacement.</p>
          <div class="file-list">
          <article v-for="file in projectFiles" :key="file.path" class="library-file">
              <div class="file-title"><strong :title="file.name">{{ file.name }}</strong><span>{{ file.kind === 'source' ? 'Source' : file.profile ? `Master ${displayProfile(file.profile)}` : 'Audio' }} · {{ file.extension }} · {{ sizeLabel(file.bytes) }} · Modifié le {{ formatModified(file.modifiedAtMs) }}</span></div>
              <label class="assign-control"><span class="sr-only">Associer {{ file.name }} à un projet</span><select :value="projectFor(file)" @change="assign(file, $event)"><option value="">Non classé</option><option v-for="project in projects" :key="project.id" :value="project.id">{{ project.title }}</option></select></label>
              <button v-if="!slots.some(slot => slot.file.path === file.path)" class="compare-secondary add-file" type="button" :disabled="slots.length >= 4 || analyzing.includes(file.path)" @click="addTrack(file)">{{ analyzing.includes(file.path) ? 'Mesure…' : 'Ajouter à l’écoute' }}</button>
              <button v-else class="compare-secondary add-file" type="button" @click="removeTrack(file.path)">Retirer</button>
            </article>
            <p v-if="!projectFiles.length" class="empty-note">Ce projet est prêt. Choisis un fichier non classé pour l’y associer.</p>
          </div>
        </template>
        <p v-else class="empty-note">Sélectionne un projet ou crée-en un pour classer tes fichiers.</p>
        <details v-if="unassigned.length" class="unassigned-list"><summary>Fichiers à classer ({{ unassigned.length }})</summary><div v-for="file in unassigned" :key="file.path" class="unassigned-row"><span :title="file.relativePath"><strong>{{ file.relativePath }} · {{ file.extension }}</strong><small>Modifié le {{ formatModified(file.modifiedAtMs) }}</small></span><label><span class="sr-only">Projet de {{ file.name }}</span><select :value="projectFor(file)" @change="assign(file, $event)"><option value="">Choisir un projet…</option><option v-for="project in projects" :key="project.id" :value="project.id">{{ project.title }}</option></select></label></div></details>
      </section>
    </div>
    <p v-if="!scanBusy && root && !files.length && !scanError" class="empty-note">Aucun WAV, FLAC, MP3 ou M4A trouvé dans ce dossier. Choisis ou actualise un dossier qui contient tes sources et tes exports.</p>

    <section v-if="slots.length" class="comparison-panel" aria-labelledby="comparison-title">
      <div class="comparison-heading"><div><p class="compare-eyebrow">ÉCOUTE COMPARATIVE</p><h2 id="comparison-title">{{ slots.length }} / 4 pistes chargées</h2></div><span class="local-badge">LOCAL</span></div>
      <div class="volume-modes" role="group" aria-label="Mode de niveau d’écoute">
        <button type="button" :aria-pressed="matched" :class="{ active: matched }" @click="toggleMode(true)">Volume égalisé <small>comparer le rendu</small></button>
        <button type="button" :aria-pressed="!matched" :class="{ active: !matched }" @click="toggleMode(false)">Niveau réel <small>écouter comme livré</small></button>
      </div>
      <p class="match-note">{{ matched ? 'Niveau d’écoute calé sur la piste la plus calme (aucun master n’est amplifié).' : 'Gain original, sans égalisation de loudness.' }} Le volume d’écoute reste réglable sur le PC.</p>
      <div class="deck-list" role="radiogroup" aria-label="Piste entendue">
        <article v-for="slot in slots" :key="slot.file.path" class="deck" :class="{ selected: slot.file.path === activePath }">
          <div class="deck-heading"><label class="deck-radio"><input type="radio" name="active-master" :value="slot.file.path" :checked="slot.file.path === activePath" @change="selectDeck(slot.file.path)" /><span><strong>{{ slot.file.name }}</strong><small>{{ slot.file.kind === 'source' ? 'Source' : slot.file.profile ? `Master ${displayProfile(slot.file.profile)}` : 'Rendu' }}</small></span></label><button class="remove-deck" type="button" :aria-label="`Retirer ${slot.file.name} de l’écoute`" @click="removeTrack(slot.file.path)">×</button></div>
          <div class="waveform-row"><div class="waveform" :aria-label="`Forme d’onde de ${slot.file.name}`"><svg viewBox="0 0 240 48" preserveAspectRatio="none" aria-hidden="true"><rect v-for="(peak, index) in slot.analysis.waveform.peaks" :key="index" :x="index" :y="24 - Math.max(1, peak * 22)" width="0.72" :height="Math.max(2, peak * 44)" rx="0.25" /></svg><span class="waveform-cursor" :style="waveformStyle(slot)" /></div><span class="track-duration">{{ formatTime(slot.analysis.waveform.durationSeconds) }}</span></div>
          <div class="deck-measures"><span>LUFS intégré <strong>{{ slot.analysis.measurements.integratedLufs.toFixed(1) }}</strong></span><span>True peak <strong>{{ slot.analysis.measurements.truePeakDbtp.toFixed(1) }} dBTP</strong></span><span>Facteur de crête <strong>{{ slot.analysis.measurements.peakFactorDb.toFixed(1) }} dB</strong></span><span>Grave relatif · 30–150 Hz <strong>{{ slot.analysis.measurements.bassRatioDb.toFixed(1) }} dB</strong></span><span class="aac-measure" :class="{ unsafe: slot.analysis.measurements.aacTruePeakDbtp != null && slot.analysis.measurements.aacTruePeakDbtp > 0 }">AAC · 256 kb/s <strong>{{ slot.analysis.measurements.aacTruePeakDbtp == null ? '—' : `${slot.analysis.measurements.aacTruePeakDbtp.toFixed(1)} dBTP` }}</strong></span><span v-if="matched">Gain d’écoute <strong>{{ gainLabel(slot) }}</strong></span></div>
          <audio :ref="element => setAudioRef(slot.file.path, element)" :src="convertFileSrc(slot.analysis.playbackPath)" preload="auto" @timeupdate="updatePlayhead(slot.file.path, $event)" @ended="ended(slot.file.path)" @error="playbackError(slot, $event)" />
        </article>
      </div>
      <div class="transport"><div class="transport-buttons"><button type="button" class="compare-primary" :disabled="playing" @click="play">Lecture</button><button type="button" class="compare-secondary" :disabled="!playing" @click="pause">Pause</button><button type="button" class="compare-secondary" @click="stop">Arrêter</button></div><div class="timeline"><span>{{ formatTime(playhead) }}</span><label class="sr-only" for="compare-seek">Position d’écoute</label><input id="compare-seek" type="range" min="0" :max="Math.max(1, duration)" step="0.05" :value="Math.min(playhead, duration)" @input="seek" /><span>{{ formatTime(duration) }}</span></div></div>
      <p class="measurement-note">Ces mesures décrivent des dimensions distinctes, pas une note globale. Le contrôle AAC est une simulation FFmpeg AAC à 256 kb/s, spécifique à cet encodeur; il ne certifie pas tous les services de diffusion.</p>
      <p v-if="trackError" class="compare-error" role="alert">{{ trackError }}</p>
      <p class="sr-only" aria-live="polite">{{ announce }}</p>
    </section>
    <footer class="compare-footer"><span>LYTE · AUDIO LOCAL</span><span>Choix final : le tien.</span></footer>
  </main>
</template>

<style scoped>
.compare-shell { width: min(1800px, 100%); max-width: 1800px; margin: 0 auto; padding: 28px 34px 24px; color: #eceef2; }
.compare-header, .compare-brand, .library-intro, .library-actions, .library-summary, .project-heading, .comparison-heading, .deck-heading, .waveform-row, .transport, .transport-buttons { display: flex; align-items: center; justify-content: space-between; gap: 14px; }
.compare-brand { justify-content: flex-start; gap: 12px; }
.compare-mark { display: grid; width: 36px; height: 36px; place-items: center; border: 1px solid #8fbfab; border-radius: 5px; color: #b3dfcd; font-size: 25px; }
.compare-eyebrow { margin: 0 0 5px; color: #9eabb6; font-size: 10px; letter-spacing: .12em; }
h1 { margin: 0; font-size: 21px; }
h2 { margin: 0; font-size: 19px; }
.compare-close, .compare-primary, .compare-secondary { min-height: 38px; border: 1px solid #46515e; border-radius: 5px; padding: 8px 12px; background: #171c23; color: #d8e0e7; cursor: pointer; font-weight: 650; }
.compare-close:hover, .compare-secondary:hover { border-color: #91c8b4; }
.compare-primary { border-color: #a4d7c7; background: #20312e; color: #e1f2eb; }
.compare-primary:hover { background: #29423c; }
button:disabled { opacity: .48; cursor: not-allowed; }
.library-intro { margin: 30px 0 10px; align-items: flex-end; }
.library-intro p, .privacy-note, .compare-instruction, .match-note, .measurement-note { color: #adb7c1; font-size: 13px; line-height: 1.5; }
.library-intro p { max-width: 610px; margin: 8px 0 0; }
.library-actions { flex-shrink: 0; }
.library-root { overflow: hidden; margin: 14px 0 0; color: #a9b3be; font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
.library-root strong { color: #d8e0e7; font-weight: 550; }
.privacy-note { margin: 8px 0 18px; font-size: 11px; }
.scan-progress { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 12px; padding: 16px; border: 1px solid #35423f; border-radius: 7px; background: #141a1d; }
.scan-progress > div { display: grid; gap: 4px; color: #d8e0e7; font-size: 13px; }
.scan-progress span { color: #aab5bf; font-size: 11px; }
.scan-progress progress { grid-column: 1 / -1; width: 100%; accent-color: #a4d7c7; }
.library-summary { justify-content: flex-start; margin: 16px 0; color: #d7e5de; font-size: 13px; }
.library-summary span { color: #aab5bf; font-size: 12px; }
.project-layout { display: grid; grid-template-columns: minmax(240px, .55fr) minmax(0, 2.45fr); gap: 16px; }
.project-list, .project-detail, .comparison-panel { min-width: 0; border: 1px solid #2d3540; border-radius: 7px; background: #14181e; }
.project-list { padding: 14px; }
.project-list > .compare-eyebrow { margin-bottom: 12px; }
.project-choice { display: grid; width: 100%; gap: 4px; margin: 4px 0; border: 1px solid transparent; border-radius: 5px; padding: 10px; background: transparent; color: #d8e0e7; text-align: left; cursor: pointer; }
.project-choice:hover, .project-choice.active { border-color: #40534e; background: #1c2525; }
.project-choice span { color: #9eabb6; font-size: 11px; }
.create-project { display: grid; gap: 7px; margin-top: 15px; border-top: 1px solid #2d3540; padding-top: 14px; }
.create-project label { color: #b7c1ca; font-size: 11px; }
.create-project > div { display: flex; gap: 6px; }
input, select { min-width: 0; border: 1px solid #3b4550; border-radius: 4px; padding: 8px; background: #11151a; color: #e7ebef; }
.create-project input { width: 100%; }
.project-detail { padding: 16px; }
.project-heading { align-items: flex-end; }
.project-heading > span { color: #aab5bf; font-size: 11px; }
.project-heading-actions { display: flex; align-items: center; gap: 10px; }.unlink-project { min-height: 32px; padding: 5px 9px; font-size: 11px; }.unlink-confirmation { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: 12px 0; border: 1px solid #6d5940; border-radius: 5px; padding: 10px 12px; background: #241f19; }.unlink-confirmation p { margin: 0; color: #e0d1b7; font-size: 12px; line-height: 1.45; }.unlink-confirmation > div { display: flex; flex-shrink: 0; gap: 8px; }.unlink-confirm { min-height: 34px; border: 1px solid #9b7151; border-radius: 5px; padding: 6px 10px; background: #38291f; color: #f0d7c3; cursor: pointer; font-weight: 650; }
.compare-instruction { margin: 10px 0 14px; font-size: 12px; }
.file-list { display: grid; gap: 8px; }
.library-file { display: grid; grid-template-columns: minmax(0, 1fr) minmax(180px, 250px) auto; align-items: center; gap: 10px; border-top: 1px solid #2b323b; padding: 10px 0 2px; }
.file-title { display: grid; min-width: 0; gap: 4px; }
.file-title strong { min-width: 0; color: #e0e5ea; font-size: 13px; line-height: 1.4; overflow-wrap: anywhere; }
.file-title span { color: #9eabb6; font-size: 11px; line-height: 1.4; }
.assign-control select, .unassigned-row select { width: 100%; font-size: 11px; }
.add-file { white-space: nowrap; font-size: 11px; }
.empty-note { margin: 12px 0; color: #aab5bf; font-size: 12px; line-height: 1.5; }
.unassigned-list, .scan-errors { margin-top: 14px; border-top: 1px solid #303943; padding-top: 11px; color: #c5cdd4; font-size: 12px; }
.unassigned-list summary, .scan-errors summary { cursor: pointer; }
.unassigned-row { display: grid; grid-template-columns: minmax(0, 1fr) minmax(150px, 210px); align-items: center; gap: 12px; margin-top: 8px; color: #aab5bf; font-size: 11px; }
.unassigned-row > span { display: grid; min-width: 0; gap: 3px; overflow-wrap: anywhere; white-space: normal; }.unassigned-row strong { color: #d7dfe6; font-weight: 500; }.unassigned-row small { color: #9eabb6; font-size: 10px; }
.scan-errors ul { max-height: 130px; overflow: auto; color: #9eabb6; }
.comparison-panel { margin-top: 18px; padding: 16px; }
.comparison-heading { margin-bottom: 14px; }
.local-badge { border: 1px solid #425d54; border-radius: 99px; padding: 4px 9px; color: #b7dfcf; font-size: 9px; letter-spacing: .1em; }
.volume-modes { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
.volume-modes button { display: grid; gap: 3px; border: 1px solid #38424d; border-radius: 5px; padding: 10px; background: #171c23; color: #c2ccd4; text-align: left; cursor: pointer; font-weight: 650; }
.volume-modes button.active { border-color: #8fc6b2; background: #1d2b29; color: #e4f0ea; }
.volume-modes small { color: #9eabb6; font-size: 10px; font-weight: 450; }
.match-note { margin: 10px 0 15px; font-size: 11px; }
.deck-list { display: grid; gap: 9px; }
.deck { min-width: 0; border: 1px solid #303943; border-radius: 5px; padding: 11px; background: #11151a; }
.deck.selected { border-color: #80b69f; box-shadow: inset 3px 0 #80b69f; }
.deck-heading { align-items: flex-start; }
.deck-radio { display: flex; min-width: 0; align-items: flex-start; gap: 9px; cursor: pointer; }
.deck-radio input { accent-color: #a4d7c7; }
.deck-radio > span { display: grid; min-width: 0; gap: 4px; }
.deck-radio strong { overflow-wrap: anywhere; color: #e6eaee; font-size: 12px; }
.deck-radio small { color: #aab5bf; font-size: 10px; }
.remove-deck { border: 0; background: transparent; color: #aab5bf; cursor: pointer; font-size: 19px; }
.waveform-row { gap: 10px; margin-top: 9px; }
.waveform { position: relative; height: 38px; flex: 1; overflow: hidden; border-radius: 3px; background: #17201f; }
.waveform svg { width: 100%; height: 100%; fill: #81bca5; }
.waveform-cursor { position: absolute; top: 0; bottom: 0; width: 2px; background: #f2d28c; box-shadow: 0 0 5px #f2d28c88; }
.track-duration { color: #aab5bf; font-size: 10px; font-variant-numeric: tabular-nums; }
.deck-measures { display: flex; flex-wrap: wrap; gap: 8px 17px; margin-top: 9px; color: #9eabb6; font-size: 10px; }
.deck-measures span { display: flex; gap: 5px; }
.deck-measures strong { color: #d5dde3; font-weight: 650; }
.deck-measures .unsafe, .deck-measures .unsafe strong { color: #f1a38b; }
.deck audio { display: none; }
.transport { flex-wrap: wrap; margin-top: 14px; }
.transport-buttons { justify-content: flex-start; }
.timeline { display: grid; grid-template-columns: auto minmax(120px, 1fr) auto; flex: 1; align-items: center; gap: 9px; color: #aab5bf; font-size: 10px; font-variant-numeric: tabular-nums; }
.timeline input { width: 100%; accent-color: #a4d7c7; }
.measurement-note { margin: 12px 0 0; font-size: 10px; }
.compare-error { color: #f1a38b; font-size: 12px; }
.compare-footer { display: flex; justify-content: space-between; margin-top: 20px; border-top: 1px solid #2b313b; padding-top: 14px; color: #9aa5b3; font-size: 10px; }
.sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
@media (max-width: 760px) { .compare-shell { padding: 22px 18px; } .project-layout { grid-template-columns: 1fr; } .project-list { display: grid; grid-template-columns: repeat(auto-fit, minmax(145px, 1fr)); gap: 4px; } .project-list > .compare-eyebrow, .create-project { grid-column: 1 / -1; } .library-file { grid-template-columns: minmax(0, 1fr) auto; } .assign-control { grid-column: 1; } .add-file { grid-column: 2; grid-row: 1 / span 2; } }
@media (max-width: 520px) { .compare-shell { padding: 17px 12px; } .compare-header, .library-intro { align-items: flex-start; } .compare-header { flex-wrap: wrap; } .library-intro { flex-direction: column; } .library-actions { width: 100%; } .library-actions button { flex: 1; } .volume-modes { grid-template-columns: 1fr; } .deck-measures { display: grid; grid-template-columns: 1fr 1fr; } .transport { align-items: stretch; } .timeline { flex-basis: 100%; } .unassigned-row { grid-template-columns: 1fr; gap: 5px; } .project-heading, .project-heading-actions, .unlink-confirmation { align-items: flex-start; flex-direction: column; } }
@media (prefers-reduced-motion: reduce) { *, *::before, *::after { scroll-behavior: auto !important; transition-duration: .01ms !important; } }
</style>
