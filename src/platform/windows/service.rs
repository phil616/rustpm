#![allow(non_snake_case)]

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        OnceLock,
    },
};

use anyhow::{anyhow, Result};
use tokio::sync::watch;

pub const SERVICE_NAME: &str = "rustpm";

#[link(name = "advapi32")]
extern "system" {
    fn OpenSCManagerW(lpMachineName: *const u16, lpDatabaseName: *const u16, dwDesiredAccess: u32) -> *mut std::ffi::c_void;
    fn CloseServiceHandle(hSCObject: *mut std::ffi::c_void) -> i32;
    fn CreateServiceW(
        hSCManager: *mut std::ffi::c_void,
        lpServiceName: *const u16,
        lpDisplayName: *const u16,
        dwDesiredAccess: u32,
        dwServiceType: u32,
        dwStartType: u32,
        dwErrorControl: u32,
        lpBinaryPathName: *const u16,
        lpLoadOrderGroup: *const u16,
        lpdwTagId: *const u32,
        lpDependencies: *const u16,
        lpServiceStartName: *const u16,
        lpPassword: *const u16,
    ) -> *mut std::ffi::c_void;
    fn OpenServiceW(hSCManager: *mut std::ffi::c_void, lpServiceName: *const u16, dwDesiredAccess: u32) -> *mut std::ffi::c_void;
    fn DeleteService(hService: *mut std::ffi::c_void) -> i32;
    fn StartServiceW(hService: *mut std::ffi::c_void, dwNumServiceArgs: u32, lpServiceArgVectors: *const *const u16) -> i32;
    fn ControlService(hService: *mut std::ffi::c_void, dwControl: u32, lpServiceStatus: *mut SERVICE_STATUS) -> i32;
    fn StartServiceCtrlDispatcherW(lpServiceTable: *const SERVICE_TABLE_ENTRY) -> i32;
    fn RegisterServiceCtrlHandlerExW(lpServiceName: *const u16, lpHandlerProc: LphandlerFunctionEx, lpContext: *mut std::ffi::c_void) -> isize;
    fn SetServiceStatus(hService: isize, lpServiceStatus: *const SERVICE_STATUS) -> i32;
    fn ChangeServiceConfig2W(hService: *mut std::ffi::c_void, dwInfoLevel: u32, lpInfo: *const std::ffi::c_void) -> i32;
}

type LphandlerFunctionEx = unsafe extern "system" fn(u32, u32, *mut std::ffi::c_void, *mut std::ffi::c_void) -> u32;

