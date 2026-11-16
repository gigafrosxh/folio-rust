//! Runs code blocks with locally installed interpreters or compilers. Output streams into a shared
//! buffer the UI polls; a run can be cancelled (the whole process group is stopped) and gets a time limit.

use crate::languages::LanguageSpec;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

static SEARCH_PATH: Lazy<Mutex<Option<Vec<String>>>> = Lazy::new(|| Mutex::new(None));

/// Directories to look for executables, like the user's login shell sees them (Finder starts apps with a
/// minimal PATH).
pub fn search_path() -> Vec<String> {
    if let Some(p) = SEARCH_PATH.lock().clone() {
        return p;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs: Vec<String> = vec![];
    if let Some(login) = login_shell_path() {
        dirs.extend(login.split(':').map(|s| s.to_string()));
    }
    dirs.extend(std::env::var("PATH").unwrap_or_default().split(':').map(|s| s.to_string()));
    for d in ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        dirs.push(d.into());
    }
    for d in [".local/bin", ".cargo/bin", "go/bin", ".bun/bin", ".deno/bin", "miniforge3/bin", "miniconda3/bin", "anaconda3/bin", ".pyenv/shims", ".rbenv/shims", ".nvm/current/bin", ".volta/bin"] {
        dirs.push(format!("{home}/{d}"));
    }
    dirs.push("/usr/local/go/bin".into());
    dirs.push("/Library/Frameworks/Python.framework/Versions/Current/bin".into());
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| !d.is_empty() && seen.insert(d.clone()));
    *SEARCH_PATH.lock() = Some(dirs.clone());
    dirs
}

fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let args: Vec<&str> = if shell.ends_with("fish") { vec!["-l", "-c", "string join : $PATH"] } else { vec!["-l", "-c", "printf %s \"$PATH\""] };
    let mut child = Command::new(&shell).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > Duration::from_secs(3) => {
                let _ = child.kill();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
    let mut s = String::new();
    child.stdout?.read_to_string(&mut s).ok()?;
    let s = s.trim().lines().last()?.to_string();
    if s.is_empty() { None } else { Some(s) }
}

pub fn find_executable(name: &str) -> Option<String> {
    let name = if let Some(rest) = name.strip_prefix("~/") { format!("{}/{rest}", std::env::var("HOME").unwrap_or_default()) } else { name.to_string() };
    if name.starts_with('/') {
        return if is_executable(Path::new(&name)) { Some(name) } else { None };
    }
    search_path().into_iter().map(|d| format!("{d}/{name}")).find(|c| is_executable(Path::new(c)))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

pub fn environment() -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    env.insert("PATH".into(), search_path().join(":"));
    env.insert("PYTHONUNBUFFERED".into(), "1".into());
    env.insert("NO_COLOR".into(), "1".into());
    env.entry("LANG".into()).or_insert("de_DE.UTF-8".into());
    env
}

/// The commands that would run `lang`; `override_cmd` is a user command line like `/opt/homebrew/bin/python3.12 {file}`.
pub fn plan(lang: &LanguageSpec, override_cmd: Option<&str>) -> Result<Vec<Vec<String>>, String> {
    if let Some(o) = override_cmd.filter(|o| !o.trim().is_empty()) {
        let mut steps: Vec<Vec<String>> = o.split("&&").map(|s| s.split_whitespace().map(|x| x.to_string()).collect::<Vec<_>>()).filter(|s: &Vec<String>| !s.is_empty()).collect();
        if !o.contains("{file}") {
            if let Some(last) = steps.last_mut() {
                last.push("{file}".into());
            }
        }
        return steps
            .into_iter()
            .map(|step| resolve(&step).ok_or_else(|| format!("Kein Interpreter für {} gefunden (gesucht: {}). In den Einstellungen kann ein eigener Befehl hinterlegt werden.", lang.name, step[0])))
            .collect();
    }
    if !lang.is_runnable() {
        return Err(format!("{} kann nicht ausgeführt werden.", lang.name));
    }
    for p in &lang.run_plans {
        let resolved: Vec<Vec<String>> = p.iter().filter_map(|s| resolve(&s.iter().map(|x| x.to_string()).collect::<Vec<_>>())).collect();
        if resolved.len() == p.len() {
            return Ok(resolved);
        }
    }
    let tried: Vec<&str> = lang.run_plans.iter().filter_map(|p| p.first().and_then(|s| s.first().copied())).collect();
    Err(format!("Kein Interpreter für {} gefunden (gesucht: {}). In den Einstellungen kann ein eigener Befehl hinterlegt werden.", lang.name, tried.join(", ")))
}

