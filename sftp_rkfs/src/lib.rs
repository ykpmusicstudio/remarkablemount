use crate::fs::RemarkableFs;
use crate::sshutils::SshWrapper;
use thiserror::Error;

#[cfg(test)]
use std::sync::Once;

pub mod fs;
mod nodes;
mod sshutils;

#[derive(Debug, Error)]
pub enum RemarkableError {
    #[error(transparent)]
    Ssh2Error(#[from] ssh2::Error),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    JsonError(#[from] serde_json::Error),
    #[error("Duplicated node")]
    NodeDuplicated,
    #[error("Node not found {0}")]
    NodeNotFound(usize),
    #[error("Node io error {0}")]
    NodeIoError(libc::c_int),
    #[error("RemarkableFs Error : {0}")]
    RkError(String),
}

pub struct RemarkableFsBuilder {
    _host: Option<String>,
    _port: Option<u16>,
    _user: Option<String>,
    _password: Option<String>,
    _mountpoint: Option<std::path::PathBuf>,
    _document_root: Option<std::path::PathBuf>,
}

impl RemarkableFsBuilder {
    const RK_PWD: &'static str = "xxx";
    const RK_USR: &'static str = "root";
    const RK_ADDRESS: &'static str = "10.11.99.1";
    const RK_ROOTPATH: &'static str = "/home/root/.local/share/remarkable/xochitl/";
    const RK_PORT: u16 = 22;
    const FB_BLOCK_SIZE: u32 = 512;

    pub fn new() -> Self {
        Self {
            _mountpoint: None,
            _document_root: None,
            _host: None,
            _port: None,
            _user: None,
            _password: None,
        }
    }

    pub fn mountpoint(mut self, mountpoint: &str) -> Self {
        self._mountpoint = Some(std::path::PathBuf::from(mountpoint));
        self
    }

    pub fn host(mut self, host: &str) -> Self {
        self._host = Some(host.to_owned());
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self._port = Some(port);
        self
    }

    pub fn user(mut self, user: &str) -> Self {
        self._user = Some(user.to_owned());
        self
    }

    pub fn password(mut self, password: &str) -> Self {
        self._password = Some(password.to_owned());
        self
    }

    /// sets document root from povided &str path:
    pub fn document_root(mut self, path: &str) -> Self {
        self._document_root = Some(std::path::PathBuf::from(path));
        self
    }

    /// builds a new RemarkableF struct creates the underlying ssh2 session
    /// Builder is consumed after this step
    pub fn build(self) -> Result<RemarkableFs, RemarkableError> {
        let mut session = SshWrapper::new()?;

        let host_addr = format!(
            "{}:{}",
            self._host
                .unwrap_or(RemarkableFsBuilder::RK_ADDRESS.to_string()),
            self._port.unwrap_or(RemarkableFsBuilder::RK_PORT)
        );
        session.connect(&host_addr)?.authenticate(
            &self
                ._user
                .unwrap_or(RemarkableFsBuilder::RK_USR.to_string()),
            &self
                ._password
                .unwrap_or(RemarkableFsBuilder::RK_PWD.to_string()),
        )?;
        if let Some(mountpoint) = &self._mountpoint {
            Ok(RemarkableFs::new(
                session,
                std::path::PathBuf::from(mountpoint),
                self._document_root
                    .unwrap_or(RemarkableFsBuilder::RK_ROOTPATH.into()),
            ))
        } else {
            Err(RemarkableError::RkError(
                "Mountpoint not provided".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static INIT: Once = Once::new();

    const TEST_MOUNTPOINT: &'static str = "/home/pelleter/reMarkable/fs";
    // put here ssh password of tested device
    const TEST_PASSWORD: &'static str = "XXXXXXXX";

    fn init() {
        INIT.call_once(|| simple_logger::init_with_level(log::Level::Trace).unwrap());
    }

    #[test]
    fn test_remarkablefs_build_default() {
        init();
        let _rb = RemarkableFsBuilder::new().build();
        assert!(
            _rb.is_err(),
            "Error while building RemarkableFs structure: should not allow empty mountpoint."
        )
    }

    #[test]
    fn test_remarkablefs_build_with_all_and_port() {
        init();
        let _rb = RemarkableFsBuilder::new()
            .mountpoint(TEST_MOUNTPOINT)
            .host(RemarkableFsBuilder::RK_ADDRESS)
            .port(22)
            .user(RemarkableFsBuilder::RK_USR)
            .password(TEST_PASSWORD)
            .document_root(RemarkableFsBuilder::RK_ROOTPATH)
            .build();
        assert!(
            _rb.is_ok(),
            "Error while building RemarkableFs structure with parameters."
        )
    }
    #[test]
    fn test_remarkablefs_build_with_all_and_host() {
        init();
        let _rb = RemarkableFsBuilder::new()
            .mountpoint(TEST_MOUNTPOINT)
            .host(RemarkableFsBuilder::RK_ADDRESS)
            .user(RemarkableFsBuilder::RK_USR)
            .password(RemarkableFsBuilder::RK_PWD)
            .document_root(RemarkableFsBuilder::RK_ROOTPATH)
            .build();
        assert!(
            _rb.is_ok(),
            "Error while building RemarkableFs structure with parameters."
        )
    }

    #[test]
    fn test_connect_and_readdir() {
        init();
        let mut _rb = RemarkableFsBuilder::new()
            .mountpoint(TEST_MOUNTPOINT)
            .host(RemarkableFsBuilder::RK_ADDRESS)
            .user(RemarkableFsBuilder::RK_USR)
            .password(RemarkableFsBuilder::RK_PWD)
            .document_root(RemarkableFsBuilder::RK_ROOTPATH)
            .build()
            .unwrap();
        _rb.init_root()
            .expect("unable to build fsroot node and trash node");
        assert!(_rb.pub_readdir(fuser::FUSE_ROOT_ID as usize).is_ok());
        //assert!(false, "just to check log output !");
    }

    #[test]
    fn test_mount() {
        init();
        let mut _rb = RemarkableFsBuilder::new()
            .mountpoint(TEST_MOUNTPOINT)
            .host(RemarkableFsBuilder::RK_ADDRESS)
            .user(RemarkableFsBuilder::RK_USR)
            .password(RemarkableFsBuilder::RK_PWD)
            .document_root(RemarkableFsBuilder::RK_ROOTPATH)
            .build()
            .unwrap();
        assert!(_rb.mount().is_ok());
        assert!(false, "just to check log output !");
    }
}