#[repr(C)]
struct SERVICE_TABLE_ENTRY {
    lpServiceName: *const u16,
    lpServiceProc: Option<unsafe extern "system" fn(u32, *mut *const u16)>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SERVICE_STATUS {
    dwServiceType: u32,
    dwCurrentState: u32,
    dwControlsAccepted: u32,
    dwWin32ExitCode: u32,
    dwServiceSpecificExitCode: u32,
    dwCheckPoint: u32,
    dwWaitHint: u32,
}

impl Default for SERVICE_STATUS {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

const SC_MANAGER_ALL_ACCESS: u32 = 0xF01FF;
const SERVICE_ALL_ACCESS: u32 = 0xF01FF;
const SERVICE_WIN32_OWN_PROCESS: u32 = 0x00000010;
const SERVICE_DEMAND_START: u32 = 0x00000003;
const SERVICE_ERROR_NORMAL: u32 = 0x00000001;
const SERVICE_CONTROL_STOP: u32 = 0x00000013;
const SERVICE_CONTROL_SHUTDOWN: u32 = 0x00000005;
const SERVICE_STOPPED: u32 = 0x00000001;
const SERVICE_RUNNING: u32 = 0x00000004;
const SERVICE_START_PENDING: u32 = 0x00000002;
const SERVICE_STOP_PENDING: u32 = 0x00000006;
const SERVICE_ACCEPT_STOP: u32 = 0x00000020;
const SERVICE_ACCEPT_SHUTDOWN: u32 = 0x00000040;
const SERVICE_CONFIG_FAILURE_ACTIONS: u32 = 0x00000002;
const SC_ACTION_RESTART: u32 = 0x00000001;

#[repr(C)]
struct SC_ACTION {
    sc_action_type: u32,
    sc_delay: u32,
}

#[repr(C)]
struct SERVICE_FAILURE_ACTIONS {
    sc_dwResetPeriod: u32,
    sc_lpRebootMsg: *const u16,
    sc_lpCommand: *const u16,
    sc_cActions: u32,
    sc_lpsaActions: *const SC_ACTION,
}

static SERVICE_HANDLE: AtomicUsize = AtomicUsize::new(0);
static SERVICE_CHECKPOINT: AtomicU32 = AtomicU32::new(0);
static SERVICE_STOP_TX: OnceLock<watch::Sender<bool>> = OnceLock::new();
static DAEMON_RUNNER: OnceLock<std::sync::Mutex<Option<Box<dyn FnOnce() -> Result<()> + Send>>>> = OnceLock::new();

fn set_service_status(state: u32, exit_code: u32) {
    let handle = SERVICE_HANDLE.load(Ordering::SeqCst) as isize;
    if handle == 0 { return; }
    let checkpoint = if state == SERVICE_STOP_PENDING || state == SERVICE_START_PENDING {
        SERVICE_CHECKPOINT.fetch_add(1, Ordering::SeqCst)
    } else {
        0
    };
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: if state == SERVICE_RUNNING { SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN } else { 0 },
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: checkpoint,
        dwWaitHint: if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING { 30000 } else { 0 },
    };
    unsafe { SetServiceStatus(handle, &status); }
}

fn start_progress_reporter() {
    let handle = SERVICE_HANDLE.load(Ordering::SeqCst) as isize;
    if handle == 0 { return; }
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let state = SERVICE_CHECKPOINT.load(Ordering::SeqCst);
            if state == u32::MAX {
                break;
            }
            let cp = SERVICE_CHECKPOINT.fetch_add(1, Ordering::SeqCst);
            let status = SERVICE_STATUS {
                dwServiceType: SERVICE_WIN32_OWN_PROCESS,
                dwCurrentState: SERVICE_STOP_PENDING,
                dwControlsAccepted: 0,
                dwWin32ExitCode: 0,
                dwServiceSpecificExitCode: 0,
                dwCheckPoint: cp + 1,
                dwWaitHint: 30000,
            };
            unsafe { SetServiceStatus(handle, &status); }
        }
    });
}

unsafe extern "system" fn service_ctrl_handler(
    dw_control: u32,
    _dw_event_type: u32,
    _lp_event_data: *mut std::ffi::c_void,
    _lp_context: *mut std::ffi::c_void,
) -> u32 {
    match dw_control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            set_service_status(SERVICE_STOP_PENDING, 0);
            start_progress_reporter();
            if let Some(tx) = SERVICE_STOP_TX.get() {
                let _ = tx.send(true);
            }
            0
        }
        _ => 0,
    }
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut *const u16) {
    // Create watch channel BEFORE registering the handler, so the handler can
    // always signal the daemon even if STOP arrives during initialization.
    let (stop_tx, _) = watch::channel(false);
    let _ = SERVICE_STOP_TX.set(stop_tx);

    let name_wide: Vec<u16> = SERVICE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = RegisterServiceCtrlHandlerExW(name_wide.as_ptr(), service_ctrl_handler, std::ptr::null_mut());
    SERVICE_HANDLE.store(handle as usize, Ordering::SeqCst);

    set_service_status(SERVICE_START_PENDING, 0);
    set_service_status(SERVICE_RUNNING, 0);

    let result = {
        let cell = DAEMON_RUNNER.get_or_init(|| std::sync::Mutex::new(None));
        let mut guard = cell.lock().unwrap();
        guard.take()
    };

    let result = match result {
        Some(runner) => runner(),
        None => Err(anyhow!("no daemon runner registered")),
    };

    let exit_code = match &result {
        Ok(_) => 0,
        Err(e) => {
            tracing::error!(error = %e, "service daemon exited with error");
            1
        }
    };

    // Signal progress reporter to stop
    SERVICE_CHECKPOINT.store(u32::MAX, Ordering::SeqCst);
    set_service_status(SERVICE_STOPPED, exit_code);
}

