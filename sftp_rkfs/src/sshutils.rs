use crate::RemarkableError;
use log::{debug, info};
use std::ffi::OsStr;
use std::io::{Read, Seek};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub struct SshWrapper {
    session: ssh2::Session,
}

pub struct SshFileStatBuilder {
    raw_flags: libssh2_sys::LIBSSH2_SFTP_ATTRIBUTES,
}

impl SshFileStatBuilder {
    pub fn new() -> Self {
        Self {
            raw_flags: libssh2_sys::LIBSSH2_SFTP_ATTRIBUTES {
                flags: 0,
                filesize: 0,
                uid: 0,
                gid: 0,
                permissions: 0o000,
                atime: 0,
                mtime: 0,
            },
        }
    }

    pub fn filesize(mut self, sz: u64) -> Self {
        self.raw_flags.filesize = sz;
        self.raw_flags.flags |= libssh2_sys::LIBSSH2_SFTP_ATTR_SIZE;
        self
    }

    pub fn uid(mut self, uid: u64) -> Self {
        self.raw_flags.uid = uid;
        self.raw_flags.flags |= libssh2_sys::LIBSSH2_SFTP_ATTR_UIDGID;
        self
    }

    pub fn gid(mut self, gid: u64) -> Self {
        self.raw_flags.gid = gid;
        self.raw_flags.flags |= libssh2_sys::LIBSSH2_SFTP_ATTR_UIDGID;
        self
    }

    pub fn perm(mut self, perm: u64) -> Self {
        self.raw_flags.permissions = perm;
        self.raw_flags.flags |= libssh2_sys::LIBSSH2_SFTP_ATTR_PERMISSIONS;
        self
    }

    pub fn mtime(mut self, mtime: u64) -> Self {
        self.raw_flags.mtime = mtime;
        self.raw_flags.flags |= libssh2_sys::LIBSSH2_SFTP_ATTR_ACMODTIME;
        self
    }

    pub fn atime(mut self, atime: u64) -> Self {
        self.raw_flags.atime = atime;
        self.raw_flags.flags |= libssh2_sys::LIBSSH2_SFTP_ATTR_ACMODTIME;
        self
    }

    pub fn set_dir(mut self) -> Self {
        self.raw_flags.permissions |= libssh2_sys::LIBSSH2_SFTP_S_IFDIR;
        self.raw_flags.flags |= libssh2_sys::LIBSSH2_SFTP_ATTR_PERMISSIONS;
        self
    }

    pub fn set_reg(mut self) -> Self {
        self.raw_flags.permissions |= libssh2_sys::LIBSSH2_SFTP_S_IFREG;
        self.raw_flags.flags |= libssh2_sys::LIBSSH2_SFTP_ATTR_PERMISSIONS;
        self
    }

    pub fn build(self) -> ssh2::FileStat {
        ssh2::FileStat::from_raw(&self.raw_flags)
    }
}

#[derive(Debug)]
pub struct SshFileStat(PathBuf, ssh2::FileStat);

impl Default for SshFileStat {
    fn default() -> Self {
        Self(
            PathBuf::from(Self::INVALID_UID),
            SshFileStatBuilder::new().build(),
            /*
            ssh2::FileStat::from_raw(&libssh2_sys::LIBSSH2_SFTP_ATTRIBUTES {
                flags: 0,
                filesize: 0,
                uid: 0,
                gid: 0,
                permissions: 0o444,
                atime: 0,
                mtime: 0,
            }),*/
        )
    }
}
impl SshFileStat {
    pub const INVALID_UID: &'static str = "INVALID-UID-0000";

    pub fn build_from_special_path(special: &str) -> Self {
        let new_stat = SshFileStatBuilder::new()
            .atime(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )
            .mtime(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            )
            .perm(0o444)
            .uid(0)
            .gid(0)
            .filesize(0)
            .set_dir()
            .build();
        Self(PathBuf::from(special), new_stat)
    }
    /// convert ssh2::FileStat times to values compatible with fuser::FileAttr
    pub fn get_time_from(fstat_time: Option<u64>) -> SystemTime {
        SystemTime::checked_add(
            &SystemTime::UNIX_EPOCH,
            Duration::from_secs(fstat_time.unwrap_or(0)),
        )
        .unwrap_or(SystemTime::UNIX_EPOCH)
    }

    pub fn get_path(&self) -> &PathBuf {
        &self.0
    }

    pub fn unique_id(&self) -> &str {
        if let Some(fstem) = self.0.file_stem() {
            fstem.to_str().unwrap_or(Self::INVALID_UID)
        } else {
            Self::INVALID_UID
        }
    }

    pub fn is_file(&self) -> bool {
        self.0.is_file()
    }

    pub fn is_metadata(&self) -> bool {
        self.0.extension() == Some(OsStr::new("metadata"))
    }

    pub fn is_contents(&self) -> bool {
        self.0.extension() == Some(OsStr::new("contents"))
    }

    pub fn perm(&self) -> u16 {
        if let Some(perm) = self.1.perm {
            (perm & 0o777) as u16
        } else {
            0o755
        }
    }

