#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

#[derive(Default)]
struct AppState {
    busy: AtomicBool,
    output: Mutex<Option<PathBuf>>,
    session: Mutex<Option<AutoSession>>,
    cancel_requested: AtomicBool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    input: String,
    loudness: f64,
    intensity: f64,
    preserve_bass: bool,
    dynamic_bass: bool,
    soft_clip_db: f64,
}
const TRUE_PEAK_CEILING: f64 = -1.0;
const BASS_CROSSOVER: &str = "acrossover=split=150:order=4th";
const AUTO_TARGET_MIN_LUFS: f64 = -9.0;
const AUTO_TARGET_MAX_LUFS: f64 = -2.0;
const AUTO_TARGET_TOLERANCE_LU: f64 = 0.3;
const AAC_TRUE_PEAK_LIMIT: f64 = 0.0;
const DEFAULT_REFERENCE_FILE: &str = "resources/reference/QUNE - READY TO GO.wav";
const DEFAULT_REFERENCE_START_SECONDS: f64 = 45.0;
const DEFAULT_REFERENCE_DURATION_SECONDS: f64 = 30.0;
static AUTO_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

struct AutoSession {
    id: String,
    export_stem: String,
    export_folder: PathBuf,
    variants: Vec<AutoVariant>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoOptions {
    input: String,
    target_lufs: f64,
    source_segment: Segment,
    reference: Option<ReferenceOptions>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Segment {
    start_seconds: f64,
    duration_seconds: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceOptions {
    input: String,
    segment: Segment,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Measurements {
    integrated_lufs: f64,
    true_peak_dbtp: f64,
    peak_factor_db: f64,
    bass_ratio_db: f64,
    band_energy_db: [f64; 4],
    aac_true_peak_dbtp: Option<f64>,
}
#[derive(Clone, Copy)]
struct AutoProfile {
    id: &'static str,
    label: &'static str,
    intensity: f64,
    preserve_bass: bool,
    dynamic_bass: bool,
    soft_clip_db: f64,
    crest_budget_db: f64,
    bass_budget_db: f64,
}
const AUTO_PROFILES: [AutoProfile; 3] = [
    AutoProfile { id: "faithful", label: "Fidèle", intensity: 0.65, preserve_bass: true, dynamic_bass: false, soft_clip_db: 0.0, crest_budget_db: 2.0, bass_budget_db: 1.5 },
    AutoProfile { id: "dense", label: "Dense", intensity: 0.85, preserve_bass: true, dynamic_bass: true, soft_clip_db: 0.5, crest_budget_db: 4.0, bass_budget_db: 2.5 },
    AutoProfile { id: "aggressive", label: "Agressif", intensity: 1.0, preserve_bass: false, dynamic_bass: true, soft_clip_db: 1.0, crest_budget_db: 6.0, bass_budget_db: 4.0 },
];
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutoVariant {
    id: String,
    profile_id: String,
    profile_label: String,
    target_lufs: f64,
    engine_reference_db: f64,
    achieved_delta_lu: f64,
    attempts: usize,
    path: String,
    measurements: Measurements,
    segment_measurements: Measurements,
    preserve_bass: bool,
    peak_factor_loss_db: f64,
    bass_change_db: f64,
    aac_risk: bool,
    diagnostics: Vec<String>,
    reference_similarity: Option<i32>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AutoResult {
    session_id: String,
    source_path: String,
    source: Measurements,
    source_segment: Measurements,
    target_lufs: f64,
    reference_path: Option<String>,
    reference_segment: Option<Measurements>,
    reference_start_seconds: Option<f64>,
    using_default_reference: bool,
    variants: Vec<AutoVariant>,
    recommended_id: String,
    recommendation: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MasterResult {
    output: String,
    source: Measurements,
    master: Measurements,
}

#[derive(Serialize)]
struct Track {
    path: String,
    name: String,
    extension: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Waveform {
    duration_seconds: f64,
    peaks: Vec<f32>,
}

fn audio_path(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    if !p.is_absolute() || !p.is_file() {
        return Err("The audio file could not be found. Choose it again.".into());
    }
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["wav", "flac", "mp3"].contains(&ext.as_str()) {
        return Err("Choose a WAV, FLAC or MP3 file.".into());
    }
    fs::File::open(&p)
        .map_err(|_| "The audio file cannot be read. Check its permissions.".to_string())?;
    Ok(p)
}
#[tauri::command]
fn inspect_track(path: String) -> Result<Track, String> {
    let p = audio_path(&path)?;
    Ok(Track {
        name: p.file_name().unwrap().to_string_lossy().into(),
        extension: p.extension().unwrap().to_string_lossy().to_uppercase(),
        path,
    })
}
#[tauri::command]
fn inspect_waveform(app: tauri::AppHandle, path: String) -> Result<Waveform, String> {
    const SAMPLE_RATE: usize = 4_000;
    const BINS: usize = 240;
    let input = audio_path(&path)?;
    let input_name = input.to_string_lossy().into_owned();
    let ffmpeg = executable(&root(&app)?, "ffmpeg.exe", true)
        .ok_or("FFmpeg was not found. Place ffmpeg.exe in /bin.")?;
    let output = hidden(&mut Command::new(ffmpeg))
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            input_name.as_str(),
            "-vn",
            "-ac",
            "1",
            "-ar",
            "4000",
            "-f",
            "f32le",
            "-",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "FFmpeg could not prepare the waveform.".to_string())?;
    if !output.status.success() {
        return Err("FFmpeg could not prepare the waveform.".into());
    }
    let sample_count = output.stdout.len() / 4;
    if sample_count == 0 {
        return Err("The audio file contains no samples for the waveform.".into());
    }
    let duration_seconds = sample_count as f64 / SAMPLE_RATE as f64;
    let mut peaks = vec![0.0_f32; BINS];
    for (index, bytes) in output.stdout[..sample_count * 4].chunks_exact(4).enumerate() {
        let sample = f32::from_le_bytes(bytes.try_into().unwrap()).abs();
        let bin = index * BINS / sample_count;
        peaks[bin] = peaks[bin].max(sample);
    }
    let ceiling = peaks.iter().copied().fold(0.0_f32, f32::max).max(0.0001);
    for peak in &mut peaks {
        *peak = (*peak / ceiling).powf(0.55);
    }
    Ok(Waveform {
        duration_seconds,
        peaks,
    })
}
fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf())
    } else {
        app.path()
            .resource_dir()
            .map(|path| dunce::simplified(&path).to_path_buf())
            .map_err(|_| "Application resources cannot be located.".into())
    }
}
fn default_reference(app: &tauri::AppHandle) -> Result<(PathBuf, Segment), String> {
    let path = root(app)?.join(DEFAULT_REFERENCE_FILE);
    if !path.is_file() {
        return Err("The built-in Hard Techno reference is unavailable. Reinstall LYTE Mastering.".into());
    }
    Ok((
        path,
        Segment {
            start_seconds: DEFAULT_REFERENCE_START_SECONDS,
            duration_seconds: DEFAULT_REFERENCE_DURATION_SECONDS,
        },
    ))
}
fn executable(root: &Path, name: &str, search_path: bool) -> Option<PathBuf> {
    let local = root.join("bin").join(name);
    if local.is_file() {
        return Some(local);
    }
    if search_path {
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths).filter(|p| p.is_absolute()) {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}
fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}
fn progression(line: &str) -> Option<f64> {
    let raw = line
        .split_once("progression:")?
        .1
        .split_whitespace()
        .next()?;
    let value: f64 = raw.parse().ok()?;
    value.is_finite().then_some(value.clamp(0.0, 1.0) * 100.0)
}
fn drain(reader: impl Read, app: tauri::AppHandle) {
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&bytes);
                eprint!("{line}");
                if let Some(value) = progression(&line) {
                    let _ = app.emit("mastering-progress", value);
                }
            }
            Err(e) => {
                eprintln!("PhaseLimiter log read failed: {e}");
                break;
            }
        }
    }
}
fn arguments(
    input: &Path,
    output: &Path,
    ffmpeg: &Path,
    cache: &Path,
    options: &Options,
) -> Vec<std::ffi::OsString> {
    let mut args = Vec::new();
    for (flag, value) in [
        ("--input", input),
        ("--output", output),
        ("--ffmpeg", ffmpeg),
        ("--sound_quality2_cache", cache),
    ] {
        args.push(flag.into());
        args.push(value.as_os_str().to_owned());
    }
    for (flag, value) in [
        ("--mastering", "true".to_string()),
        ("--mastering_mode", "mastering5".to_string()),
        ("--mastering_matching_level", options.intensity.to_string()),
        (
            "--mastering_ms_matching_level",
            options.intensity.to_string(),
        ),
        (
            "--mastering5_mastering_level",
            options.intensity.to_string(),
        ),
        (
            "--erb_eval_func_weighting",
            options.preserve_bass.to_string(),
        ),
        ("--reference", options.loudness.to_string()),
    ] {
        args.push(flag.into());
        args.push(value.into());
    }
    args
}
fn output_filename(stem: &str, options: &Options) -> String {
    let bass = if options.preserve_bass { "on" } else { "off" };

    format!(
        "{stem}_mastered_{:.1}dB_i{:.2}_bass-{bass}.wav",
        options.loudness, options.intensity
    )
}
fn ffmpeg_log(ffmpeg: &Path, args: &[String]) -> Result<String, String> {
    let result = hidden(&mut Command::new(ffmpeg))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "FFmpeg could not start.".to_string())?;
    if !result.status.success() {
        return Err("FFmpeg could not analyse this audio file.".into());
    }
    Ok(String::from_utf8_lossy(&result.stderr).into())
}
fn metric(log: &str, name: &str) -> Option<f64> {
    log.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix(name)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}
