//! Runs mold's shell tests in parallel for Cargo's test harness.
//!
//! The tests themselves deliberately remain shell scripts so that this port
//! exercises exactly the same inputs and toolchains as the system linker.
//! The runner owns test discovery, architecture selection, scheduling,
//! timeouts and reporting. Tests run natively on the host, and on an
//! arm64 host also for x86_64 under Rosetta.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Mode {
    /// The host architecture only.
    Native,
    /// The host architecture, plus x86_64 under Rosetta when available.
    All,
}

struct Options {
    jobs: usize,
    mode: Mode,
    patterns: Vec<String>,
    timeout: Duration,
    list: bool,
}

#[derive(Clone)]
struct TestJob {
    arch: &'static str,
    script: PathBuf,
    name: String,
    log: PathBuf,
    status_file: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Pass,
    Skip,
    Fail,
    Timeout,
}

impl Outcome {
    fn status(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Skip => "skip",
            Self::Fail | Self::Timeout => "fail",
        }
    }
}

struct TestResult {
    arch: &'static str,
    name: String,
    log: PathBuf,
    outcome: Outcome,
}

#[derive(Default)]
struct Counts {
    pass: usize,
    skip: usize,
    fail: usize,
}

impl Counts {
    fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Pass => self.pass += 1,
            Outcome::Skip => self.skip += 1,
            Outcome::Fail | Outcome::Timeout => self.fail += 1,
        }
    }

    fn merge(&mut self, other: &Self) {
        self.pass += other.pass;
        self.skip += other.skip;
        self.fail += other.fail;
    }
}

fn usage() -> ! {
    eprintln!(
        "Usage: cargo test [pattern] [-- [--test-threads N] \
         [--native | --all] [--timeout SECONDS] [--list]]"
    );
    std::process::exit(2);
}

fn parse_usize(value: Option<String>) -> usize {
    value.and_then(|s| s.parse().ok()).filter(|&n| n != 0).unwrap_or_else(|| usage())
}

fn parse_options() -> Options {
    let mut jobs = thread::available_parallelism().map_or(1, usize::from);
    let mut mode = Mode::All;
    let mut patterns = Vec::new();
    let mut timeout = DEFAULT_TIMEOUT;
    let mut list = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-j" | "--jobs" | "--test-threads" => jobs = parse_usize(args.next()),
            "--native" => mode = Mode::Native,
            "--all" => mode = Mode::All,
            "--timeout" => timeout = Duration::from_secs(parse_usize(args.next()) as u64),
            "--list" => list = true,
            "--nocapture" | "--show-output" => {}
            "-h" | "--help" => usage(),
            _ if arg.starts_with("-j") && arg.len() > 2 => {
                jobs = parse_usize(Some(arg[2..].to_owned()))
            }
            _ if arg.starts_with("--test-threads=") => {
                jobs = parse_usize(arg.split_once('=').map(|(_, value)| value.to_owned()))
            }
            _ if arg.starts_with('-') => usage(),
            _ => patterns.push(arg),
        }
    }

    Options { jobs, mode, patterns, timeout, list }
}

fn native_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" }
}