    pub fn uid(&self) -> Option<u32> {
        self.1.uid
    }

    pub fn gid(&self) -> Option<u32> {
        self.1.gid
    }

    pub fn size(&self) -> Option<u64> {
        self.1.size
    }

    pub fn atime(&self) -> Option<u64> {
        self.1.atime
    }

    pub fn mtime(&self) -> Option<u64> {
        self.1.mtime
    }

    pub fn is_more_recent_than(&self, new: &Self) -> bool {
        let old = &self.1;
        let new = &new.1;
        old.mtime.unwrap_or(0) > new.mtime.unwrap_or(0)
    }
}

impl SshWrapper {
    pub fn new() -> Result<Self, RemarkableError> {
        let new_session = ssh2::Session::new()?;
        Ok(Self {
            session: new_session,
        })
    }

    /// Connect the TCP Stream to provided host address and add it to the session
    pub fn connect(&mut self, host_address: &str) -> Result<&Self, RemarkableError> {
        match TcpStream::connect(host_address) {
            Err(_) => Err(RemarkableError::Ssh2Error(ssh2::Error::from_errno(
                ssh2::ErrorCode::Session(libssh2_sys::LIBSSH2_ERROR_SOCKET_TIMEOUT),
            ))),
            Ok(tcp) => {
                self.session.set_tcp_stream(tcp);
                match self.session.handshake() {
                    Ok(_) => Ok(self),
                    Err(e) => Err(RemarkableError::Ssh2Error(e)),
                }
            }
        }
    }

    /// Authenticates with username and password
    pub fn authenticate(&self, username: &str, password: &str) -> Result<&Self, RemarkableError> {
        self.session.userauth_password(username, password)?;
        Ok(self)
    }

    /// Executes a command and returns the result as a string
    pub fn execute_cmd(&self, command: &str) -> Result<String, RemarkableError> {
        let mut channel = self.session.channel_session()?;
        channel.exec(command)?;
        let mut s = String::new();
        channel.read_to_string(&mut s)?;
        Ok(s)
    }

    /// Reads the given path
    pub fn stat(&self, path: &str) -> Result<SshFileStat, RemarkableError> {
        let my_sftp = self.session.sftp()?;
        let fstat = my_sftp.stat(Path::new(path))?;
        debug!("{path} {fstat:?}");
        Ok(SshFileStat(PathBuf::from(path), fstat))
    }
    /// Reads contents of the folder at given Path
    /// and returns a Vec of (Path, FileStat) sorted by filename
    pub fn stat_files(&self, files: &[&str]) -> Result<Vec<SshFileStat>, RemarkableError> {
//        let my_sftp = self.session.sftp()?;
        let result = files
            .iter()
            .map(|f| 
/*
            {
                let fstat = my_sftp.stat(Path::new(f));
                debug!("{f} {fstat:?}");
                match fstat {
                    Ok(fs) => Ok(SshFileStat(PathBuf::from(f), fs)),
                    Err(e) => Err(e),
                }

            }*/
                self.stat(f)
            )
            .collect();
        debug!("{result:?}");
        match result {
            Ok(x) => Ok(x),
            Err(x) => Err(x), //RemarkableError::Ssh2Error(x)),
        }
    }

    /// Reads contents of the folder at given Path
    /// and returns a Vec of (Path, FileStat) sorted by filename
    pub fn readdir(&self, path: &Path) -> Result<Vec<SshFileStat>, RemarkableError> {
        let mut result = self.session.sftp()?.readdir(path)?;
        result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        Ok(result.into_iter().map(|x| SshFileStat(x.0, x.1)).collect())
    }

    /// Reads file content as string (for json parsing)
    pub fn read_as_string(&self, path: &Path) -> Result<String, RemarkableError> {
        //Box<dyn Error>> {
        let mut fopen = self.session.sftp()?.open(path)?;
        let mut str_result = String::new();
        /*
        let szbyte = fopen.stat()?.size;
        match szbyte {
            Some(sz) => {
                str_result.reserve(sz as usize);
                unsafe {
                    let mut str_buf = str_result.as_bytes_mut();
                    //fopen.read_to_string(&mut str_result)?;
                    fopen.read(str_buf, szbyte);
                }
                Ok(str_result)
            }
            None => Err("Cannot stat file".into()),
        }*/
        fopen.read_to_string(&mut str_result)?;
        Ok(str_result)
    }

    /// Reads a chunk of data with given size & offset from PathBuf
    pub fn read_as_bytes(
        &self,
        path: &Path,
        offset: u64,
        size: u64,
        buf: &mut [u8],
    ) -> Result<u64, RemarkableError> {
        let mut fopen = self.session.sftp()?.open(path)?;
        if let Ok(offset) = fopen.seek(std::io::SeekFrom::Start(offset)) {
            fopen.read_exact(buf)?;
            Ok(size)
        } else {
            Err(RemarkableError::NodeIoError(libc::EOF))
        }
    }
}