fn resolve(cmd: &[String]) -> Option<Vec<String>> {
    let exe = cmd.first()?;
    if exe.starts_with('{') {
        return Some(cmd.to_vec());
    }
    let found = find_executable(exe)?;
    Some(std::iter::once(found).chain(cmd[1..].iter().cloned()).collect())
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    pub output: String,
    pub finished: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub truncated: bool,
    pub duration: f64,
    pub command: String,
    pub error: Option<String>,
    /// A local server the program announced (http://localhost:…), if any.
    pub server_url: Option<String>,
}

impl RunState {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && !self.cancelled
    }
}

pub struct RunOptions {
    pub stdin: Option<String>,
    pub interactive: bool,
    pub working_dir: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub timeout: f64,
    pub max_output: usize,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions { stdin: None, interactive: false, working_dir: None, env: HashMap::new(), timeout: 30.0, max_output: 1_000_000 }
    }
}

/// A running (or finished) code run.
pub struct CodeRun {
    pub state: Arc<Mutex<RunState>>,
    pgid: Arc<Mutex<Option<i32>>>,
    input: Arc<Mutex<Option<std::process::ChildStdin>>>,
}

impl CodeRun {
    pub fn snapshot(&self) -> RunState {
        self.state.lock().clone()
    }

    pub fn cancel(&self) {
        self.state.lock().cancelled = true;
        if let Some(g) = *self.pgid.lock() {
            unsafe {
                libc::killpg(g, libc::SIGTERM);
            }
            let g2 = g;
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(800));
                unsafe {
                    libc::killpg(g2, libc::SIGKILL);
                }
            });
        }
    }

    pub fn send(&self, text: &str) {
        if let Some(i) = self.input.lock().as_mut() {
            let _ = i.write_all(text.as_bytes());
            let _ = i.flush();
        }
    }

    pub fn close_input(&self) {
        self.input.lock().take();
    }

    /// Starts `code` in `lang` in the background.
    pub fn start(code: &str, lang: &LanguageSpec, override_cmd: Option<&str>, opts: RunOptions) -> CodeRun {
        let run = CodeRun { state: Arc::new(Mutex::new(RunState::default())), pgid: Arc::new(Mutex::new(None)), input: Arc::new(Mutex::new(None)) };
        let steps = match plan(lang, override_cmd) {
            Ok(s) => s,
            Err(e) => {
                let mut st = run.state.lock();
                st.error = Some(e.clone());
                st.output = e;
                st.finished = true;
                st.exit_code = Some(-1);
                drop(st);
                return run;
            }
        };
        let dir = std::env::temp_dir().join(format!("folio-run-{}", crate::new_id()));
        let _ = std::fs::create_dir_all(&dir);
        let file_name = if lang.id == "java" {
            // Java runs single files named after their public class.
            let re = regex::Regex::new(r"public\s+class\s+(\w+)").unwrap();
            re.captures(code).map(|c| format!("{}.java", &c[1])).unwrap_or_else(|| "Main.java".into())
        } else {
            format!("main.{}", lang.file_extension)
        };
        let file = dir.join(&file_name);
        let _ = std::fs::write(&file, code);
        let out_bin = dir.join("main.out");
        let state = run.state.clone();
        let pgid = run.pgid.clone();
        let input = run.input.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut env = environment();
            env.extend(opts.env.clone());
            let sub = |s: &str| s.replace("{file}", &file.to_string_lossy()).replace("{out}", &out_bin.to_string_lossy()).replace("{dir}", &dir.to_string_lossy());
            let cwd = opts.working_dir.clone().filter(|d| d.is_dir()).unwrap_or_else(|| dir.clone());
            let deadline = started + Duration::from_secs_f64(opts.timeout.max(1.0));
            for (si, step) in steps.iter().enumerate() {
                let args: Vec<String> = step.iter().map(|s| sub(s)).collect();
                // sqlite3 ".read {file}" is one argument with a space.
                state.lock().command = args.join(" ");
                let mut cmd = Command::new(&args[0]);
                cmd.args(&args[1..]).current_dir(&cwd).env_clear().envs(&env).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::piped());
                unsafe {
                    cmd.pre_exec(|| {
                        libc::setpgid(0, 0);
                        Ok(())
                    });
                }
                let mut child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => {
                        let mut st = state.lock();
                        st.output.push_str(&format!("{}: {e}\n", args[0]));
                        st.exit_code = Some(-1);
                        break;
                    }
                };
                *pgid.lock() = Some(child.id() as i32);
                let last = si + 1 == steps.len();
                if let Some(mut si_pipe) = child.stdin.take() {
                    if last {
                        if let Some(text) = &opts.stdin {
                            let _ = si_pipe.write_all(text.as_bytes());
                        }
                        if opts.interactive {
                            *input.lock() = Some(si_pipe);
                        }
                    }
                }
                let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
                    .into_iter()
                    .flatten()
                    .map(|mut r| {
                        let state = state.clone();
                        let max = opts.max_output;
                        std::thread::spawn(move || {
                            let mut buf = [0u8; 8192];
                            let mut pending: Vec<u8> = vec![];
                            while let Ok(n) = r.read(&mut buf) {
                                if n == 0 {
                                    break;
                                }
                                pending.extend_from_slice(&buf[..n]);
                                // Keep incomplete UTF-8 for the next read.
                                let valid = match std::str::from_utf8(&pending) {
                                    Ok(_) => pending.len(),
                                    Err(e) => e.valid_up_to(),
                                };
                                let text = String::from_utf8_lossy(&pending[..valid]).to_string();
                                pending.drain(..valid);
                                if pending.len() > 4 {
                                    pending.clear();
                                }
                                let mut st = state.lock();
                                if st.output.len() < max {
                                    st.output.push_str(&text);
                                    if st.server_url.is_none() {
                                        st.server_url = detect_server(&text);
                                    }
                                } else {
                                    st.truncated = true;
                                }
                            }
                        })
                    })
                    .collect();
                let status = loop {
                    match child.try_wait() {
                        Ok(Some(s)) => break Some(s),
                        Ok(None) => {
                            if Instant::now() > deadline && state.lock().server_url.is_none() {
                                state.lock().timed_out = true;
                                unsafe {
                                    libc::killpg(child.id() as i32, libc::SIGKILL);
                                }
                            }
                            if state.lock().cancelled {
                                unsafe {
                                    libc::killpg(child.id() as i32, libc::SIGTERM);
                                }
                            }
                            std::thread::sleep(Duration::from_millis(15));
                        }
                        Err(_) => break None,
                    }
                };
                for r in readers {
                    let _ = r.join();
                }
                input.lock().take();
                let code = status.and_then(|s| s.code()).unwrap_or(-1);
                state.lock().exit_code = Some(code);
                if code != 0 {
                    break;
                }
            }
            let mut st = state.lock();
            st.duration = started.elapsed().as_secs_f64();
            if st.truncated {
                st.output.push_str("\n… (Ausgabe gekürzt)");
            }
            if st.timed_out {
                st.output.push_str(&format!("\nZeitlimit von {} s überschritten – abgebrochen.", opts.timeout as i64));
            }
            st.finished = true;
            drop(st);
            let _ = std::fs::remove_dir_all(&dir);
        });
        run
    }
}