/// Whether x86_64 binaries run on this arm64 host.
fn rosetta_available() -> bool {
    Command::new("arch")
        .args(["-x86_64", "/usr/bin/true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn selected_archs(options: &Options) -> Vec<&'static str> {
    let native = native_arch();
    let mut archs = vec![native];
    if options.mode == Mode::All && native == "arm64" && rosetta_available() {
        archs.push("x86_64");
    }
    archs
}

fn matches_patterns(name: &str, patterns: &[String]) -> bool {
    patterns.is_empty() || patterns.iter().any(|pattern| name.contains(pattern))
}

fn discover_scripts(test_dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut scripts = Vec::new();
    for entry in fs::read_dir(test_dir)? {
        let path = entry?.path();
        if path.extension() != Some(OsStr::new("sh")) {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        scripts.push((name, path));
    }
    scripts.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(scripts)
}

fn clear_results(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if matches!(path.extension().and_then(OsStr::to_str), Some("log" | "status")) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

/// Tests run from a directory beside the linker under test and write
/// their outputs under out/test there, so nothing generated lands in the
/// source tree.
fn prepare_work_dir(mold: &Path) -> io::Result<PathBuf> {
    let mold = mold.canonicalize()?;
    let profile_dir = mold.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no parent directory", mold.display()),
        )
    })?;
    let work_dir = profile_dir.join("mold-test");
    fs::create_dir_all(&work_dir)?;
    Ok(work_dir)
}

fn make_jobs(
    cases_dirs: &[PathBuf],
    work_dir: &Path,
    archs: &[&'static str],
    patterns: &[String],
    clean: bool,
) -> io::Result<Vec<TestJob>> {
    let mut scripts = Vec::new();
    for dir in cases_dirs {
        scripts.extend(discover_scripts(dir)?);
    }
    scripts.sort_by(|a, b| a.0.cmp(&b.0));
    let mut jobs = Vec::new();

    for &arch in archs {
        let result_dir = work_dir.join("out/test/results").join(arch);
        if clean {
            clear_results(&result_dir)?;
        }
        for (name, script) in &scripts {
            if matches_patterns(name, patterns) {
                jobs.push(TestJob {
                    arch,
                    script: script.clone(),
                    name: name.clone(),
                    log: result_dir.join(format!("{name}.log")),
                    status_file: result_dir.join(format!("{name}.status")),
                });
            }
        }
    }
    Ok(jobs)
}

/// A script that skips itself ends its startup line with "skipped".
fn log_says_skipped(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| {
        bytes
            .split(|&byte| byte == b'\n')
            .any(|line| line.strip_suffix(b"\r").unwrap_or(line).ends_with(b"skipped"))
    })
}

fn run_process(
    root: &Path,
    job: &TestJob,
    linker: &Path,
    timeout: Duration,
) -> Result<Outcome, String> {
    let log = File::create(&job.log)
        .map_err(|err| format!("cannot create {}: {err}", job.log.display()))?;
    let stderr =
        log.try_clone().map_err(|err| format!("cannot clone {}: {err}", job.log.display()))?;
    let mut command = Command::new(&job.script);
    command
        .current_dir(root)
        .env("mold", linker)
        .env("ARCH", job.arch)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr));

    // A timeout must also kill compiler children. The test runs in
    // its own process group, which is a background group of the
    // terminal cargo was started from, so it must not inherit that
    // terminal as stdin: a program that reads it or restores its
    // settings on exit (lldb does, even in batch mode) is stopped by
    // SIGTTIN or SIGTTOU and hangs until the timeout.
    command.process_group(0);
    let mut child =
        command.spawn().map_err(|err| format!("cannot run {}: {err}", job.script.display()))?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|err| format!("cannot wait for test: {err}"))? {
            Some(status) => {
                return Ok(if !status.success() {
                    Outcome::Fail
                } else if log_says_skipped(&job.log) {
                    Outcome::Skip
                } else {
                    Outcome::Pass
                });
            }
            None if start.elapsed() < timeout => thread::sleep(Duration::from_millis(20)),
            None => {
                // SAFETY: kill only signals the test's own process group.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.wait();
                return Ok(Outcome::Timeout);
            }
        }
    }
}

fn run_job(root: &Path, job: &TestJob, linker: &Path, timeout: Duration) -> TestResult {
    let mut outcome = run_process(root, job, linker, timeout).unwrap_or_else(|err| {
        eprintln!("{}: {err}", job.name);
        Outcome::Fail
    });

    // Keep failed test directories for diagnosis, but do not retain the
    // successful tests' potentially large temporary files.
    if matches!(outcome, Outcome::Pass | Outcome::Skip) {
        let dir = root.join("out/test").join(job.arch).join(&job.name);
        match fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                eprintln!("{}: cannot remove {}: {err}", job.name, dir.display());
                outcome = Outcome::Fail;
            }
        }
    }

    if let Err(err) = fs::write(&job.status_file, format!("{}\n", outcome.status())) {
        eprintln!("{}: cannot write {}: {err}", job.name, job.status_file.display());
    }
    TestResult { arch: job.arch, name: job.name.clone(), log: job.log.clone(), outcome }
}

