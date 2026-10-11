use anyhow::{anyhow, Context, Result};
use std::{
    collections::BTreeMap,
    os::windows::{ffi::OsStrExt, io::AsRawHandle, process::ExitStatusExt},
    path::PathBuf,
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Security::*,
        System::{Environment::*, JobObjects::*, Threading::*},
    },
};

pub struct OwnedHandle(pub usize);
impl OwnedHandle {
    pub fn raw(&self) -> HANDLE {
        HANDLE(self.0 as *mut std::ffi::c_void)
    }
}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.raw());
        }
    }
}

pub fn create_job() -> Result<OwnedHandle> {
    unsafe {
        let job = OwnedHandle(CreateJobObjectW(None, None)?.0 as usize);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            &limits as *const _ as _,
            std::mem::size_of_val(&limits) as u32,
        )?;
        Ok(job)
    }
}

pub struct ManagedChild {
    process: OwnedHandle,
    pub job: OwnedHandle,
    pid: u32,
}
impl ManagedChild {
    pub fn id(&self) -> Option<u32> {
        Some(self.pid)
    }
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        loop {
            let result = unsafe { WaitForSingleObject(self.process.raw(), 0) };
            if result == WAIT_OBJECT_0 {
                let mut code = 0;
                unsafe { GetExitCodeProcess(self.process.raw(), &mut code) }
                    .map_err(std::io::Error::from)?;
                // A parent's exit must not leave descendants alive during restart.
                self.terminate_tree()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                loop {
                    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                    unsafe {
                        QueryInformationJobObject(
                            self.job.raw(),
                            JobObjectBasicAccountingInformation,
                            &mut accounting as *mut _ as _,
                            std::mem::size_of_val(&accounting) as u32,
                            None,
                        )
                    }
                    .map_err(std::io::Error::from)?;
                    if accounting.ActiveProcesses == 0 {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                return Ok(std::process::ExitStatus::from_raw(code));
            }
            if result == WAIT_FAILED {
                return Err(std::io::Error::last_os_error());
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }
    pub fn terminate_tree(&self) -> Result<()> {
        unsafe {
            TerminateJobObject(self.job.raw(), 1)?;
        }
        Ok(())
    }
}

fn wide(value: &str) -> Result<Vec<u16>> {
    if value.contains('\0') {
        return Err(anyhow!("Windows argument contains NUL"));
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}

fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.chars().any(|c| c.is_whitespace() || c == '"') {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat('\\').take(if c == '"' { slashes * 2 + 1 } else { slashes }));
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat('\\').take(slashes * 2));
    out.push('"');
    out
}

// Passwordless service accounts work before interactive logon. Other identities
// require credentials: never silently execute them under the daemon account.
fn service_token(user: Option<&str>, elevated: bool) -> Result<Option<OwnedHandle>> {
    if elevated {
        if user.is_some() {
            return Err(anyhow!("Windows elevated tasks use the helper identity; omit user (account switching requires credentials)"));
        }
        return Ok(None);
    }
    let user = user.unwrap_or("LocalService");
    let account = match user.to_ascii_lowercase().as_str() {
        "localservice" | "nt authority\\localservice" => "LocalService",
        "networkservice" | "nt authority\\networkservice" => "NetworkService",
        _ => return Err(anyhow!("Windows user '{}' requires credentials; supported passwordless normal accounts: LocalService, NetworkService", user)),
    };
    let name = wide(account)?;
    let domain = wide("NT AUTHORITY")?;
    let mut token = HANDLE::default();
    unsafe {
        LogonUserW(
            PCWSTR(name.as_ptr()),
            PCWSTR(domain.as_ptr()),
            PCWSTR::null(),
            LOGON32_LOGON_SERVICE,
            LOGON32_PROVIDER_DEFAULT,
            &mut token,
        )?;
    }
    let token = OwnedHandle(token.0 as usize);
    // Service-account tokens may start at system integrity. Normal tasks use
    // medium integrity even though their SID is a passwordless service account.
    unsafe {
        let sid_text = wide("S-1-16-8192")?;
        let mut sid = PSID::default();
        Authorization::ConvertStringSidToSidW(PCWSTR(sid_text.as_ptr()), &mut sid)?;
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: sid,
                Attributes: windows::Win32::System::SystemServices::SE_GROUP_INTEGRITY as u32,
            },
        };
        let result = SetTokenInformation(
            token.raw(),
            TokenIntegrityLevel,
            &label as *const _ as _,
            std::mem::size_of_val(&label) as u32 + GetLengthSid(sid),
        );
        LocalFree(HLOCAL(sid.0));
        result?;
    }
    Ok(Some(token))
}

