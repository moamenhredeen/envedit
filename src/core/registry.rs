//! Storage of persistent environment variables.

#[cfg(test)]
use std::collections::BTreeMap;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SAM_FLAGS, REG_VALUE_TYPE,
    RegCloseKey, RegDeleteValueW, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{HSTRING, PWSTR};

use super::model::{Key, Scope, ValueType, Var};

pub type StoreResult<T> = Result<T, String>;

/// Read and write access to the variables of both scopes.
pub trait EnvStore {
    /// All variables of a scope, sorted by name.
    fn load(&self, scope: Scope) -> StoreResult<Vec<Var>>;
    /// Current value and type of one variable, `None` if it does not exist.
    fn read(&self, scope: Scope, name: &str) -> StoreResult<Option<(String, ValueType)>>;
    fn write(&mut self, scope: Scope, name: &str, value: &str, ty: ValueType) -> StoreResult<()>;
    fn delete(&mut self, scope: Scope, name: &str) -> StoreResult<()>;
}

/// Predefined registry root. Kept as an enum so stores stay `Send`.
#[derive(Clone, Copy, Debug)]
pub enum Root {
    CurrentUser,
    LocalMachine,
}

impl Root {
    fn hkey(self) -> HKEY {
        match self {
            Root::CurrentUser => HKEY_CURRENT_USER,
            Root::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }
}

pub const USER_KEY: &str = "Environment";
pub const MACHINE_KEY: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// The Windows registry.
#[derive(Clone, Debug)]
pub struct RegistryStore {
    user: (Root, String),
    machine: (Root, String),
}

impl RegistryStore {
    /// The real User and Machine environment keys.
    pub fn system() -> RegistryStore {
        RegistryStore::with_keys((Root::CurrentUser, USER_KEY), (Root::LocalMachine, MACHINE_KEY))
    }

    pub fn with_keys(user: (Root, &str), machine: (Root, &str)) -> RegistryStore {
        RegistryStore {
            user: (user.0, user.1.to_string()),
            machine: (machine.0, machine.1.to_string()),
        }
    }

    fn location(&self, scope: Scope) -> &(Root, String) {
        match scope {
            Scope::User => &self.user,
            Scope::Machine => &self.machine,
        }
    }

    fn open(&self, scope: Scope, access: REG_SAM_FLAGS) -> StoreResult<OpenKey> {
        let (root, path) = self.location(scope);
        let mut hkey = HKEY::default();
        // SAFETY: valid root key, NUL-terminated path, out pointer to a local.
        let status = unsafe { RegOpenKeyExW(root.hkey(), &HSTRING::from(path.as_str()), None, access, &mut hkey) };
        check(status, &format!("open {} key", scope.label()))?;
        Ok(OpenKey(hkey))
    }
}

struct OpenKey(HKEY);

impl Drop for OpenKey {
    fn drop(&mut self) {
        // SAFETY: the handle came from RegOpenKeyExW and is closed once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn check(status: WIN32_ERROR, what: &str) -> StoreResult<()> {
    if status.is_ok() {
        Ok(())
    } else {
        Err(format!("{what}: {}", windows::core::Error::from(status.to_hresult()).message()))
    }
}

/// Converts registry string data to text, dropping trailing NULs.
fn wide_to_string(data: &[u16]) -> String {
    let end = data.iter().rposition(|&c| c != 0).map_or(0, |i| i + 1);
    String::from_utf16_lossy(&data[..end])
}

fn wide_bytes(data: &[u16]) -> &[u8] {
    // SAFETY: u16 has no padding; the byte view covers the same memory.
    unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2) }
}

fn string_value(ty: ValueType, data: &[u16]) -> String {
    if ty.is_editable() { wide_to_string(data) } else { String::new() }
}

impl EnvStore for RegistryStore {
    fn load(&self, scope: Scope) -> StoreResult<Vec<Var>> {
        let key = self.open(scope, KEY_QUERY_VALUE)?;
        let mut max_name = 0u32;
        let mut max_data = 0u32;
        // SAFETY: open key, out pointers to locals.
        let status = unsafe {
            RegQueryInfoKeyW(
                key.0,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&mut max_name),
                Some(&mut max_data),
                None,
                None,
            )
        };
        check(status, "query key info")?;