fn run_jobs(root: &Path, jobs: Vec<TestJob>, linker: &Path, options: &Options) -> Vec<TestResult> {
    if jobs.is_empty() {
        return Vec::new();
    }
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let workers = options.jobs.min(jobs.len());

    thread::scope(|scope| {
        for _ in 0..workers {
            let jobs = &jobs;
            let next = &next;
            let sender = sender.clone();
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(index) else {
                        break;
                    };
                    if sender.send(run_job(root, job, linker, options.timeout)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        let mut results = Vec::with_capacity(jobs.len());
        while let Ok(result) = receiver.recv() {
            if matches!(result.outcome, Outcome::Fail | Outcome::Timeout) {
                eprintln!(
                    "FAIL {}:{}{} ({})",
                    result.arch,
                    result.name,
                    if result.outcome == Outcome::Timeout { " [timeout]" } else { "" },
                    result.log.display()
                );
            }
            results.push(result);
            if results.len().is_multiple_of(100) || results.len() == jobs.len() {
                eprintln!("completed {}/{}", results.len(), jobs.len());
            }
        }
        results
    })
}

fn print_inventory(jobs: &[TestJob]) {
    let mut counts = BTreeMap::new();
    for job in jobs {
        *counts.entry(job.arch).or_insert(0usize) += 1;
    }
    for (arch, count) in counts {
        println!("{arch}: tests={count}");
    }
    println!("total: tests={}", jobs.len());
}

fn print_summary(results: &[TestResult]) -> bool {
    let mut by_arch: BTreeMap<&str, Counts> = BTreeMap::new();
    for result in results {
        by_arch.entry(result.arch).or_default().add(result.outcome);
    }

    let mut total = Counts::default();
    for (arch, counts) in &by_arch {
        println!("{arch}: pass={} skip={} fail={}", counts.pass, counts.skip, counts.fail);
        total.merge(counts);
    }
    if by_arch.len() > 1 {
        println!("total: pass={} skip={} fail={}", total.pass, total.skip, total.fail);
    } else {
        println!("pass={} skip={} fail={}", total.pass, total.skip, total.fail);
    }
    total.fail == 0
}

/// Discovers and runs the test scripts in `cases_dirs`, using the linker
/// at `mold`. Command line arguments are substring patterns selecting a
/// subset of tests.
pub fn run(cases_dirs: &[PathBuf], mold: &Path) -> ExitCode {
    let options = parse_options();
    let work_dir = prepare_work_dir(mold).unwrap_or_else(|err| {
        eprintln!("mold-macho-tests: {err}");
        std::process::exit(1);
    });
    let linker = mold.canonicalize().expect("linker not found");
    let archs = selected_archs(&options);
    let jobs = make_jobs(cases_dirs, &work_dir, &archs, &options.patterns, !options.list)
        .unwrap_or_else(|err| {
            eprintln!("mold-macho-tests: {err}");
            std::process::exit(1);
        });

    if options.list {
        print_inventory(&jobs);
        return ExitCode::SUCCESS;
    }

    let results = run_jobs(&work_dir, jobs, &linker, &options);
    if print_summary(&results) { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Output;

    fn run_script(body: &str) -> Output {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = env::temp_dir().join(format!(
            "mold-harness-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let common = Path::new(env!("CARGO_MANIFEST_DIR")).join("common.inc");
        let output = Command::new("bash")
            .args(["-c", &format!("source \"$1\"\n{body}"), "harness-test"])
            .arg(common)
            .env("mold", "unused")
            .current_dir(&dir)
            .output()
            .unwrap();
        fs::remove_dir_all(dir).unwrap();
        output
    }

    #[test]
    fn negated_failure_at_exit_is_not_success() {
        let output = run_script("! true");
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains("OK"));
    }

    #[test]
    fn negative_assertion_stops_before_later_commands() {
        let output = run_script("not true\necho reached");
        assert!(!output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.contains("reached"));
        assert!(!stdout.contains("OK"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("unexpectedly succeeded"));
    }

    #[test]
    fn explicit_exit_status_is_preserved() {
        assert_eq!(run_script("exit 7").status.code(), Some(7));
    }

    #[test]
    fn expected_failure_and_success_pass() {
        let output = run_script("not false\ntrue");
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("OK"));
    }

    #[test]
    fn skip_remains_successful() {
        let output = run_script("skip");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("skipped"));
        assert!(!stdout.contains("OK"));
    }

    #[test]
    fn selects_tests_by_substring() {
        assert!(matches_patterns("dead-strip", &[]));
        assert!(matches_patterns("dead-strip", &["strip".to_owned()]));
        assert!(!matches_patterns("hello", &["strip".to_owned()]));
    }
}