struct Environment(*mut std::ffi::c_void);
impl Drop for Environment {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = DestroyEnvironmentBlock(self.0);
            }
        }
    }
}

// Keep UTF-16 inherited values intact and replace configured ASCII names
// case-insensitively, including PATH/Path. The OS requires a double-NUL block.
unsafe fn merge_environment(
    base: *const u16,
    overrides: &BTreeMap<String, String>,
) -> Result<Vec<u16>> {
    let mut entries = BTreeMap::<Vec<u16>, Vec<u16>>::new();
    let folded = |key: &[u16]| {
        key.iter()
            .map(|c| {
                if (b'a' as u16..=b'z' as u16).contains(c) {
                    c - 32
                } else {
                    *c
                }
            })
            .collect::<Vec<_>>()
    };
    let mut cursor = base;
    while *cursor != 0 {
        let mut len = 0;
        while *cursor.add(len) != 0 {
            len += 1;
        }
        let entry = std::slice::from_raw_parts(cursor, len).to_vec();
        if let Some(offset) = entry
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(i, c)| (*c == b'=' as u16).then_some(i))
        {
            entries.insert(folded(&entry[..offset]), entry);
        }
        cursor = cursor.add(len + 1);
    }
    for (name, value) in overrides {
        if name.is_empty() || name.contains(['=', '\0']) {
            return Err(anyhow!("invalid Windows environment name"));
        }
        let key: Vec<u16> = name.encode_utf16().collect();
        let mut entry = wide(&format!("{name}={value}"))?;
        entry.pop();
        entries.insert(folded(&key), entry);
    }
    let mut block = Vec::new();
    for entry in entries.values() {
        block.extend_from_slice(entry);
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

struct AttributeList {
    _storage: Vec<usize>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}
impl AttributeList {
    fn new(handles: &[HANDLE]) -> Result<Self> {
        unsafe {
            let mut bytes = 0;
            // The sizing call is expected to fail with an insufficient buffer.
            let sizing = InitializeProcThreadAttributeList(
                LPPROC_THREAD_ATTRIBUTE_LIST::default(),
                1,
                0,
                &mut bytes,
            );
            if bytes == 0 {
                sizing?;
                return Err(anyhow!("empty process attribute list"));
            }
            let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr() as _);
            InitializeProcThreadAttributeList(list, 1, 0, &mut bytes)?;
            let attributes = Self {
                _storage: storage,
                list,
            };
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr() as _),
                std::mem::size_of_val(handles),
                None,
                None,
            )?;
            Ok(attributes)
        }
    }
}
impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.list);
        }
    }
}