        let mut vars = Vec::new();
        let mut name_buf = vec![0u16; max_name as usize + 1];
        let mut data_buf = vec![0u16; max_data as usize / 2 + 1];
        let mut index = 0;
        loop {
            let mut name_len = name_buf.len() as u32;
            let mut data_len = (data_buf.len() * 2) as u32;
            let mut raw_type = 0u32;
            // SAFETY: buffers and their lengths match; out pointers to locals.
            let status = unsafe {
                RegEnumValueW(
                    key.0,
                    index,
                    Some(PWSTR(name_buf.as_mut_ptr())),
                    &mut name_len,
                    None,
                    Some(&mut raw_type),
                    Some(data_buf.as_mut_ptr().cast()),
                    Some(&mut data_len),
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            if status == ERROR_MORE_DATA {
                // A value grew between the size query and now: enlarge and retry.
                name_buf.resize(name_buf.len() * 2, 0);
                data_buf.resize((data_len as usize / 2 + 1).max(data_buf.len() * 2), 0);
                continue;
            }
            check(status, "enumerate values")?;
            let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            let ty = ValueType::from_raw(raw_type);
            let value = string_value(ty, &data_buf[..data_len as usize / 2]);
            vars.push(Var { name, value, ty });
            index += 1;
        }
        vars.sort_by_key(|v| Key::new(scope, &v.name));
        Ok(vars)
    }

    fn read(&self, scope: Scope, name: &str) -> StoreResult<Option<(String, ValueType)>> {
        let key = self.open(scope, KEY_QUERY_VALUE)?;
        let name_w = HSTRING::from(name);
        let mut buf: Vec<u16> = Vec::new();
        loop {
            let mut raw_type = REG_VALUE_TYPE::default();
            let mut len = (buf.len() * 2) as u32;
            let data = if buf.is_empty() { None } else { Some(buf.as_mut_ptr().cast()) };
            // SAFETY: buffer pointer and length match; out pointers to locals.
            let status = unsafe { RegQueryValueExW(key.0, &name_w, None, Some(&mut raw_type), data, Some(&mut len)) };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            let fits = !buf.is_empty() && status.is_ok();
            if status.is_ok() && !fits || status == ERROR_MORE_DATA {
                buf.resize(len as usize / 2 + 1, 0);
                continue;
            }
            check(status, &format!("read {name}"))?;
            let ty = ValueType::from_raw(raw_type.0);
            return Ok(Some((string_value(ty, &buf[..len as usize / 2]), ty)));
        }
    }

    fn write(&mut self, scope: Scope, name: &str, value: &str, ty: ValueType) -> StoreResult<()> {
        let key = self.open(scope, KEY_SET_VALUE)?;
        let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: open key, NUL-terminated name, data slice with terminating NUL.
        let status = unsafe {
            RegSetValueExW(key.0, &HSTRING::from(name), None, REG_VALUE_TYPE(ty.raw()), Some(wide_bytes(&data)))
        };
        check(status, &format!("write {name}"))
    }

    fn delete(&mut self, scope: Scope, name: &str) -> StoreResult<()> {
        let key = self.open(scope, KEY_SET_VALUE)?;
        // SAFETY: open key, NUL-terminated name.
        let status = unsafe { RegDeleteValueW(key.0, &HSTRING::from(name)) };
        check(status, &format!("delete {name}"))
    }
}

/// In-memory store for tests.
#[cfg(test)]
#[derive(Default, Debug)]
pub struct MemoryStore {
    pub vars: BTreeMap<Key, Var>,
    /// Names whose writes fail, to exercise partial failures.
    pub failing: Vec<String>,
}

#[cfg(test)]
impl MemoryStore {
    pub fn with(vars: &[(Scope, &str, &str, ValueType)]) -> MemoryStore {
        let mut store = MemoryStore::default();
        for &(scope, name, value, ty) in vars {
            store.vars.insert(Key::new(scope, name), Var::new(name, value, ty));
        }
        store
    }

    pub fn get(&self, scope: Scope, name: &str) -> Option<&Var> {
        self.vars.get(&Key::new(scope, name))
    }
}

#[cfg(test)]
impl EnvStore for MemoryStore {
    fn load(&self, scope: Scope) -> StoreResult<Vec<Var>> {
        Ok(self.vars.iter().filter(|(k, _)| k.scope == scope).map(|(_, v)| v.clone()).collect())
    }

