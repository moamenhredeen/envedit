//! Elevation check, the elevated helper mode, and launching it.
//!
//! The unelevated UI writes the Machine part of an Apply to an input file
//! and starts this executable elevated with
//! `--apply-machine <input> --result <result>`. The helper validates the
//! whole input, applies it with the shared apply logic and writes one
//! outcome per variable to the result file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject};
use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{HRESULT, HSTRING, PCWSTR, w};

use super::apply::{Outcome, VarResult, apply_scope};
use super::model::{Scope, ValueType, validate_name};
use super::records::{self, Record};
use super::registry::{EnvStore, RegistryStore};
use super::staging::PlannedChange;

pub const APPLY_MACHINE_ARG: &str = "--apply-machine";
pub const RESULT_ARG: &str = "--result";

/// Exit codes of the helper.
pub const EXIT_DONE: i32 = 0;
pub const EXIT_BAD_INPUT: i32 = 2;

/// Whether this process runs with an elevated token.
pub fn is_elevated() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: pseudo handle of the current process, out pointer to a local.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut len = 0u32;
    // SAFETY: buffer is a TOKEN_ELEVATION of the size passed.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
    };
    // SAFETY: token was opened above and is closed once.
    unsafe {
        let _ = CloseHandle(token);
    }
    ok.is_ok() && elevation.TokenIsElevated != 0
}

fn type_token(state: &Option<(String, ValueType)>) -> (String, String) {
    match state {
        Some((value, ty)) => (ty.token(), value.clone()),
        None => ("-".to_string(), String::new()),
    }
}

fn parse_state(ty: &str, value: &str) -> Result<Option<(String, ValueType)>, String> {
    if ty == "-" {
        return if value.is_empty() { Ok(None) } else { Err("value given for absent state".to_string()) };
    }
    match ValueType::from_token(ty) {
        Some(t) if t.is_editable() => Ok(Some((value.to_string(), t))),
        _ => Err(format!("unsupported type {ty}")),
    }
}

/// Encodes Machine changes for the helper.
pub fn encode_input(changes: &[PlannedChange]) -> String {
    let records: Vec<Record> = changes
        .iter()
        .map(|c| {
            let (bt, bv) = type_token(&c.before);
            let (at, av) = type_token(&c.after);
            Record::new("change", [c.scope.token(), &c.name, &bt, &bv, &at, &av])
        })
        .collect();
    records::encode(&records)
}

/// Decodes and validates helper input. Only Machine changes are accepted.
pub fn decode_input(text: &str) -> Result<Vec<PlannedChange>, String> {
    let records = records::decode(text)?;
    if records.is_empty() {
        return Err("no changes in input".to_string());
    }
    records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let n = i + 1;
            let [scope, name, bt, bv, at, av] = r.fields.as_slice() else {
                return Err(format!("record {n}: expected 6 fields"));
            };
            if r.kind != "change" {
                return Err(format!("record {n}: unknown kind {}", r.kind));
            }
            if Scope::from_token(scope) != Some(Scope::Machine) {
                return Err(format!("record {n}: only Machine changes are allowed"));
            }
            validate_name(name).map_err(|e| format!("record {n}: {e}"))?;
            let before = parse_state(bt, bv).map_err(|e| format!("record {n}: {e}"))?;
            let after = parse_state(at, av).map_err(|e| format!("record {n}: {e}"))?;
            if before.is_none() && after.is_none() {
                return Err(format!("record {n}: neither before nor after state"));
            }
            if let Some((value, _)) = &after
                && value.is_empty()
            {
                return Err(format!("record {n}: empty value"));
            }
            Ok(PlannedChange { scope: Scope::Machine, name: name.clone(), before, after })
        })
        .collect()
}

pub fn encode_results(results: &[VarResult]) -> String {
    let records: Vec<Record> = results
        .iter()
        .map(|r| {
            let (kind, message) = match &r.outcome {
                Outcome::Applied => ("ok", ""),
                Outcome::Conflict(m) => ("conflict", m.as_str()),
                Outcome::Failed(m) => ("failed", m.as_str()),
            };
            Record::new(kind, [r.scope.token(), &r.name, message])
        })
        .collect();
    records::encode(&records)
}