pub fn spawn_managed(
    cmd: &str,
    args: &[String],
    user: Option<&str>,
    elevated: bool,
    cwd: Option<&PathBuf>,
    stdout: Option<&PathBuf>,
    stderr: Option<&PathBuf>,
    overrides: &BTreeMap<String, String>,
) -> Result<ManagedChild> {
    let token =
        service_token(user, elevated).context("failed to select Windows process identity")?;
    let job = create_job()?;
    let application = wide(cmd)?;
    let mut command = wide(
        &std::iter::once(cmd)
            .chain(args.iter().map(String::as_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" "),
    )?;
    let cwd = cwd.map(|p| wide(&p.to_string_lossy())).transpose()?;
    let mut environment = Environment(std::ptr::null_mut());
    if let Some(token) = &token {
        unsafe {
            CreateEnvironmentBlock(&mut environment.0, token.raw(), false)?;
        }
    }
    let block = if environment.0.is_null() {
        let mut base = Vec::new();
        for (name, value) in std::env::vars_os() {
            base.extend(name.encode_wide());
            base.push(b'=' as u16);
            base.extend(value.encode_wide());
            base.push(0);
        }
        if base.is_empty() {
            base.push(0);
        }
        base.push(0);
        unsafe { merge_environment(base.as_ptr(), overrides)? }
    } else {
        unsafe { merge_environment(environment.0.cast(), overrides)? }
    };
    let input = std::fs::File::open("NUL")?;
    let open_output = |path: Option<&PathBuf>| -> std::io::Result<std::fs::File> {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.map_or(std::path::Path::new("NUL"), |p| p.as_path()))
    };
    let output = open_output(stdout)?;
    let error = open_output(stderr)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = HANDLE(input.as_raw_handle());
    startup.StartupInfo.hStdOutput = HANDLE(output.as_raw_handle());
    startup.StartupInfo.hStdError = HANDLE(error.as_raw_handle());
    let handles = [
        startup.StartupInfo.hStdInput,
        startup.StartupInfo.hStdOutput,
        startup.StartupInfo.hStdError,
    ];
    let attributes = AttributeList::new(&handles)?;
    startup.lpAttributeList = attributes.list;
    static SPAWN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = SPAWN_LOCK
        .lock()
        .map_err(|_| anyhow!("spawn lock poisoned"))?;
    for handle in handles {
        unsafe {
            SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT)?;
        }
    }
    let mut info = PROCESS_INFORMATION::default();
    let flags = CREATE_SUSPENDED
        | CREATE_NEW_PROCESS_GROUP
        | CREATE_UNICODE_ENVIRONMENT
        | CREATE_NO_WINDOW
        | EXTENDED_STARTUPINFO_PRESENT;
    let cwd_ptr = cwd.as_ref().map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr()));
    let env = Some(block.as_ptr() as *const std::ffi::c_void);
    let result = unsafe {
        if let Some(token) = &token {
            CreateProcessAsUserW(
                token.raw(),
                PCWSTR(application.as_ptr()),
                PWSTR(command.as_mut_ptr()),
                None,
                None,
                true,
                flags,
                env,
                cwd_ptr,
                &startup.StartupInfo,
                &mut info,
            )
        } else {
            CreateProcessW(
                PCWSTR(application.as_ptr()),
                PWSTR(command.as_mut_ptr()),
                None,
                None,
                true,
                flags,
                env,
                cwd_ptr,
                &startup.StartupInfo,
                &mut info,
            )
        }
    };
    // Only these three handles are inherited; close our copies before releasing the launch lock.
    drop((input, output, error));
    result.context("failed to create Windows process under requested identity")?;
    let process = OwnedHandle(info.hProcess.0 as usize);
    let thread = OwnedHandle(info.hThread.0 as usize);
    unsafe {
        if let Err(err) = AssignProcessToJobObject(job.raw(), process.raw()) {
            TerminateProcess(process.raw(), 1)
                .context("failed to clean up unassigned suspended process")?;
            WaitForSingleObject(process.raw(), INFINITE);
            return Err(err.into());
        }
        if ResumeThread(thread.raw()) == u32::MAX {
            let err = std::io::Error::last_os_error();
            TerminateJobObject(job.raw(), 1)?;
            return Err(err.into());
        }
    }
    Ok(ManagedChild {
        process,
        job,
        pid: info.dwProcessId,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_quoting_round_trips_windows_parser() {
        // Tested by launching a process below; edge cases also guard quote output.
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("C:\\some dir\\"), "\"C:\\some dir\\\\\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn unsupported_accounts_fail_before_launch() {
        assert!(service_token(Some("unconfigured-user"), false).is_err());
        assert!(service_token(Some("Administrator"), true).is_err());
    }

    #[tokio::test]
    async fn managed_process_reports_exit_and_releases_tree() {
        let cmd = format!(
            "{}\\System32\\cmd.exe",
            std::env::var("SystemRoot").unwrap()
        );
        let mut child = spawn_managed(
            &cmd,
            &["/d".into(), "/c".into(), "exit".into(), "7".into()],
            None,
            true,
            None,
            None,
            None,
            &BTreeMap::new(),
        )
        .unwrap();
        let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.code(), Some(7));
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                child.job.raw(),
                JobObjectBasicAccountingInformation,
                &mut accounting as *mut _ as _,
                std::mem::size_of_val(&accounting) as u32,
                None,
            )
            .unwrap();
        }
        assert_eq!(accounting.ActiveProcesses, 0);
    }

    #[tokio::test]
    async fn termination_cleans_descendants() {
        let cmd = format!(
            "{}\\System32\\cmd.exe",
            std::env::var("SystemRoot").unwrap()
        );
        let mut child = spawn_managed(
            &cmd,
            &[
                "/d".into(),
                "/c".into(),
                "ping -n 30 127.0.0.1 > nul".into(),
            ],
            None,
            true,
            None,
            None,
            None,
            &BTreeMap::new(),
        )
        .unwrap();
        // Wait for the shell to create a descendant rather than assume a fixed delay.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                unsafe {
                    QueryInformationJobObject(
                        child.job.raw(),
                        JobObjectBasicAccountingInformation,
                        &mut accounting as *mut _ as _,
                        std::mem::size_of_val(&accounting) as u32,
                        None,
                    )
                    .unwrap();
                }
                if accounting.ActiveProcesses >= 2 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        child.terminate_tree().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
    }
}
