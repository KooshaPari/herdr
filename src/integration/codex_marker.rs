#[cfg(windows)]
mod native {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Seek, Write};
    use std::os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    use std::path::Path;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    const READY: &[u8] = b"HERDR-CODEX-MARKER-1:READY\n";
    const ACTIVE: &[u8] = b"HERDR-CODEX-MARKER-1:IN-PROGRESS\n";

    pub(in crate::integration) struct Marker(File);

    fn update(file: &mut File, state: &[u8]) -> io::Result<()> {
        file.rewind()?;
        file.set_len(0)?;
        file.write_all(state)?;
        file.sync_all()
    }

    impl Marker {
        pub(in crate::integration) fn complete(mut self) -> io::Result<()> {
            update(&mut self.0, READY)
        }
    }

    fn owner_is_current_user(owner: windows_sys::Win32::Security::PSID) -> io::Result<bool> {
        use std::ptr::null_mut;
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER},
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let mut token = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut needed = 0;
            unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut needed) };
            if needed == 0 {
                return Err(io::Error::last_os_error());
            }
            let words = (needed as usize).div_ceil(std::mem::size_of::<usize>());
            let mut buffer = vec![0usize; words];
            if unsafe {
                GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
            Ok(unsafe { EqualSid(owner, user.User.Sid) } != 0)
        })();
        unsafe {
            CloseHandle(token);
        }
        result
    }

    fn private_acl(file: &File) -> io::Result<bool> {
        use std::ptr::null_mut;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo,
                    SDDL_REVISION_1, SE_FILE_OBJECT,
                },
                DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
            },
        };
        let mut descriptor = null_mut();
        let mut owner = null_mut();
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let result = (|| {
            if owner.is_null() || !owner_is_current_user(owner)? {
                return Ok(false);
            }
            let mut text = null_mut();
            if unsafe {
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    SDDL_REVISION_1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let value = unsafe { widestring::U16CStr::from_ptr_str(text) }.to_string_lossy();
            unsafe {
                LocalFree(text.cast());
            }
            Ok(value == "D:P(A;;FA;;;SY)(A;;FA;;;OW)")
        })();
        unsafe {
            LocalFree(descriptor.cast());
        }
        result
    }

    #[cfg(test)]
    #[test]
    fn install_codex_snapshot_rejects_foreign_owner_sid() {
        use windows_sys::Win32::{
            Foundation::LocalFree, Security::Authorization::ConvertStringSidToSidW,
        };
        let everyone = widestring::U16CString::from_str("S-1-1-0").unwrap();
        let mut sid = std::ptr::null_mut();
        assert_ne!(
            unsafe { ConvertStringSidToSidW(everyone.as_ptr(), &mut sid) },
            0
        );
        let matches = owner_is_current_user(sid).unwrap();
        unsafe {
            LocalFree(sid.cast());
        }
        assert!(!matches);
    }

    pub(in crate::integration) fn acquire(path: &Path) -> io::Result<Marker> {
        match crate::platform::create_config_temporary(path, true) {
            Ok(mut initial) => {
                initial.write_all(READY)?;
                initial.sync_all()?;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other(
                "Codex marker is not a regular non-reparse file",
            ));
        }
        let mut identity = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut identity) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if identity.nNumberOfLinks != 1 {
            return Err(io::Error::other("Codex marker has multiple hard links"));
        }
        file.try_lock().map_err(io::Error::other)?;
        if !private_acl(&file)? {
            return Err(io::Error::other(
                "Codex marker ACL is not the expected protected owner/SYSTEM ACL",
            ));
        }
        let mut contents = Vec::new();
        (&mut file).take(1024).read_to_end(&mut contents)?;
        if contents != READY {
            return Err(io::Error::other("Codex install marker is incomplete or unrecognized; inspect it and backups before retrying"));
        }
        update(&mut file, ACTIVE)?;
        Ok(Marker(file))
    }
}

#[cfg(windows)]
pub(super) use native::acquire;

#[cfg(unix)]
mod unix {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Read, Seek, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;

    const READY: &[u8] = b"HERDR-CODEX-MARKER-1:READY\n";
    const ACTIVE: &[u8] = b"HERDR-CODEX-MARKER-1:IN-PROGRESS\n";

    pub(in crate::integration) struct Marker(File);
    fn update(file: &mut File, state: &[u8]) -> io::Result<()> {
        file.rewind()?;
        file.set_len(0)?;
        file.write_all(state)?;
        file.sync_all()
    }
    impl Marker {
        pub(in crate::integration) fn complete(mut self) -> io::Result<()> {
            update(&mut self.0, READY)
        }
    }
    pub(in crate::integration) fn acquire(path: &Path) -> io::Result<Marker> {
        match crate::platform::create_config_temporary(path, true) {
            Ok(mut initial) => {
                initial.write_all(READY)?;
                initial.sync_all()?;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600
        {
            return Err(io::Error::other("Codex marker must be an owner-only, single-link regular file owned by the current user"));
        }
        file.try_lock().map_err(io::Error::other)?;
        let mut contents = Vec::new();
        (&mut file).take(1024).read_to_end(&mut contents)?;
        if contents != READY {
            return Err(io::Error::other("Codex install marker is incomplete or unrecognized; inspect it and backups before retrying"));
        }
        update(&mut file, ACTIVE)?;
        Ok(Marker(file))
    }
}

#[cfg(unix)]
pub(super) use unix::acquire;
