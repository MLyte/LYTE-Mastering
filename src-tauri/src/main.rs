#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
};
use tauri::{Emitter, Manager};

#[derive(Default)]
struct AppState {
    busy: AtomicBool,
    output: Mutex<Option<PathBuf>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    input: String,
    loudness: f64,
    intensity: f64,
    preserve_bass: bool,
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