fn detect_server(text: &str) -> Option<String> {
    static RE: Lazy<regex::Regex> = Lazy::new(|| regex::Regex::new(r"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]):\d{2,5}[^\s]*").unwrap());
    RE.find(text).map(|m| m.as_str().replace("0.0.0.0", "localhost"))
}

/// Checks a run's output against a block test (#29): {expectedOutput, expectedExitCode, match}.
pub fn check_test(test: &serde_json::Value, output: &str, exit_code: i32) -> (bool, String) {
    let match_kind = test.get("match").and_then(|m| m.as_str()).unwrap_or("exact");
    if let Some(code) = test.get("expectedExitCode").and_then(|c| c.as_i64()) {
        if code as i32 != exit_code {
            return (false, format!("Exit-Code {exit_code}, erwartet {code}"));
        }
    }
    if let Some(exp) = test.get("expectedOutput").and_then(|e| e.as_str()) {
        let out = output.trim_end();
        let e = exp.trim_end();
        let ok = match match_kind {
            "contains" => out.contains(e),
            "regex" => regex::Regex::new(e).map(|r| r.is_match(out)).unwrap_or(false),
            _ => out == e,
        };
        if !ok {
            return (false, "Ausgabe weicht ab".into());
        }
    }
    (true, "Test bestanden".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_bash() {
        let lang = crate::languages::spec_for("bash").unwrap();
        let run = CodeRun::start("echo hallo; read x; echo \"$x\"", &lang, None, RunOptions { stdin: Some("welt\n".into()), ..Default::default() });
        for _ in 0..500 {
            if run.snapshot().finished {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let s = run.snapshot();
        assert!(s.finished);
        assert_eq!(s.output, "hallo\nwelt\n");
        assert_eq!(s.exit_code, Some(0));
    }

    #[test]
    fn timeout_kills() {
        let lang = crate::languages::spec_for("bash").unwrap();
        let run = CodeRun::start("sleep 5", &lang, None, RunOptions { timeout: 1.0, ..Default::default() });
        std::thread::sleep(Duration::from_millis(1600));
        let s = run.snapshot();
        assert!(s.finished && s.timed_out);
    }
}