    fn read(&self, scope: Scope, name: &str) -> StoreResult<Option<(String, ValueType)>> {
        Ok(self.get(scope, name).map(|v| (v.value.clone(), v.ty)))
    }

    fn write(&mut self, scope: Scope, name: &str, value: &str, ty: ValueType) -> StoreResult<()> {
        if self.failing.iter().any(|f| f == name) {
            return Err(format!("write {name}: simulated failure"));
        }
        self.vars.insert(Key::new(scope, name), Var::new(name, value, ty));
        Ok(())
    }

    fn delete(&mut self, scope: Scope, name: &str) -> StoreResult<()> {
        if self.failing.iter().any(|f| f == name) {
            return Err(format!("delete {name}: simulated failure"));
        }
        self.vars.remove(&Key::new(scope, name));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::{KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, RegCreateKeyExW, RegDeleteTreeW};
    use windows::core::PCWSTR;

    #[test]
    fn memory_store_round_trip() {
        let mut store = MemoryStore::default();
        store.write(Scope::User, "A", "1", ValueType::Sz).unwrap();
        assert_eq!(store.read(Scope::User, "a").unwrap(), Some(("1".to_string(), ValueType::Sz)));
        store.delete(Scope::User, "A").unwrap();
        assert_eq!(store.read(Scope::User, "A").unwrap(), None);
    }

    /// Scratch key under HKCU, removed again when dropped.
    struct ScratchKey(String);

    impl ScratchKey {
        fn create(name: &str) -> ScratchKey {
            let path = format!(r"Software\envedit-test\{name}");
            let mut hkey = HKEY::default();
            // SAFETY: valid root, NUL-terminated path, out pointer to a local.
            let status = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    &HSTRING::from(path.as_str()),
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_ALL_ACCESS,
                    None,
                    &mut hkey,
                    None,
                )
            };
            check(status, "create scratch key").unwrap();
            drop(OpenKey(hkey));
            ScratchKey(path)
        }
    }

    impl Drop for ScratchKey {
        fn drop(&mut self) {
            // SAFETY: valid root, NUL-terminated path.
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(self.0.as_str()));
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(r"Software\envedit-test"));
            }
        }
    }

    #[test]
    fn registry_round_trip_on_scratch_key() {
        let scratch = ScratchKey::create("round-trip");
        let mut store = RegistryStore::with_keys((Root::CurrentUser, &scratch.0), (Root::CurrentUser, &scratch.0));
        let long = format!("C:\\{}", "x".repeat(5_000));

        store.write(Scope::User, "PLAIN", r"C:\plain", ValueType::Sz).unwrap();
        store.write(Scope::User, "EXPAND", r"%USERPROFILE%\bin", ValueType::ExpandSz).unwrap();
        store.write(Scope::User, "LONG", &long, ValueType::Sz).unwrap();

        assert_eq!(store.read(Scope::User, "plain").unwrap(), Some((r"C:\plain".to_string(), ValueType::Sz)));
        assert_eq!(
            store.read(Scope::User, "EXPAND").unwrap(),
            Some((r"%USERPROFILE%\bin".to_string(), ValueType::ExpandSz))
        );
        assert_eq!(store.read(Scope::User, "LONG").unwrap(), Some((long.clone(), ValueType::Sz)));

        let loaded = store.load(Scope::User).unwrap();
        let names: Vec<_> = loaded.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["EXPAND", "LONG", "PLAIN"]);
        assert_eq!(loaded[1].value, long);
        assert_eq!(loaded[0].ty, ValueType::ExpandSz);

        store.delete(Scope::User, "PLAIN").unwrap();
        assert_eq!(store.read(Scope::User, "PLAIN").unwrap(), None);
        assert!(store.delete(Scope::User, "PLAIN").is_err());
    }

    #[test]
    fn loads_real_environment_keys_unelevated() {
        let store = RegistryStore::system();
        for scope in [Scope::User, Scope::Machine] {
            let vars = store.load(scope).unwrap();
            assert!(
                vars.iter().any(|v| v.name.eq_ignore_ascii_case("Path")),
                "{} scope has no Path",
                scope.label()
            );
        }
    }
}