pub fn decode_results(text: &str) -> Result<Vec<VarResult>, String> {
    records::decode(text)?
        .into_iter()
        .map(|r| {
            let [scope, name, message] = r.fields.as_slice() else {
                return Err("result record: expected 3 fields".to_string());
            };
            let scope = Scope::from_token(scope).ok_or("result record: bad scope")?;
            let outcome = match r.kind.as_str() {
                "ok" => Outcome::Applied,
                "conflict" => Outcome::Conflict(message.clone()),
                "failed" => Outcome::Failed(message.clone()),
                other => return Err(format!("result record: unknown kind {other}")),
            };
            Ok(VarResult { scope, name: name.clone(), outcome })
        })
        .collect()
}

/// The helper's work without process and file handling: validates `input`
/// completely, then applies it to `store`. The Machine snapshot was taken
/// by the invoking user before the helper started.
pub fn helper_apply(input: &str, store: &mut dyn EnvStore) -> Result<String, String> {
    let changes = decode_input(input)?;
    let results = apply_scope(store, Scope::Machine, &changes, &mut |_| Ok(()));
    Ok(encode_results(&results))
}

/// Entry point of `--apply-machine <input> --result <result>`. Returns the exit code.
pub fn run_helper(args: &[String]) -> i32 {
    let (Some(input), Some(result)) = (arg_value(args, APPLY_MACHINE_ARG), arg_value(args, RESULT_ARG)) else {
        return EXIT_BAD_INPUT;
    };
    let Ok(text) = fs::read_to_string(input) else {
        return EXIT_BAD_INPUT;
    };
    match helper_apply(&text, &mut RegistryStore::system()) {
        Ok(encoded) => match fs::write(result, encoded) {
            Ok(()) => EXIT_DONE,
            // Changes were written but the result is lost; the UI reloads and sees them.
            Err(_) => EXIT_BAD_INPUT,
        },
        Err(_) => EXIT_BAD_INPUT,
    }
}

fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let pos = args.iter().position(|a| a == name)?;
    args.get(pos + 1).map(String::as_str)
}

/// Paths of one helper exchange, under `%LOCALAPPDATA%\envedit\pending`.
#[derive(Clone, Debug)]
pub struct Exchange {
    pub input: PathBuf,
    pub result: PathBuf,
}

impl Exchange {
    pub fn create(base: &Path, changes: &[PlannedChange]) -> Result<Exchange, String> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = base.join("pending");
        fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let id = format!("{}-{nanos}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
        let exchange = Exchange { input: dir.join(format!("{id}.in")), result: dir.join(format!("{id}.out")) };
        fs::write(&exchange.input, encode_input(changes)).map_err(|e| format!("write {}: {e}", exchange.input.display()))?;
        Ok(exchange)
    }

    pub fn remove(&self) {
        let _ = fs::remove_file(&self.input);
        let _ = fs::remove_file(&self.result);
    }
}

/// How a helper run ended, as seen by the UI.
#[derive(Debug)]
pub enum HelperEnd {
    /// The user declined the UAC prompt.
    Cancelled,
    /// The helper could not be started.
    LaunchFailed(String),
    /// The helper ran and exited with this code.
    Exited(u32),
}

