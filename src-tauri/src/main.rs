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
    time::{SystemTime, UNIX_EPOCH},
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
}
const AUTO_TARGETS: [f64; 4] = [-11.0, -9.0, -7.0, -5.0];
const TRUE_PEAK_CEILING: f64 = -1.0;
static AUTO_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

struct AutoSession {
    id: String,
    folder: PathBuf,
    export_folder: PathBuf,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutoOptions {
    input: String,
    intensity: f64,
    preserve_bass: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Measurements {
    integrated_lufs: f64,
    true_peak_dbtp: f64,
    peak_factor_db: f64,
    bass_ratio_db: f64,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutoVariant {
    id: String,
    target_db: f64,
    path: String,
    measurements: Measurements,
    peak_factor_loss_db: f64,
    bass_change_db: f64,
    eligible: bool,
    note: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AutoResult {
    session_id: String,
    source_path: String,
    source: Measurements,
    variants: Vec<AutoVariant>,
    recommended_id: String,
    recommendation: String,
}

#[derive(Serialize)]
struct Track {
    path: String,
    name: String,
    extension: String,
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
fn measure(app: &tauri::AppHandle, path: &Path) -> Result<Measurements, String> {
    let base = root(app)?;
    let ffmpeg = executable(&base, "ffmpeg.exe", true)
        .ok_or("FFmpeg was not found. Place ffmpeg.exe in /bin.")?;
    let file = path.to_string_lossy().into_owned();
    let log = ffmpeg_log(
        &ffmpeg,
        &[
            "-hide_banner".into(),
            "-i".into(),
            file.clone(),
            "-filter:a".into(),
            "ebur128=peak=true,astats=metadata=0:reset=0".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ],
    )?;
    if !(log.contains(" mono") || log.contains(" stereo")) {
        return Err("Auto Hard Techno supports mono or stereo files only.".into());
    }
    let integrated = metric(&log, "I:").ok_or("Could not measure integrated loudness.")?;
    if !integrated.is_finite() || integrated <= -69.0 {
        return Err("The audio appears silent. Choose a non-silent track.".into());
    }
    let peak = metric(&log, "Peak:")
        .or_else(|| metric(&log, "Peak level dB:"))
        .ok_or("Could not measure true peak.")?;
    let rms = metric(&log, "RMS level dB:").unwrap_or(integrated);
    let full = ffmpeg_log(
        &ffmpeg,
        &[
            "-hide_banner".into(),
            "-i".into(),
            file.clone(),
            "-af".into(),
            "volumedetect".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ],
    )?;
    let bass = ffmpeg_log(
        &ffmpeg,
        &[
            "-hide_banner".into(),
            "-i".into(),
            file,
            "-af".into(),
            "highpass=f=30,lowpass=f=150,volumedetect".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ],
    )?;
    let full_mean = metric(&full, "mean_volume:").unwrap_or(rms);
    let bass_mean = metric(&bass, "mean_volume:").unwrap_or(rms);
    Ok(Measurements {
        integrated_lufs: integrated,
        true_peak_dbtp: peak,
        peak_factor_db: peak - rms,
        bass_ratio_db: bass_mean - full_mean,
    })
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
fn auto_recommend(source: &Measurements, variants: &mut [AutoVariant]) -> (String, String) {
    for variant in variants.iter_mut() {
        variant.peak_factor_loss_db = source.peak_factor_db - variant.measurements.peak_factor_db;
        variant.bass_change_db = variant.measurements.bass_ratio_db - source.bass_ratio_db;
        variant.eligible =
            variant.peak_factor_loss_db <= 3.0 && variant.bass_change_db.abs() <= 2.0;
        variant.note = if variant.eligible {
            "Within the initial impact and bass guardrails.".into()
        } else {
            "Outside the initial impact or bass guardrails.".into()
        };
    }
    let eligible: Vec<&AutoVariant> = variants.iter().filter(|v| v.eligible).collect();
    let pool = if eligible.is_empty() {
        vec![variants
            .iter()
            .min_by(|a, b| a.target_db.partial_cmp(&b.target_db).unwrap())
            .unwrap()]
    } else {
        eligible
    };
    let loudest = pool
        .iter()
        .map(|v| v.measurements.integrated_lufs)
        .fold(f64::NEG_INFINITY, f64::max);
    let selected = pool
        .into_iter()
        .filter(|v| loudest - v.measurements.integrated_lufs <= 0.3)
        .min_by(|a, b| a.target_db.partial_cmp(&b.target_db).unwrap())
        .unwrap();
    (
        selected.id.clone(),
        if selected.eligible {
            "Recommended from measured loudness while retaining the initial impact and bass guardrails.".into()
        } else {
            "Compromise to verify by ear: no variant met every initial guardrail.".into()
        },
    )
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
    let result = child.wait();
    let _ = out_thread.join();
    let _ = err_thread.join();
    let result = result.map_err(|_| "Could not wait for PhaseLimiter to finish.".to_string())?;
    if !result.success() {
        eprintln!("PhaseLimiter exit: {result}");
        return Err("Mastering failed. Check the audio file, the matching PhaseLimiter cache, its DLLs and available memory. Technical details are in the development console.".into());
    }
    let mut audio = fs::File::open(&stage_output)
        .map_err(|_| "PhaseLimiter finished without creating a WAV file.".to_string())?;
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
    if app.state::<AppState>().busy.swap(true, Ordering::SeqCst) {
        return Err("A track is already being mastered.".into());
    }
    app.state::<AppState>()
        .cancel_requested
        .store(false, Ordering::SeqCst);
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<AutoResult, String> {
        if !options.intensity.is_finite() || !(0.0..=1.0).contains(&options.intensity) {
            return Err("The mastering intensity is outside the allowed range.".into());
        }
        let input = audio_path(&options.input)?;
        let export_folder = input
            .parent()
            .ok_or("The input folder cannot be located.")?
            .to_path_buf();
        let (session_id, folder) = auto_session()?;
        let source = folder.join("source.wav");
        pcm24(&worker, &input, &source, 0.0)?;
        let source_measurements = measure(&worker, &source)?;
        let mut variants = Vec::new();
        for (index, target) in AUTO_TARGETS.iter().enumerate() {
            if worker
                .state::<AppState>()
                .cancel_requested
                .load(Ordering::SeqCst)
            {
                return Err("Mastering was cancelled.".into());
            }
            let engine_options = Options {
                input: source.to_string_lossy().into(),
                loudness: *target,
                intensity: options.intensity,
                preserve_bass: options.preserve_bass,
            };
            let raw_path = PathBuf::from(run_mastering(&worker, engine_options)?);
            let raw_measurements = measure(&worker, &raw_path)?;
            let final_path = folder.join(format!("hard-techno_{target:.0}dB.wav"));
            pcm24(
                &worker,
                &raw_path,
                &final_path,
                (TRUE_PEAK_CEILING - raw_measurements.true_peak_dbtp).min(0.0),
            )?;
            let measurements = measure(&worker, &final_path)?;
            let _ = fs::remove_file(raw_path);
            variants.push(AutoVariant {
                id: format!("v{index}"),
                target_db: *target,
                path: final_path.to_string_lossy().into(),
                measurements,
                peak_factor_loss_db: 0.0,
                bass_change_db: 0.0,
                eligible: false,
                note: String::new(),
            });
        }
        let (recommended_id, recommendation) = auto_recommend(&source_measurements, &mut variants);
        *worker
            .state::<AppState>()
            .session
            .lock()
            .map_err(|_| "Cannot record the mastering session.")? = Some(AutoSession {
            id: session_id.clone(),
            folder,
            export_folder,
        });
        Ok(AutoResult {
            session_id,
            source_path: source.to_string_lossy().into(),
            source: source_measurements,
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
    let target = match variant_id.as_str() {
        "v0" => -11,
        "v1" => -9,
        "v2" => -7,
        "v3" => -5,
        _ => return Err("Unknown master variant.".into()),
    };
    let source = session.folder.join(format!("hard-techno_{target}dB.wav"));
    if !source.is_file() {
        return Err("The selected master is no longer available.".into());
    }
    let mut number = 1;
    let mut destination = session
        .export_folder
        .join(format!("hard-techno_{target}dB_auto.wav"));
    while destination.exists() {
        number += 1;
        destination = session
            .export_folder
            .join(format!("hard-techno_{target}dB_auto_{number}.wav"));
    }
    fs::copy(source, &destination).map_err(|_| "Cannot export the selected master.".to_string())?;
    *app.state::<AppState>()
        .output
        .lock()
        .map_err(|_| "Cannot record the output folder.")? = Some(destination.clone());
    Ok(destination.to_string_lossy().into())
}
#[tauri::command]
async fn start_mastering(app: tauri::AppHandle, options: Options) -> Result<String, String> {
    if app.state::<AppState>().busy.swap(true, Ordering::SeqCst) {
        return Err("A track is already being mastered.".into());
    }
    let worker = app.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || run_mastering(&worker, options)).await;
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
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            inspect_track,
            start_mastering,
            start_auto_mastering,
            cancel_mastering,
            export_auto_master,
            open_output_folder
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
        };

        assert_eq!(
            output_filename("LET THE KICK HIT", &options),
            "LET THE KICK HIT_mastered_-7.0dB_i0.80_bass-off.wav"
        );
    }
    #[test]
    fn preserves_upstream_mapping() {
        let options = Options {
            input: "track.wav".into(),
            loudness: -8.5,
            intensity: 1.0,
            preserve_bass: true,
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
