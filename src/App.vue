<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

type Status = "Idle" | "Ready" | "Processing" | "Succeeded" | "Failed";
type Track = { path: string; name: string; extension: string };
const status = ref<Status>("Idle");
const track = ref<Track | null>(null);
const progress = ref(0);
const error = ref("");
const output = ref("");
const dragging = ref(false);
const connected = ref(false);
const loading = ref(false);
const busy = computed(() => status.value === "Processing");
const settings = reactive({ loudness: -9, intensity: 1, preserveBass: false });
try {
  const saved = JSON.parse(localStorage.getItem("lyte-settings") || "null");
  if (saved) {
    if (
      Number.isFinite(saved.loudness) &&
      saved.loudness >= -20 &&
      saved.loudness <= 0
    )
      settings.loudness = saved.loudness;
    if (
      Number.isFinite(saved.intensity) &&
      saved.intensity >= 0 &&
      saved.intensity <= 1
    )
      settings.intensity = saved.intensity;
    if (typeof saved.preserveBass === "boolean")
      settings.preserveBass = saved.preserveBass;
  }
} catch {
  /* Defaults survive unavailable or corrupted storage. */
}
watch(settings, () => {
  try {
    localStorage.setItem("lyte-settings", JSON.stringify(settings));
  } catch {
    /* Optional persistence. */
  }
});
const cleanup: UnlistenFn[] = [];
function fail(reason: unknown) {
  error.value = String(reason);
  status.value = "Failed";
}
async function load(path: string) {
  if (busy.value || loading.value) return;
  loading.value = true;
  error.value = "";
  output.value = "";
  progress.value = 0;
  try {
    track.value = await invoke<Track>("inspect_track", { path });
    status.value = "Ready";
  } catch (e) {
    track.value = null;
    fail(e);
  } finally {
    loading.value = false;
  }
}
async function choose() {
  try {
    const path = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Audio", extensions: ["wav", "flac", "mp3"] }],
    });
    if (typeof path === "string") await load(path);
  } catch (e) {
    fail(e);
  }
}
async function master() {
  if (!track.value || busy.value || loading.value) return;
  status.value = "Processing";
  progress.value = 0;
  error.value = "";
  output.value = "";
  try {
    output.value = await invoke<string>("start_mastering", {
      options: { input: track.value.path, ...settings },
    });
    progress.value = 100;
    status.value = "Succeeded";
  } catch (e) {
    fail(e);
  }
}
async function openFolder() {
  try {
    await invoke("open_output_folder");
  } catch (e) {
    error.value = String(e);
  }
}
onMounted(async () => {
  try {
    cleanup.push(
      await listen<number>("mastering-progress", (e) => {
        if (busy.value)
          progress.value = Math.max(progress.value, Math.min(99, e.payload));
      }),
    );
    cleanup.push(
      await getCurrentWebview().onDragDropEvent((e) => {
        dragging.value =
          !busy.value &&
          (e.payload.type === "over" || e.payload.type === "enter");
        if (e.payload.type === "drop" && !busy.value && !loading.value) {
          dragging.value = false;
          if (e.payload.paths.length !== 1) {
            fail("Please drop one audio file at a time.");
            return;
          }
          void load(e.payload.paths[0]);
        }
      }),
    );
    connected.value = true;
  } catch {
    error.value =
      "Desktop connection unavailable. Launch LYTE using npm run dev.";
  }
});
onUnmounted(() => cleanup.forEach((fn) => fn()));
</script>

<template>
  <main>
    <header>
      <div class="brand">
        <span class="mark" aria-hidden="true">L</span>
        <h1>LYTE <span>Mastering</span></h1>
      </div>
      <span class="local"><i></i> LOCAL AUDIO</span>
    </header>
    <section class="intro">
      <p class="eyebrow">THE FINAL TOUCH</p>
      <h2>Make your track ready.</h2>
      <p>Local mastering. Powered by PhaseLimiter.</p>
    </section>
    <button
      class="drop"
      :class="{ dragging, loaded: track }"
      :disabled="busy || loading || !connected"
      @click="choose"
    >
      <svg
        viewBox="0 0 24 24"
        width="28"
        height="28"
        fill="none"
        stroke="currentColor"
        stroke-width="1.4"
        aria-hidden="true"
      >
        <path d="M12 16V3m-5 5 5-5 5 5M4 15v5h16v-5" />
      </svg>
      <template v-if="track"
        ><strong class="filename">{{ track.name }}</strong
        ><span>{{ track.extension }} <b>·</b> Click to replace</span></template
      >
      <template v-else
        ><strong>Drop a track here</strong><span>or <u>choose a file</u></span
        ><small>WAV · FLAC · MP3</small></template
      >
    </button>
    <fieldset :disabled="busy">
      <legend class="sr-only">Mastering settings</legend>
      <div class="control">
        <label for="loudness">Target loudness</label
        ><output for="loudness"
          >{{ settings.loudness.toFixed(1) }} <span>dB</span></output
        ><input
          id="loudness"
          v-model.number="settings.loudness"
          type="range"
          min="-20"
          max="0"
          step="0.1"
        />
        <div class="scale"><span>−20 dB</span><span>0 dB</span></div>
      </div>
      <div class="control">
        <label for="intensity">Mastering intensity</label
        ><output for="intensity">{{ settings.intensity.toFixed(2) }}</output
        ><input
          id="intensity"
          v-model.number="settings.intensity"
          type="range"
          min="0"
          max="1"
          step="0.01"
        />
        <div class="scale"><span>Subtle</span><span>Full</span></div>
      </div>
      <label class="bass" for="bass"
        ><span
          >Preserve bass<small>Keep more weight in the low end.</small></span
        ><input
          id="bass"
          v-model="settings.preserveBass"
          type="checkbox"
          role="switch" /><span class="toggle" aria-hidden="true"></span
      ></label>
    </fieldset>
    <button
      class="primary"
      :disabled="!track || busy || loading || !connected"
      @click="master"
    >
      {{ busy ? "MASTERING…" : "MASTER TRACK"
      }}<span aria-hidden="true">↗</span>
    </button>
    <section class="result" aria-live="polite" :aria-busy="busy">
      <template v-if="busy"
        ><div class="result-row">
          <strong>Mastering…</strong><span>{{ Math.floor(progress) }}%</span>
        </div>
        <progress
          :value="progress"
          max="100"
          aria-label="Mastering progress"
        ></progress>
        <p>Working on your track. This may take a few minutes.</p></template
      >
      <template v-else-if="status === 'Succeeded'"
        ><div class="result-row">
          <strong class="success">✓ Master complete</strong><span>WAV</span>
        </div>
        <p class="filename">{{ output.split(/[\\/]/).pop() }}</p>
        <button class="secondary" @click="openFolder">
          Open folder ↗
        </button></template
      >
      <p v-else-if="!error" class="hint">
        {{
          track
            ? "Your master will be saved beside the original."
            : "Choose a track to get started."
        }}
      </p>
      <p v-if="error" role="alert" class="error">{{ error }}</p>
    </section>
    <footer>
      <span>PHASELIMITER ENGINE</span><span>Always on your computer.</span>
    </footer>
  </main>
</template>