/// Starts the helper elevated and waits for it. Blocks (including while the
/// UAC prompt is shown), so call it from a worker thread.
pub fn run_elevated_helper(exchange: &Exchange) -> HelperEnd {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return HelperEnd::LaunchFailed(e.to_string()),
    };
    let params = format!(
        "{APPLY_MACHINE_ARG} \"{}\" {RESULT_ARG} \"{}\"",
        exchange.input.display(),
        exchange.result.display()
    );
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(params);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: info is fully initialized; strings outlive the call.
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        return if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
            HelperEnd::Cancelled
        } else {
            HelperEnd::LaunchFailed(e.message())
        };
    }
    if info.hProcess.is_invalid() {
        return HelperEnd::LaunchFailed("no process handle".to_string());
    }
    let mut code = 0u32;
    // SAFETY: hProcess is a valid process handle owned by us and closed once.
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        let got = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if let Err(e) = got {
            return HelperEnd::LaunchFailed(e.message());
        }
    }
    HelperEnd::Exited(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::MemoryStore;

    const M: Scope = Scope::Machine;

    fn change(name: &str, before: Option<&str>, after: Option<&str>) -> PlannedChange {
        PlannedChange {
            scope: M,
            name: name.to_string(),
            before: before.map(|v| (v.to_string(), ValueType::ExpandSz)),
            after: after.map(|v| (v.to_string(), ValueType::ExpandSz)),
        }
    }

    #[test]
    fn input_round_trip() {
        let changes = vec![
            change("Path", Some(r"C:\Windows"), Some(r"C:\Windows;C:\x")),
            change("NEW", None, Some("1")),
            change("OLD", Some("2"), None),
        ];
        assert_eq!(decode_input(&encode_input(&changes)).unwrap(), changes);
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(decode_input("garbage").is_err());
        assert!(decode_input(&records::encode(&[])).is_err(), "empty input");
        let user = PlannedChange { scope: Scope::User, ..change("Path", Some("a"), Some("b")) };
        assert!(decode_input(&encode_input(&[user])).is_err(), "user scope");
        let bad_kind = records::encode(&[Record::new("drop", ["machine", "X", "-", "", "REG_SZ", "1"])]);
        assert!(decode_input(&bad_kind).is_err());
        let few_fields = records::encode(&[Record::new("change", ["machine", "X"])]);
        assert!(decode_input(&few_fields).is_err());
        let bad_type = records::encode(&[Record::new("change", ["machine", "X", "-", "", "TYPE:3", "1"])]);
        assert!(decode_input(&bad_type).is_err());
        let neither = records::encode(&[Record::new("change", ["machine", "X", "-", "", "-", ""])]);
        assert!(decode_input(&neither).is_err());
        let bad_name = records::encode(&[Record::new("change", ["machine", "A=B", "-", "", "REG_SZ", "1"])]);
        assert!(decode_input(&bad_name).is_err());
    }

    #[test]
    fn malformed_input_writes_nothing() {
        let mut store = MemoryStore::with(&[(M, "Path", r"C:\Windows", ValueType::ExpandSz)]);
        // First record valid, second malformed: nothing may be written.
        let mut text = encode_input(&[change("Path", Some(r"C:\Windows"), Some(r"C:\changed"))]);
        text.push_str("change\tmachine\tbroken\n");
        assert!(helper_apply(&text, &mut store).is_err());
        assert_eq!(store.get(M, "Path").unwrap().value, r"C:\Windows");
    }

    #[test]
    fn helper_applies_and_reports() {
        let mut store = MemoryStore::with(&[
            (M, "Path", r"C:\Windows", ValueType::ExpandSz),
            (M, "Moved", "theirs", ValueType::ExpandSz),
        ]);
        let input = encode_input(&[
            change("Path", Some(r"C:\Windows"), Some(r"C:\Windows;C:\x")),
            change("Moved", Some("mine-before"), Some("mine-after")),
            change("NEW", None, Some("1")),
        ]);
        let results = decode_results(&helper_apply(&input, &mut store).unwrap()).unwrap();
        let outcomes: Vec<_> = results.iter().map(|r| (r.name.as_str(), &r.outcome)).collect();
        assert_eq!(outcomes[0], ("Path", &Outcome::Applied));
        assert!(matches!(outcomes[1], ("Moved", Outcome::Conflict(_))));
        assert_eq!(outcomes[2], ("NEW", &Outcome::Applied));
        assert_eq!(store.get(M, "Path").unwrap().value, r"C:\Windows;C:\x");
        assert_eq!(store.get(M, "Moved").unwrap().value, "theirs");
    }

    #[test]
    fn results_round_trip() {
        let results = vec![
            VarResult { scope: M, name: "A".into(), outcome: Outcome::Applied },
            VarResult { scope: M, name: "B".into(), outcome: Outcome::Conflict("changed".into()) },
            VarResult { scope: M, name: "C".into(), outcome: Outcome::Failed("denied".into()) },
        ];
        assert_eq!(decode_results(&encode_results(&results)).unwrap(), results);
    }

    #[test]
    fn helper_without_valid_input_exits_non_zero() {
        let missing = std::env::temp_dir().join("envedit-no-such-input.txt");
        let args = vec![
            APPLY_MACHINE_ARG.to_string(),
            missing.display().to_string(),
            RESULT_ARG.to_string(),
            std::env::temp_dir().join("envedit-no-such-result.txt").display().to_string(),
        ];
        assert_eq!(run_helper(&args), EXIT_BAD_INPUT);
        assert_eq!(run_helper(&[APPLY_MACHINE_ARG.to_string()]), EXIT_BAD_INPUT);
    }

    #[test]
    fn exchange_files() {
        let base = std::env::temp_dir().join(format!("envedit-test-exchange-{}", std::process::id()));
        let changes = vec![change("Path", Some("a"), Some("b"))];
        let exchange = Exchange::create(&base, &changes).unwrap();
        assert_eq!(decode_input(&fs::read_to_string(&exchange.input).unwrap()).unwrap(), changes);
        exchange.remove();
        assert!(!exchange.input.exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn elevation_check_runs() {
        // Value depends on how the tests are run; it must simply not fail.
        let _ = is_elevated();
    }
}
