use anyhow::Result;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        },
    },
};

pub fn create_control_pipe(name: &str, first: bool) -> Result<NamedPipeServer> {
    // Only SYSTEM and elevated local administrators may manage tasks.
    let sddl: Vec<u16> = "D:P(A;;GA;;;SY)(A;;GA;;;BA)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            1,
            &mut descriptor,
            None,
        )?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let result = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, &attributes as *const _ as *mut _);
        LocalFree(HLOCAL(descriptor.0));
        Ok(result?)
    }
}