fn validate_segment(segment: &Segment) -> Result<(), String> {
    if !segment.start_seconds.is_finite() || segment.start_seconds < 0.0 {
        return Err("The passage start must be a positive number of seconds.".into());
    }
    if !segment.duration_seconds.is_finite() || !(15.0..=60.0).contains(&segment.duration_seconds) {
        return Err("Choose a passage between 15 and 60 seconds.".into());
    }
    Ok(())
}
fn measured_input_args(path: &Path, segment: Option<&Segment>) -> Vec<String> {
    let mut args = vec!["-hide_banner".into()];
    if let Some(segment) = segment {
        args.extend(["-ss".into(), format!("{:.3}", segment.start_seconds), "-t".into(), format!("{:.3}", segment.duration_seconds)]);
    }
    args.extend(["-i".into(), path.to_string_lossy().into_owned()]);
    args
}
fn measure_window(app: &tauri::AppHandle, path: &Path, segment: Option<&Segment>) -> Result<Measurements, String> {
    if let Some(segment) = segment { validate_segment(segment)?; }
    let base = root(app)?;
    let ffmpeg = executable(&base, "ffmpeg.exe", true)
        .ok_or("FFmpeg was not found. Place ffmpeg.exe in /bin.")?;
    let mut meter_args = measured_input_args(path, segment);
    meter_args.extend(["-filter:a".into(), "ebur128=peak=true,astats=metadata=0:reset=0".into(), "-f".into(), "null".into(), "-".into()]);
    let log = ffmpeg_log(
        &ffmpeg,
        &meter_args,
    )?;
    if !(log.contains(" mono") || log.contains(" stereo")) {
        return Err("Auto Hard Techno supports mono or stereo files only.".into());
    }
    let integrated = metric(&log, "I:").ok_or("Could not measure integrated loudness.")?;
    let peak = metric(&log, "Peak:")
        .or_else(|| metric(&log, "Peak level dB:"))
        .ok_or("Could not measure true peak.")?;
    let rms = metric(&log, "RMS level dB:").unwrap_or(integrated);
    let volume = |filter: &str| -> Result<f64, String> {
        let mut args = measured_input_args(path, segment);
        args.extend(["-af".into(), filter.into(), "-f".into(), "null".into(), "-".into()]);
        let log = ffmpeg_log(&ffmpeg, &args)?;
        Ok(metric(&log, "mean_volume:").unwrap_or(rms))
    };
    let full_mean = volume("volumedetect")?;
    let bass_mean = volume("highpass=f=30,lowpass=f=150,volumedetect")?;
    let bands = [(30, 150), (150, 500), (500, 4000), (4000, 16000)];
    let mut band_energy_db = [0.0; 4];
    for (index, (low, high)) in bands.iter().enumerate() {
        band_energy_db[index] = volume(&format!("highpass=f={low},lowpass=f={high},volumedetect"))? - full_mean;
    }
    Ok(Measurements {
        integrated_lufs: integrated,
        true_peak_dbtp: peak,
        peak_factor_db: peak - rms,
        bass_ratio_db: bass_mean - full_mean,
        band_energy_db,
        aac_true_peak_dbtp: None,
    })
}
fn measure(app: &tauri::AppHandle, path: &Path) -> Result<Measurements, String> {
    measure_window(app, path, None)
}
fn aac_true_peak(app: &tauri::AppHandle, path: &Path) -> Result<f64, String> {
    let ffmpeg = executable(&root(app)?, "ffmpeg.exe", true)
        .ok_or("FFmpeg was not found. Place ffmpeg.exe in /bin.")?;
    let encoded = tempfile::Builder::new()
        .prefix("lyte-codec-")
        .suffix(".m4a")
        .tempfile()
        .map_err(|_| "Cannot create the temporary AAC verification file.")?
        .into_temp_path();
    let encoded_name = encoded.to_string_lossy().into_owned();
    ffmpeg_log(
        &ffmpeg,
        &[
            "-y".into(),
            "-i".into(),
            path.to_string_lossy().into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "256k".into(),
            encoded_name.clone(),
        ],
    )?;
    let log = ffmpeg_log(
        &ffmpeg,
        &[
            "-hide_banner".into(),
            "-i".into(),
            encoded_name,
            "-filter:a".into(),
            "ebur128=peak=true".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ],
    )?;
    metric(&log, "Peak:").ok_or("Could not measure true peak after AAC encoding.".into())
}
fn measure_export(app: &tauri::AppHandle, path: &Path) -> Result<Measurements, String> {
    let mut measurements = measure(app, path)?;
    measurements.aac_true_peak_dbtp = Some(aac_true_peak(app, path)?);
    Ok(measurements)
}
fn pcm24(app: &tauri::AppHandle, source: &Path, output: &Path, gain: f64) -> Result<(), String> {
    let ffmpeg = executable(&root(app)?, "ffmpeg.exe", true)
        .ok_or("FFmpeg was not found. Place ffmpeg.exe in /bin.")?;
    ffmpeg_log(
        &ffmpeg,
        &[
            "-y".into(),
            "-i".into(),
            source.to_string_lossy().into(),
            "-af".into(),
            format!("volume={gain:.3}dB"),
            "-c:a".into(),
            "pcm_s24le".into(),
            output.to_string_lossy().into(),
        ],
    )
    .map(|_| ())
}
fn pcm24_segment(
    app: &tauri::AppHandle,
    source: &Path,
    output: &Path,
    segment: &Segment,
) -> Result<(), String> {
    validate_segment(segment)?;
    let ffmpeg = executable(&root(app)?, "ffmpeg.exe", true)
        .ok_or("FFmpeg was not found. Place ffmpeg.exe in /bin.")?;
    ffmpeg_log(
        &ffmpeg,
        &[
            "-y".into(),
            "-ss".into(),
            format!("{:.3}", segment.start_seconds),
            "-t".into(),
            format!("{:.3}", segment.duration_seconds),
            "-i".into(),
            source.to_string_lossy().into(),
            "-c:a".into(),
            "pcm_s24le".into(),
            output.to_string_lossy().into(),
        ],
    )
    .map(|_| ())
}
fn finish_master(
    app: &tauri::AppHandle,
    source: &Path,
    output: &Path,
    options: &Options,
) -> Result<(), String> {
    let ffmpeg = executable(&root(app)?, "ffmpeg.exe", true)
        .ok_or("FFmpeg was not found. Place ffmpeg.exe in /bin.")?;
    finish_master_with_ffmpeg(&ffmpeg, source, output, options)
}
fn finish_master_with_ffmpeg(ffmpeg: &Path, source: &Path, output: &Path, options: &Options) -> Result<(), String> {
    if !options.soft_clip_db.is_finite() || !(0.0..=2.0).contains(&options.soft_clip_db) {
        return Err("Soft clip must be between 0.0 and 2.0 dB.".into());
    }
    let mut args = vec!["-y".into(), "-i".into(), source.to_string_lossy().into()];
    if options.dynamic_bass || options.soft_clip_db > 0.0 {
        let compression = if options.dynamic_bass {
            "compand=attacks=0.02:decays=0.25:points=-90/-90|-20/-20|-8/-9.5|0/-4"
        } else { "anull" };
        let mut graph = format!("[0:a]{BASS_CROSSOVER}[lowin][high];[lowin]{compression}[low]");
        if options.soft_clip_db > 0.0 {
            // Shape bands independently here as well as in the final calibration.
            graph.push_str(&format!(
                ";[low]volume={0:.3}dB,asoftclip=type=tanh:param=0.95[clipped_low];\
                 [high]volume={0:.3}dB,asoftclip=type=tanh:param=0.95[clipped_high];\
                 [clipped_low][clipped_high]amix=inputs=2:normalize=0[finished]",
                options.soft_clip_db
            ));
        } else {
            graph.push_str(";[low][high]amix=inputs=2:normalize=0[finished]");
        }
        args.extend(["-filter_complex".into(), graph, "-map".into(), "[finished]".into()]);
    }
    args.extend([
        "-c:a".into(),
        "pcm_f32le".into(),
        output.to_string_lossy().into(),
    ]);
    ffmpeg_log(&ffmpeg, &args).map(|_| ())
}
// Measure and calibrate the actual delivery signal, after tonal/dynamic shaping.
// Every attempt starts from the same floating-point source, never a previous limiter pass.
fn delivery_levels(ffmpeg: &Path, path: &Path) -> Result<(f64, f64), String> {
    delivery_probe(ffmpeg, path).map(|(loudness, peak, _)| (loudness, peak))
}
fn delivery_probe(ffmpeg: &Path, path: &Path) -> Result<(f64, f64, u32), String> {
    let log = ffmpeg_log(ffmpeg, &[
        "-hide_banner".into(), "-nostats".into(), "-i".into(), path.to_string_lossy().into(),
        "-af".into(), "ebur128=peak=true".into(), "-f".into(), "null".into(), "-".into(),
    ])?;
    let loudness = metric(&log, "I:").ok_or("Cannot measure final loudness.")?;
    let peak = metric(&log, "Peak:").ok_or("Cannot measure final true peak.")?;
    if !loudness.is_finite() || !peak.is_finite() || loudness <= -70.0 {
        return Err("The audio has no measurable loudness for calibration.".into());
    }
    let sample_rate = log.lines().filter(|line| line.contains("Audio:")).find_map(|line| {
        line.split(" Hz").next()?.split_whitespace().last()?.parse::<u32>().ok()
    }).ok_or("Cannot read audio sample rate.")?;
    Ok((loudness, peak, sample_rate))
}
// Independent saturation prevents a loud kick from modulating the lead.
// There is deliberately no broadband gain envelope after recombination:
// Each band is bounded to +/-1. Summing with 3 dB of headroom bounds the
// final instantaneous peak shave to 3 dB, at 4x sample rate, without release.
// True-peak safety then uses constant gain over the complete render below.
fn final_level_filter(gain: f64, initial_peak: f64, sample_rate: u32) -> String {
    if initial_peak + gain <= TRUE_PEAK_CEILING {
        return format!("[0:a]volume={gain:.3}dB[finished]");
    }
    format!(
        "[0:a]aresample={},{BASS_CROSSOVER}[low][high];\
         [low]volume={gain:.3}dB,asoftclip=type=tanh[limited_low];\
         [high]volume={gain:.3}dB,asoftclip=type=tanh[limited_high];\
         [limited_low][limited_high]amix=inputs=2:normalize=0,volume=0.70710678,asoftclip=type=hard,aresample={sample_rate}[finished]",
        sample_rate * 4
    )
}
fn calibrate_final_level(
    ffmpeg: &Path, source: &Path, output: &Path, target: f64,
    cancelled: impl Fn() -> bool,
) -> Result<(), String> {
    let work = tempfile::tempdir().map_err(|_| "Cannot create final calibration folder.")?;
    let limited = work.path().join("limited.wav");
    let candidate = work.path().join("candidate.wav");
    let (initial, initial_peak, sample_rate) = delivery_probe(ffmpeg, source)?;
    // Do not chase an unreachable target with ever harder saturation.
    let max_gain = (TRUE_PEAK_CEILING - initial_peak + 12.0).clamp(-24.0, 36.0);
    let mut gain = (target - initial).clamp(-24.0, max_gain);
    let mut best_error = f64::INFINITY;
    for _ in 0..8 {
        if cancelled() { return Err("Mastering was cancelled.".into()); }
        ffmpeg_log(ffmpeg, &[
            "-y".into(), "-i".into(), source.to_string_lossy().into(),
            "-filter_complex".into(), final_level_filter(gain, initial_peak, sample_rate),
            "-map".into(), "[finished]".into(),
            "-c:a".into(), "pcm_f32le".into(), limited.to_string_lossy().into(),
        ])?;
        let (_, peak) = delivery_levels(ffmpeg, &limited)?;
        // EBU output is rounded to 0.1 dB: reserve 0.05 dB for that rounding.
        let trim = (TRUE_PEAK_CEILING - 0.05 - peak).min(0.0);
        ffmpeg_log(ffmpeg, &[
            "-y".into(), "-i".into(), limited.to_string_lossy().into(),
            "-af".into(), format!("volume={trim:.3}dB"),
            "-c:a".into(), "pcm_s24le".into(), candidate.to_string_lossy().into(),
        ])?;
        let (actual, actual_peak) = delivery_levels(ffmpeg, &candidate)?;
        let error = target - actual;
        if actual_peak <= TRUE_PEAK_CEILING && error.abs() < best_error {
            fs::copy(&candidate, output).map_err(|_| "Cannot save calibrated master.")?;
            best_error = error.abs();
        }
        if best_error <= AUTO_TARGET_TOLERANCE_LU { break; }
        let next = (gain + error).clamp(-24.0, max_gain);
        if (next - gain).abs() < 0.05 { break; }
        gain = next;
    }
    if !best_error.is_finite() { return Err("Cannot produce a master within the true-peak ceiling.".into()); }
    Ok(())
}
fn auto_session() -> Result<(String, PathBuf), String> {
    let id = format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        AUTO_SESSION_COUNTER.fetch_add(1, Ordering::SeqCst)
    );
    let folder = std::env::temp_dir().join("lyte-mastering").join(&id);
    fs::create_dir_all(&folder)
        .map_err(|_| "Cannot create the temporary mastering session.".to_string())?;
    Ok((id, folder))
}
fn reference_similarity(master: &Measurements, reference: &Measurements) -> i32 {
    let mut distance = (master.peak_factor_db - reference.peak_factor_db).abs() * 7.0
        + (master.bass_ratio_db - reference.bass_ratio_db).abs() * 7.0;
    for index in 0..4 {
        distance += (master.band_energy_db[index] - reference.band_energy_db[index]).abs() * 3.0;
    }
    (100.0 - distance).clamp(0.0, 100.0).round() as i32
}
fn diagnose_variant(source: &Measurements, variant: &mut AutoVariant, profile: AutoProfile, reference: Option<&Measurements>) {
    variant.peak_factor_loss_db = source.peak_factor_db - variant.measurements.peak_factor_db;
    variant.bass_change_db = variant.measurements.bass_ratio_db - source.bass_ratio_db;
    variant.aac_risk = variant.measurements.aac_true_peak_dbtp.map(|peak| peak > AAC_TRUE_PEAK_LIMIT).unwrap_or(true);
    let mut diagnostics = Vec::new();
    if variant.achieved_delta_lu.abs() > AUTO_TARGET_TOLERANCE_LU {
        diagnostics.push(format!("Target missed by {:+.1} LU after {} attempts. Final drive is bounded to avoid forcing saturation or broadband pumping.", variant.achieved_delta_lu, variant.attempts));
    }
    if variant.peak_factor_loss_db > profile.crest_budget_db {
        diagnostics.push(format!("Transient/impact change is {:.1} dB ({} profile budget: {:.1} dB).", variant.peak_factor_loss_db, profile.label, profile.crest_budget_db));
    }
    if variant.bass_change_db.abs() > profile.bass_budget_db {
        diagnostics.push(format!("Sustained 30–150 Hz energy moved {:+.1} dB ({} profile budget: ±{:.1} dB).", variant.bass_change_db, profile.label, profile.bass_budget_db));
    }
    if variant.aac_risk {
        diagnostics.push("AAC simulation reaches above 0 dBTP; use caution for lossy delivery.".into());
    }
    if diagnostics.is_empty() {
        diagnostics.push("Target, low-end movement and impact remain within this profile’s stated trade-offs.".into());
    }
    variant.reference_similarity = reference.map(|reference| reference_similarity(&variant.segment_measurements, reference));
    variant.diagnostics = diagnostics;
}
fn render_profile_attempt(
    app: &tauri::AppHandle,
    source: &Path,
    folder: &Path,
    profile: AutoProfile,
    target_lufs: f64,
    engine_reference_db: f64,
    attempt: usize,
    segment: &Segment,
) -> Result<AutoVariant, String> {
    if app
        .state::<AppState>()
        .cancel_requested
        .load(Ordering::SeqCst)
    {
        return Err("Mastering was cancelled.".into());
    }
    let raw_path = PathBuf::from(run_mastering(
        app,
        Options {
            input: source.to_string_lossy().into(),
            loudness: engine_reference_db,
            intensity: profile.intensity,
            preserve_bass: profile.preserve_bass,
            dynamic_bass: false,
            soft_clip_db: 0.0,
        },
    )?);
    let shaped_path = folder.join(format!("{}-{attempt}-shaped.wav", profile.id));
    let options = Options {
        input: source.to_string_lossy().into(),
        loudness: engine_reference_db,
        intensity: profile.intensity,
        preserve_bass: profile.preserve_bass,
        dynamic_bass: profile.dynamic_bass,
        soft_clip_db: profile.soft_clip_db,
    };
    finish_master(app, &raw_path, &shaped_path, &options)?;
    let final_path = folder.join(format!("{}-{attempt}.wav", profile.id));
    let ffmpeg = executable(&root(app)?, "ffmpeg.exe", true)
        .ok_or("FFmpeg was not found. Place ffmpeg.exe in /bin.")?;
    calibrate_final_level(&ffmpeg, &shaped_path, &final_path, target_lufs, || {
        app.state::<AppState>().cancel_requested.load(Ordering::SeqCst)
    })?;
    let measurements = measure_export(app, &final_path)?;
    let segment_measurements = measure_window(app, &final_path, Some(segment))?;
    let _ = fs::remove_file(raw_path);
    let _ = fs::remove_file(shaped_path);
    Ok(AutoVariant {
        id: profile.id.into(),
        profile_id: profile.id.into(),
        profile_label: profile.label.into(),
        target_lufs,
        engine_reference_db,
        achieved_delta_lu: measurements.integrated_lufs - target_lufs,
        attempts: attempt,
        path: final_path.to_string_lossy().into(),
        measurements,
        segment_measurements,
        preserve_bass: profile.preserve_bass,
        peak_factor_loss_db: 0.0,
        bass_change_db: 0.0,
        aac_risk: false,
        diagnostics: Vec::new(),
        reference_similarity: None,
    })
}
fn calibrate_profile(app: &tauri::AppHandle, source: &Path, folder: &Path, profile: AutoProfile, target_lufs: f64, segment: &Segment) -> Result<AutoVariant, String> {
    // The final stage already performs bounded calibration. Re-driving the
    // upstream dynamics to chase a missed target would bypass that protection.
    render_profile_attempt(app, source, folder, profile, target_lufs, target_lufs, 1, segment)
}
fn run_mastering(app: &tauri::AppHandle, options: Options) -> Result<String, String> {
    let input = audio_path(&options.input)?;
    if !options.loudness.is_finite()
        || !(-20.0..=0.0).contains(&options.loudness)
        || !options.intensity.is_finite()
        || !(0.0..=1.0).contains(&options.intensity)
    {
        return Err("The mastering settings are outside the allowed range.".into());
    }
    let base = root(app)?;
    let engine = executable(&base, "phase_limiter.exe", false)
        .ok_or("PhaseLimiter was not found. Place phase_limiter.exe and its DLLs in /bin.")?;
    let ffmpeg = executable(&base, "ffmpeg.exe", true)
        .ok_or("FFmpeg was not found. Place ffmpeg.exe in /bin.")?;
    let cache = base.join("resources/phaselimiter/sound_quality2_cache");
    if !cache.is_file() || fs::metadata(&cache).map(|m| m.len() == 0).unwrap_or(true) {
        return Err("PhaseLimiter's sound_quality2_cache is missing. Place the matching cache file in /resources/phaselimiter.".into());
    }
    let parent = input
        .parent()
        .ok_or("The input folder cannot be located.")?;
    let stem = input.file_stem().unwrap().to_string_lossy();
    let output = parent.join(output_filename(&stem, &options));
    if output.exists() {
        return Err(
            "A master with this name already exists. Rename or move it before mastering again."
                .into(),
        );
    }
    // Keep partial files out of the final destination; persist_noclobber protects existing masters.
    let mut final_file = tempfile::NamedTempFile::new_in(parent).map_err(|_| {
        "Cannot write beside the original. Move the track to a writable folder.".to_string()
    })?;
    let work = tempfile::Builder::new()
        .prefix("lyte-")
        .tempdir()
        .map_err(|_| "Cannot create the mastering temporary folder.".to_string())?;
    let stage_input = work.path().join(format!(
        "input.{}",
        input.extension().unwrap().to_string_lossy()
    ));
    fs::copy(&input, &stage_input)
        .map_err(|_| "Cannot prepare the audio file. Check available disk space.".to_string())?;
    let stage_output = work.path().join("master.wav");
    fs::create_dir(work.path().join("tmp"))
        .map_err(|_| "Cannot create the audio processing folder.".to_string())?;
    // Upstream concatenates --ffmpeg into std::system without quoting the executable.
    // Pass a fixed basename and prepend the verified directory to this child's PATH.
    // This supports installation paths containing spaces without modifying the DSP.
    let mut search_paths = vec![ffmpeg
        .parent()
        .ok_or("FFmpeg folder cannot be located.")?
        .to_path_buf()];
    if let Some(paths) = std::env::var_os("PATH") {
        search_paths.extend(std::env::split_paths(&paths));
    }
    let child_path = std::env::join_paths(search_paths)
        .map_err(|_| "The FFmpeg search path is invalid.".to_string())?;
    let mut cmd = Command::new(engine);
    cmd.env("PATH", child_path);
    cmd.args(arguments(
        &stage_input,
        &stage_output,
        Path::new("ffmpeg.exe"),
        &cache,
        &options,
    ))
    .current_dir(work.path())
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = hidden(&mut cmd).spawn().map_err(|e| {
        eprintln!("PhaseLimiter launch failed: {e}");
        "PhaseLimiter could not start. Check its Windows x64 DLLs and the Microsoft Visual C++ runtime.".to_string()
    })?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let a = app.clone();
    let b = app.clone();
    let out_thread = thread::spawn(move || drain(stdout, a));
    let err_thread = thread::spawn(move || drain(stderr, b));
    let result = loop {
        if app.state::<AppState>().cancel_requested.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            break Err("Mastering was cancelled.".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(_) => break Err("Could not wait for PhaseLimiter to finish.".to_string()),
        }
    };
    let _ = out_thread.join();
    let _ = err_thread.join();
    let result = result?;
    if !result.success() {
        eprintln!("PhaseLimiter exit: {result}");
        return Err("Mastering failed. Check the audio file, the matching PhaseLimiter cache, its DLLs and available memory. Technical details are in the development console.".into());
    }
    let stage_finished = work.path().join("finished.wav");
    finish_master(app, &stage_output, &stage_finished, &options)?;
    let finished_measurements = measure(app, &stage_finished)?;
    let stage_capped = work.path().join("capped.wav");
    pcm24(
        app,
        &stage_finished,
        &stage_capped,
        (TRUE_PEAK_CEILING - finished_measurements.true_peak_dbtp).min(0.0),
    )?;
    let mut audio = fs::File::open(&stage_capped)
        .map_err(|_| "The finishing stage did not produce a WAV file.".to_string())?;
    let mut header = [0u8; 12];
    audio
        .read_exact(&mut header)
        .map_err(|_| "PhaseLimiter produced an incomplete file.".to_string())?;
    if &header[0..4] != b"RIFF"
        || &header[8..12] != b"WAVE"
        || audio.metadata().map(|m| m.len() <= 44).unwrap_or(true)
    {
        return Err("PhaseLimiter did not produce a valid WAV file.".into());
    }
    final_file
        .write_all(&header)
        .and_then(|_| std::io::copy(&mut audio, &mut final_file).map(|_| ()))
        .and_then(|_| final_file.as_file().sync_all())
        .map_err(|_| {
            "Cannot save the master. Check free disk space and folder permissions.".to_string()
        })?;
    final_file.persist_noclobber(&output).map_err(|_| {
        "Cannot save the master. An output may already exist or the folder is not writable."
            .to_string()
    })?;
    *app.state::<AppState>()
        .output
        .lock()
        .map_err(|_| "Cannot record the output folder.")? = Some(output.clone());
    Ok(output.to_string_lossy().into())
}
#[tauri::command]
async fn start_auto_mastering(
    app: tauri::AppHandle,
    options: AutoOptions,
) -> Result<AutoResult, String> {
    if !options.target_lufs.is_finite() || !(AUTO_TARGET_MIN_LUFS..=AUTO_TARGET_MAX_LUFS).contains(&options.target_lufs) {
        return Err("Choose an Auto target between −9 and −2 LUFS.".into());
    }
    validate_segment(&options.source_segment)?;
    if let Some(reference) = &options.reference { validate_segment(&reference.segment)?; }
    if app.state::<AppState>().busy.swap(true, Ordering::SeqCst) {
        return Err("A track is already being mastered.".into());
    }
    app.state::<AppState>()
        .cancel_requested
        .store(false, Ordering::SeqCst);
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<AutoResult, String> {
        let input = audio_path(&options.input)?;
        let export_stem = input
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|stem| !stem.is_empty())
            .ok_or("The input filename cannot be used for export.")?;
        let export_folder = input
            .parent()
            .ok_or("The input folder cannot be located.")?
            .to_path_buf();
        let (session_id, folder) = auto_session()?;
        let source = folder.join("source.wav");
        pcm24(&worker, &input, &source, 0.0)?;
        let source_measurements = measure(&worker, &source)?;
        let source_segment = measure_window(&worker, &source, Some(&options.source_segment))?;
        let (reference_input, reference_selection, using_default_reference) = if let Some(reference) = options.reference {
            (audio_path(&reference.input)?, reference.segment, false)
        } else {
            let (input, segment) = default_reference(&worker)?;
            (input, segment, true)
        };
        let local_reference = folder.join("reference.wav");
        pcm24_segment(&worker, &reference_input, &local_reference, &reference_selection)?;
        let reference_measurements = measure(&worker, &local_reference)?;
        let mut variants = Vec::new();
        for profile in AUTO_PROFILES {
            let mut variant = calibrate_profile(&worker, &source, &folder, profile, options.target_lufs, &options.source_segment)?;
            diagnose_variant(&source_measurements, &mut variant, profile, Some(&reference_measurements));
            variants.push(variant);
        }
        let selected = variants
            .iter()
            .max_by(|left, right| {
                left.reference_similarity
                    .unwrap_or(0)
                    .cmp(&right.reference_similarity.unwrap_or(0))
                    .then_with(|| right.achieved_delta_lu.abs().total_cmp(&left.achieved_delta_lu.abs()))
            })
            .ok_or("The Auto profiles did not produce a master.")?;
        let recommended_id = selected.id.clone();
        let similarity = selected.reference_similarity.unwrap_or(0);
        let reference_kind = if using_default_reference { "built-in" } else { "selected" };
        let recommendation = format!("{} is closest to the {reference_kind} reference passage ({similarity}/100 measured similarity) while landing at {:+.1} LU from the requested target.", selected.profile_label, selected.achieved_delta_lu);
        *worker
            .state::<AppState>()
            .session
            .lock()
            .map_err(|_| "Cannot record the mastering session.")? = Some(AutoSession {
            id: session_id.clone(),
            export_stem,
            export_folder,
            variants: variants.clone(),
        });
        Ok(AutoResult {
            session_id,
            source_path: source.to_string_lossy().into(),
            source: source_measurements,
            source_segment,
            target_lufs: options.target_lufs,
            reference_path: Some(local_reference.to_string_lossy().into()),
            reference_segment: Some(reference_measurements),
            reference_start_seconds: Some(reference_selection.start_seconds),
            using_default_reference,
            variants,
            recommended_id,
            recommendation,
        })
    })
    .await
    .map_err(|_| "The mastering process stopped unexpectedly.".to_string())?;
    app.state::<AppState>().busy.store(false, Ordering::SeqCst);
    result
}
#[tauri::command]
fn cancel_mastering(app: tauri::AppHandle) -> Result<(), String> {
    app.state::<AppState>()
        .cancel_requested
        .store(true, Ordering::SeqCst);
    Ok(())
}
#[tauri::command]
fn export_auto_master(
    app: tauri::AppHandle,
    session_id: String,
    variant_id: String,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    let session = state
        .session
        .lock()
        .map_err(|_| "Cannot access the mastering session.")?;
    let session = session
        .as_ref()
        .filter(|s| s.id == session_id)
        .ok_or("This mastering session is no longer available.")?;
    let variant = session
        .variants
        .iter()
        .find(|variant| variant.id == variant_id)
        .ok_or("Unknown master variant.")?;
    let source = PathBuf::from(&variant.path);
    if !source.is_file() {
        return Err("The selected master is no longer available.".into());
    }
    let profile = &variant.profile_id;
    let mut number = 1;
    let mut destination = session.export_folder.join(format!(
        "{}_hard-techno_{}_target-{:.1}LUFS_auto.wav",
        session.export_stem, profile, variant.target_lufs
    ));
    while destination.exists() {
        number += 1;
        destination = session.export_folder.join(format!(
            "{}_hard-techno_{}_target-{:.1}LUFS_auto_{number}.wav",
            session.export_stem, profile, variant.target_lufs
        ));
    }
    fs::copy(source, &destination).map_err(|_| "Cannot export the selected master.".to_string())?;
    *app.state::<AppState>()
        .output
        .lock()
        .map_err(|_| "Cannot record the output folder.")? = Some(destination.clone());
    Ok(destination.to_string_lossy().into())
}
#[tauri::command]
async fn start_mastering(app: tauri::AppHandle, options: Options) -> Result<MasterResult, String> {
    if app.state::<AppState>().busy.swap(true, Ordering::SeqCst) {
        return Err("A track is already being mastered.".into());
    }
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<MasterResult, String> {
        let source = measure(&worker, &audio_path(&options.input)?)?;
        let output = run_mastering(&worker, options)?;
        let master = measure_export(&worker, Path::new(&output))?;
        Ok(MasterResult {
            output,
            source,
            master,
        })
    })
    .await;
    app.state::<AppState>().busy.store(false, Ordering::SeqCst);
    result.map_err(|e| {
        eprintln!("Mastering worker: {e}");
        "The mastering process stopped unexpectedly.".to_string()
    })?
}
#[tauri::command]
fn open_output_folder(app: tauri::AppHandle) -> Result<(), String> {
    let output = app
        .state::<AppState>()
        .output
        .lock()
        .map_err(|_| "Cannot access the output folder.")?
        .clone()
        .ok_or("No completed master is available yet.")?;
    let folder = output
        .parent()
        .filter(|p| p.is_dir())
        .ok_or("The output folder no longer exists.")?;
    let explorer =
        PathBuf::from(std::env::var_os("WINDIR").ok_or("Windows Explorer is unavailable.")?)
            .join("explorer.exe");
    hidden(Command::new(explorer).arg(folder))
        .spawn()
        .map_err(|_| "The output folder could not be opened.".to_string())?;
    Ok(())
}
#[tauri::command]
fn open_help_link(project: String) -> Result<(), String> {
    // The WebView can only request the two upstream project pages shown in help.
    let url = match project.as_str() {
        "bakuage" => "https://github.com/ai-mastering/bakuage",
        "phaselimiter" => "https://github.com/ai-mastering/phaselimiter",
        _ => return Err("This help link is not available.".into()),
    };
    let explorer =
        PathBuf::from(std::env::var_os("WINDIR").ok_or("Windows Explorer is unavailable.")?)
            .join("explorer.exe");
    hidden(Command::new(explorer).arg(url))
        .spawn()
        .map_err(|_| "The help link could not be opened in your default browser.".to_string())?;
    Ok(())
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            inspect_track,
            inspect_waveform,
            start_mastering,
            start_auto_mastering,
            cancel_mastering,
            export_auto_master,
            open_output_folder,
            open_help_link
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<AppState>().busy.load(Ordering::SeqCst) {
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Unable to launch LYTE Mastering");
}
#[cfg(test)]
mod tests {
    use super::*;
    fn test_ffmpeg() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../bin/ffmpeg.exe")
    }
    #[test]
    fn crossover_preserves_energy_at_the_split() {
        let ffmpeg = test_ffmpeg();
        for frequency in [60, 150, 300, 1000] {
            let tone = format!("sine=frequency={frequency}:duration=2:sample_rate=48000");
            let run = |graph: &str| {
                let log = ffmpeg_log(&ffmpeg, &[
                    "-f".into(), "lavfi".into(), "-i".into(), tone.clone(),
                    "-filter_complex".into(), graph.into(), "-f".into(), "null".into(), "-".into(),
                ]).unwrap();
                metric(&log, "mean_volume:").unwrap_or_else(|| {
                    log.lines().find_map(|line| line.split("mean_volume:").nth(1)?.split_whitespace().next()?.parse::<f64>().ok()).unwrap()
                })
            };
            let original = run("[0:a]volumedetect");
            let recombined = run(&format!("[0:a]{BASS_CROSSOVER}[low][high];[low][high]amix=inputs=2:normalize=0,volumedetect"));
            assert!((original - recombined).abs() <= 0.2, "{frequency} Hz: {original} vs {recombined}");
        }
    }
    #[test]
    fn kick_does_not_modulate_a_constant_lead() {
        for amplitude in [0.05, 0.2] {
        let signal = format!("aevalsrc=0.8*sin(2*PI*60*t)*lt(mod(t\\,2)\\,1)+{amplitude}*sin(2*PI*2000*t):s=48000:d=4");
        let result = hidden(&mut Command::new(test_ffmpeg())).args([
            "-v", "error", "-f", "lavfi", "-i",
            &signal,
            "-filter_complex", &final_level_filter(8.0, 0.0, 48000),
            "-map", "[finished]", "-f", "f32le", "-",
        ]).output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        let samples: Vec<f64> = result.stdout.chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()) as f64).collect();
        let lead_level = |start: usize| {
            let window = &samples[start..start + 9600];
            let (mut real, mut imaginary) = (0.0, 0.0);
            for (index, sample) in window.iter().enumerate() {
                let phase = 2.0 * std::f64::consts::PI * 2000.0 * index as f64 / 48000.0;
                real += sample * phase.cos();
                imaginary += sample * phase.sin();
            }
            20.0 * (2.0 * real.hypot(imaginary) / window.len() as f64).log10()
        };
        let jump = lead_level(72000) - lead_level(24000);
        assert!(jump.abs() < 0.2, "Kick removal changed the constant lead by {jump:.2} dB");
        }
    }
    #[test]
    fn final_calibration_reaches_feasible_target_and_bounds_unreachable_target() {
        let ffmpeg = test_ffmpeg();
        let work = tempfile::tempdir().unwrap();
        let source = work.path().join("source.wav");
        let output = work.path().join("output.wav");
        ffmpeg_log(&ffmpeg, &[
            "-f".into(), "lavfi".into(), "-i".into(),
            "aevalsrc=0.2*sin(2*PI*90*t)+0.1*sin(2*PI*900*t):s=48000:d=4".into(),
            "-c:a".into(), "pcm_f32le".into(), source.to_string_lossy().into(),
        ]).unwrap();
        for target in [-9.0, -6.0] {
            calibrate_final_level(&ffmpeg, &source, &output, target, || false).unwrap();
            let (actual, peak) = delivery_levels(&ffmpeg, &output).unwrap();
            assert!((actual-target).abs() <= AUTO_TARGET_TOLERANCE_LU, "target {target}: {actual}");
            assert!(peak <= TRUE_PEAK_CEILING, "{peak}");
        }
        // This bass-heavy fixture cannot reach 0 LUFS within the drive budget.
        // A finite, peak-safe target miss is preferable to unlimited distortion.
        calibrate_final_level(&ffmpeg, &source, &output, 0.0, || false).unwrap();
        let (actual, peak) = delivery_levels(&ffmpeg, &output).unwrap();
        assert!(actual > -9.0 && actual < -AUTO_TARGET_TOLERANCE_LU, "Unreachable target produced {actual}");
        assert!(peak <= TRUE_PEAK_CEILING);
        assert!(calibrate_final_level(&ffmpeg, &source, &output, -5.0, || true).is_err());
    }
    #[test]
    #[ignore = "requires the source/raw fixtures in tests/artifacts/pumping-fix"]
    fn pumping_real_track_regression() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let folder = root.join("tests/artifacts/pumping-fix");
        let ffmpeg = test_ffmpeg();
        let raw = folder.join("raw.wav");
        let capped = folder.join("raw-capped.wav");
        let shaped = folder.join("shaped.wav");
        let output = folder.join("PSYCHEDELICS - corrected.wav");
        let (_, peak) = delivery_levels(&ffmpeg, &raw).unwrap();
        let trim = (TRUE_PEAK_CEILING - peak).min(0.0);
        ffmpeg_log(&ffmpeg, &[
            "-y".into(), "-i".into(), raw.to_string_lossy().into(), "-af".into(),
            format!("volume={trim:.3}dB"), "-c:a".into(), "pcm_s24le".into(), capped.to_string_lossy().into(),
        ]).unwrap();
        let options = Options { input: String::new(), loudness: -4.0, intensity: 0.65, preserve_bass: true, dynamic_bass: false, soft_clip_db: 0.0 };
        finish_master_with_ffmpeg(&ffmpeg, &capped, &shaped, &options).unwrap();
        calibrate_final_level(&ffmpeg, &shaped, &output, -4.0, || false).unwrap();
        let (loudness, peak) = delivery_levels(&ffmpeg, &output).unwrap();
        println!("Corrected PSYCHEDELICS: {loudness} LUFS, {peak} dBTP");
        assert!(peak <= TRUE_PEAK_CEILING);
    }
    #[test]
    #[ignore = "requires LYTE_AUDIO_SOURCE; renders the complete supplied track"]
    fn real_track_regression() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let folder = root.join("tests/artifacts/audio-fix");
        fs::create_dir_all(&folder).unwrap();
        let source = folder.join("input.wav");
        fs::copy(std::env::var("LYTE_AUDIO_SOURCE").unwrap(), &source).unwrap();
        let ffmpeg = dunce::canonicalize(test_ffmpeg()).unwrap();
        let raw = folder.join("raw.wav");
        let shaped = folder.join("shaped.wav");
        let output = folder.join("Cry Me A River - corrected -5 LUFS.wav");
        let options = Options { input: source.to_string_lossy().into(), loudness: -5.0, intensity: 1.0, preserve_bass: false, dynamic_bass: true, soft_clip_db: 1.0 };
        let cache = root.join("resources/phaselimiter/sound_quality2_cache");
        fs::create_dir_all(folder.join("tmp")).unwrap();
        let result = hidden(&mut Command::new(root.join("bin/phase_limiter.exe")))
            .args(arguments(&source, &raw, &ffmpeg, &cache, &options))
            .current_dir(&folder).output().unwrap();
        fs::write(folder.join("engine.log"), &result.stderr).unwrap();
        assert!(result.status.success(), "engine failed");
        // Match run_mastering's intermediate true-peak cap before Auto shaping.
        let capped = folder.join("raw-capped.wav");
        let (_, raw_peak) = delivery_levels(&ffmpeg, &raw).unwrap();
        let trim = (TRUE_PEAK_CEILING - raw_peak).min(0.0);
        ffmpeg_log(&ffmpeg, &[
            "-y".into(), "-i".into(), raw.to_string_lossy().into(), "-af".into(),
            format!("volume={trim:.3}dB"), "-c:a".into(), "pcm_s24le".into(), capped.to_string_lossy().into(),
        ]).unwrap();
        finish_master_with_ffmpeg(&ffmpeg, &capped, &shaped, &options).unwrap();
        calibrate_final_level(&ffmpeg, &shaped, &output, -5.0, || false).unwrap();
        let (actual, peak) = delivery_levels(&ffmpeg, &output).unwrap();
        println!("Corrected full track: {actual} LUFS, {peak} dBTP; {}", output.display());
        assert!(actual.is_finite() && actual <= -5.0 + AUTO_TARGET_TOLERANCE_LU);
        assert!(peak <= TRUE_PEAK_CEILING);
    }
    #[test]
    fn parses_real_progress_and_rejects_noise() {
        assert_eq!(progression("progression: 0.42\r\n"), Some(42.0));
        assert_eq!(progression("progression: 1.2"), Some(100.0));
        assert_eq!(progression("progression: NaN"), None);
        assert_eq!(progression("diagnostic"), None);
    }
    #[test]
    fn includes_mastering_settings_in_output_filename() {
        let options = Options {
            input: "C:\\Audio\\LET THE KICK HIT.wav".to_string(),
            loudness: -7.0,
            intensity: 0.80,
            preserve_bass: false,
            dynamic_bass: false,
            soft_clip_db: 0.0,
        };

        assert_eq!(
            output_filename("LET THE KICK HIT", &options),
            "LET THE KICK HIT_mastered_-7.0dB_i0.80_bass-off.wav"
        );
    }
    #[test]
    fn diagnoses_aac_risk_without_rejecting_an_aggressive_profile() {
        let source = Measurements {
            integrated_lufs: -8.0,
            true_peak_dbtp: -1.0,
            peak_factor_db: 7.0,
            bass_ratio_db: -6.0,
            band_energy_db: [-6.0, -9.0, -12.0, -18.0],
            aac_true_peak_dbtp: None,
        };
        let mut variant = AutoVariant {
            id: "test".into(),
            profile_id: "aggressive".into(),
            profile_label: "Agressif".into(),
            target_lufs: -5.0,
            engine_reference_db: -5.0,
            achieved_delta_lu: 0.0,
            attempts: 1,
            path: "test.wav".into(),
            measurements: Measurements {
                integrated_lufs: -8.0,
                true_peak_dbtp: -1.0,
                peak_factor_db: 7.0,
                bass_ratio_db: -6.0,
                band_energy_db: [-6.0, -9.0, -12.0, -18.0],
                aac_true_peak_dbtp: Some(1.4),
            },
            segment_measurements: source.clone(),
            preserve_bass: false,
            peak_factor_loss_db: 0.0,
            bass_change_db: 0.0,
            aac_risk: false,
            diagnostics: Vec::new(),
            reference_similarity: None,
        };
        diagnose_variant(&source, &mut variant, AUTO_PROFILES[2], None);
        assert!(variant.aac_risk);
        assert!(variant.diagnostics.iter().any(|note| note.contains("AAC simulation")));
    }

    #[test]
    fn reference_similarity_rewards_matching_passages() {
        let reference = Measurements { integrated_lufs: -5.0, true_peak_dbtp: -1.0, peak_factor_db: 5.0, bass_ratio_db: -6.0, band_energy_db: [-6.0, -8.0, -12.0, -18.0], aac_true_peak_dbtp: None };
        let close = reference_similarity(&reference, &reference);
        let loudness_matched = reference_similarity(&Measurements { integrated_lufs: -2.9, ..reference.clone() }, &reference);
        let far = reference_similarity(&Measurements { integrated_lufs: -8.0, true_peak_dbtp: -1.0, peak_factor_db: 9.0, bass_ratio_db: -1.0, band_energy_db: [-1.0, -2.0, -3.0, -4.0], aac_true_peak_dbtp: None }, &reference);
        assert_eq!(close, 100);
        assert_eq!(loudness_matched, 100);
        assert!(far < close);
    }

    #[test]
    fn validates_auto_target_and_passage_bounds() {
        assert!(validate_segment(&Segment { start_seconds: 0.0, duration_seconds: 30.0 }).is_ok());
        assert!(validate_segment(&Segment { start_seconds: -1.0, duration_seconds: 30.0 }).is_err());
        assert!(validate_segment(&Segment { start_seconds: 0.0, duration_seconds: 10.0 }).is_err());
        assert!((AUTO_TARGET_MIN_LUFS..=AUTO_TARGET_MAX_LUFS).contains(&-5.0));
        assert!((AUTO_TARGET_MIN_LUFS..=AUTO_TARGET_MAX_LUFS).contains(&-2.0));
    }

    #[test]
    fn preserves_upstream_mapping() {
        let options = Options {
            input: "track.wav".into(),
            loudness: -8.5,
            intensity: 1.0,
            preserve_bass: true,
            dynamic_bass: false,
            soft_clip_db: 0.0,
        };
        let args = arguments(
            Path::new("input.wav"),
            Path::new("out.wav"),
            Path::new("ffmpeg.exe"),
            Path::new("cache"),
            &options,
        );
        let pairs: Vec<_> = args
            .chunks(2)
            .map(|p| (p[0].to_str().unwrap(), p[1].to_str().unwrap()))
            .collect();
        for pair in [
            ("--reference", "-8.5"),
            ("--mastering_mode", "mastering5"),
            ("--mastering_matching_level", "1"),
            ("--mastering_ms_matching_level", "1"),
            ("--mastering5_mastering_level", "1"),
            ("--erb_eval_func_weighting", "true"),
        ] {
            assert!(pairs.contains(&pair));
        }
    }
}
