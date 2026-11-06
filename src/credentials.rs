use anyhow::{bail, Result};
use windows_sys::Win32::Security::Credentials::*;

fn target(name: &str) -> Vec<u16> {
    format!("GoodSearch/{name}\0").encode_utf16().collect()
}

pub fn load() -> Result<Option<String>> {
    load_named("OpenAI")
}
pub fn load_named(name: &str) -> Result<Option<String>> {
    unsafe {
        let mut credential = std::ptr::null_mut();
        if CredReadW(target(name).as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(1168) {
                return Ok(None);
            }
            return Err(error.into());
        }
        if (*credential).CredentialBlobSize == 0 {
            CredFree(credential.cast());
            return Ok(None);
        }
        let bytes = std::slice::from_raw_parts(
            (*credential).CredentialBlob,
            (*credential).CredentialBlobSize as usize,
        );
        let key = String::from_utf8(bytes.to_vec());
        CredFree(credential.cast());
        Ok(Some(key?))
    }
}

pub fn save(key: &str) -> Result<()> {
    save_named("OpenAI", key)
}
pub fn save_named(name: &str, key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() || key.len() > 2500 || key.contains(['\r', '\n']) {
        bail!("Invalid API key");
    }
    let mut target = target(name);
    let mut user: Vec<u16> = "Good Search\0".encode_utf16().collect();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: key.len() as u32,
        CredentialBlob: key.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: user.as_mut_ptr(),
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { CredWriteW(&credential, 0) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

pub fn remove() -> Result<()> {
    remove_named("OpenAI")
}
pub fn remove_named(name: &str) -> Result<()> {
    if unsafe { CredDeleteW(target(name).as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(1168) {
            return Err(error.into());
        }
    }
    Ok(())
}