pub fn run_as_service<F>(service_name: &'static str, daemon_fn: F) -> Result<()>
where
    F: FnOnce() -> Result<()> + Send + 'static,
{
    {
        let cell = DAEMON_RUNNER.get_or_init(|| std::sync::Mutex::new(None));
        let mut guard = cell.lock().unwrap();
        *guard = Some(Box::new(daemon_fn));
    }

    let name_wide: Vec<u16> = service_name.encode_utf16().chain(std::iter::once(0)).collect();
    let entry = SERVICE_TABLE_ENTRY {
        lpServiceName: name_wide.as_ptr(),
        lpServiceProc: Some(service_main),
    };

    unsafe {
        let ret = StartServiceCtrlDispatcherW(&entry);
        if ret == 0 {
            Err(anyhow!("StartServiceCtrlDispatcherW failed"))
        } else {
            Ok(())
        }
    }
}

pub fn service_stop_receiver() -> Option<watch::Receiver<bool>> {
    SERVICE_STOP_TX.get().map(|tx| tx.subscribe())
}

pub fn stop_service() -> Result<()> {
    unsafe {
        let sc_manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
        if sc_manager.is_null() {
            return Err(anyhow!("failed to open SC Manager"));
        }

        let service_name_wide: Vec<u16> = SERVICE_NAME.encode_utf16().chain(std::iter::once(0)).collect();

        let service_handle = OpenServiceW(sc_manager, service_name_wide.as_ptr(), SERVICE_ALL_ACCESS);
        if service_handle.is_null() {
            let _ = CloseServiceHandle(sc_manager);
            return Err(anyhow!("failed to open service"));
        }

        let mut status: SERVICE_STATUS = std::mem::zeroed();
        if ControlService(service_handle, SERVICE_CONTROL_STOP, &mut status) != 0 {
            let _ = CloseServiceHandle(service_handle);
            let _ = CloseServiceHandle(sc_manager);
            tracing::info!(service = SERVICE_NAME, "Windows Service stopped");
            Ok(())
        } else {
            let _ = CloseServiceHandle(service_handle);
            let _ = CloseServiceHandle(sc_manager);
            Err(anyhow!("failed to stop service"))
        }
    }
}

pub fn configure_service_failure_actions() -> Result<()> {
    let actions = [SC_ACTION {
        sc_action_type: SC_ACTION_RESTART,
        sc_delay: 5000,
    }];
    let failure_actions = SERVICE_FAILURE_ACTIONS {
        sc_dwResetPeriod: 86400,
        sc_lpRebootMsg: std::ptr::null(),
        sc_lpCommand: std::ptr::null(),
        sc_cActions: 1,
        sc_lpsaActions: actions.as_ptr(),
    };

    unsafe {
        let sc_manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
        if sc_manager.is_null() {
            return Err(anyhow!("failed to open SC Manager for failure actions"));
        }

        let service_name_wide: Vec<u16> = SERVICE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let service_handle = OpenServiceW(sc_manager, service_name_wide.as_ptr(), SERVICE_ALL_ACCESS);
        if service_handle.is_null() {
            let _ = CloseServiceHandle(sc_manager);
            return Err(anyhow!("failed to open service for failure actions"));
        }

        let ret = ChangeServiceConfig2W(
            service_handle,
            SERVICE_CONFIG_FAILURE_ACTIONS,
            &failure_actions as *const _ as *const std::ffi::c_void,
        );

        let _ = CloseServiceHandle(service_handle);
        let _ = CloseServiceHandle(sc_manager);

        if ret != 0 {
            tracing::info!(service = SERVICE_NAME, "SCM failure actions configured (auto-restart)");
            Ok(())
        } else {
            Err(anyhow!("ChangeServiceConfig2W failed"))
        }
    }
}

pub fn install_service(config_path: &PathBuf) -> Result<()> {
    let exe_path = super::get_current_exe_path()?;
    let exe_path_str = exe_path.to_string_lossy();
    let binary_path = format!("\"{}\" run --config \"{}\"", exe_path_str, config_path.to_string_lossy());

    unsafe {
        let sc_manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
        if sc_manager.is_null() {
            return Err(anyhow!("failed to open SC Manager"));
        }

        let service_name_wide: Vec<u16> = SERVICE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let display_name_wide: Vec<u16> = "rustpm".encode_utf16().chain(std::iter::once(0)).collect();
        let binary_path_wide: Vec<u16> = binary_path.encode_utf16().chain(std::iter::once(0)).collect();

        let service_handle = CreateServiceW(
            sc_manager,
            service_name_wide.as_ptr(),
            display_name_wide.as_ptr(),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_DEMAND_START,
            SERVICE_ERROR_NORMAL,
            binary_path_wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        );

        if service_handle.is_null() {
            let _ = CloseServiceHandle(sc_manager);
            return Err(anyhow!("failed to create service"));
        }

        let _ = CloseServiceHandle(service_handle);
        let _ = CloseServiceHandle(sc_manager);
    }

    tracing::info!(service = SERVICE_NAME, "Windows Service installed successfully");

    if let Err(e) = configure_service_failure_actions() {
        tracing::warn!(error = %e, "failed to configure SCM failure actions (non-fatal)");
    }

    Ok(())
}

pub fn uninstall_service() -> Result<()> {
    unsafe {
        let sc_manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
        if sc_manager.is_null() {
            return Err(anyhow!("failed to open SC Manager"));
        }

        let service_name_wide: Vec<u16> = SERVICE_NAME.encode_utf16().chain(std::iter::once(0)).collect();

        let service_handle = OpenServiceW(sc_manager, service_name_wide.as_ptr(), SERVICE_ALL_ACCESS);
        if service_handle.is_null() {
            let _ = CloseServiceHandle(sc_manager);
            return Err(anyhow!("failed to open service"));
        }

        if DeleteService(service_handle) != 0 {
            let _ = CloseServiceHandle(service_handle);
            let _ = CloseServiceHandle(sc_manager);
            tracing::info!(service = SERVICE_NAME, "Windows Service uninstalled successfully");
            Ok(())
        } else {
            let _ = CloseServiceHandle(service_handle);
            let _ = CloseServiceHandle(sc_manager);
            Err(anyhow!("failed to delete service"))
        }
    }
}

pub fn start_service() -> Result<()> {
    unsafe {
        let sc_manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS);
        if sc_manager.is_null() {
            return Err(anyhow!("failed to open SC Manager"));
        }

        let service_name_wide: Vec<u16> = SERVICE_NAME.encode_utf16().chain(std::iter::once(0)).collect();

        let service_handle = OpenServiceW(sc_manager, service_name_wide.as_ptr(), SERVICE_ALL_ACCESS);
        if service_handle.is_null() {
            let _ = CloseServiceHandle(sc_manager);
            return Err(anyhow!("failed to open service"));
        }

        if StartServiceW(service_handle, 0, std::ptr::null()) != 0 {
            let _ = CloseServiceHandle(service_handle);
            let _ = CloseServiceHandle(sc_manager);
            tracing::info!(service = SERVICE_NAME, "Windows Service started");
            Ok(())
        } else {
            let _ = CloseServiceHandle(service_handle);
            let _ = CloseServiceHandle(sc_manager);
            Err(anyhow!("failed to start service"))
        }
    }
}

pub fn service_exists() -> bool {
    unsafe {
        let sc_manager = match OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_ALL_ACCESS) {
            h if !h.is_null() => h,
            _ => return false,
        };

        let service_name_wide: Vec<u16> = SERVICE_NAME.encode_utf16().chain(std::iter::once(0)).collect();

        let service_handle = OpenServiceW(sc_manager, service_name_wide.as_ptr(), SERVICE_ALL_ACCESS);

        let _ = CloseServiceHandle(sc_manager);

        if !service_handle.is_null() {
            let _ = CloseServiceHandle(service_handle);
            true
        } else {
            false
        }
    }
}
