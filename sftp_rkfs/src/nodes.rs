use crate::sshutils::SshFileStat;
use crate::RemarkableError;

use log::{debug, error, warn};
use serde::Deserialize;
use serde_with::{serde_as, DisplayFromStr};
use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Deserialize, Debug, Clone)]
pub enum RkNodeType {
    CollectionType,
    DocumentType,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
enum RkOrientation {
    Portrait,
    Landscape,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
enum RkFileType {
    EPUB,
    PDF,
    Notebook,
    #[serde(rename = "")]
    Lines,
}

#[derive(Deserialize, Debug)]
struct RkTimestamp {
    timestamp: String,
    value: serde_json::Value,
}

/// structure containing RkNode metadata
#[serde_as]
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RkMetadata {
    deleted: Option<bool>,
    #[serde_as(as = "DisplayFromStr")]
    last_modified: u64,
    #[serde_as(as = "Option<DisplayFromStr>")]
    created_time: Option<u64>,
    metadatamodified: Option<bool>,
    modified: Option<bool>,
    parent: String,
    pinned: bool,
    synced: Option<bool>,
    type_: RkNodeType,
    #[serde(default = "RkMetadata::default_version")]
    version: i32,
    visible_name: String,
}

impl RkMetadata {
    fn default_version() -> i32 {
        0
    }

    fn from_str(visible_name: &str) -> Self {
        Self {
            deleted: None,
            last_modified: 0,
            created_time: None,
            metadatamodified: None,
            modified: None,
            parent: String::new(),
            pinned: false,
            synced: None,
            type_: RkNodeType::CollectionType,
            version: 0,
            visible_name: String::from(visible_name),
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RkPage {
    id: String,
    idx: RkTimestamp,
    template: RkTimestamp,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RkCPages {
    last_opened: RkTimestamp,
    original: RkTimestamp,
    pages: Vec<RkPage>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum RkContentChoice {
    HasSome(RkContents),
    Emtpy {},
}
impl RkContentChoice {
    pub fn from_str(contents: &str) -> Result<Self, RemarkableError> {
        let ocontents: RkContentChoice = serde_json::from_str(contents)?;
        match ocontents {
            RkContentChoice::HasSome(_) => {
                /*trace!(
                    "uid {:?} = {:?} has content {:?}",
                    self.get_uid(),
                    self.get_visible_name(),
                    c
                );*/
            }
            RkContentChoice::Emtpy {} => {
                warn!("uid {contents:?} is parsed as empty");
            }
        }
        Ok(ocontents)
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RkContents {
    c_pages: Option<RkCPages>,
    pages: Option<Vec<String>>,
    cover_page_number: Option<i64>,
    custom_zoom_center_x: Option<i64>,
    custom_zoom_center_y: Option<i64>,
    custom_zoom_orientation: Option<RkOrientation>,
    custom_zoom_page_height: Option<i16>,
    custom_zoom_page_width: Option<i16>,
    custom_zoom_scale: Option<i16>,
    file_type: RkFileType,
    font_name: String,
    line_height: i16,
    margins: i16,
    orientation: RkOrientation,
    #[serde(default = "RkContents::default_format_version")]
    format_version: i16,
    page_count: u16,
}

impl RkContents {
    fn default_format_version() -> i16 {
        1
    }
}

#[derive(Debug, Clone)]
pub struct FuserChild(
    pub usize,
    pub usize,
    pub fuser::FileType,
    pub std::ffi::OsString,
);

impl FuserChild {
    pub fn new(ino: usize, size: usize, kind: fuser::FileType, name: PathBuf) -> Self {
        Self(ino, size, kind, name.into())
    }

    pub fn ino(&self) -> usize {
        self.0
    }
}

pub struct Node {
    ino: usize,
    metadata: Option<RkMetadata>,
    content: Option<RkContentChoice>,
    filestat: SshFileStat,
    parent: usize,
    children: Vec<FuserChild>,
    handles: u64,
}

impl Node {
    pub const INVALID_NODE_INO: usize = 0;
    pub const INVALID_NODE_NAME: &'static str = "<Invalid Node>";
    pub const ROOT_NODE_UID: &'static str = "";
    pub const ROOT_NODE_PATH: &'static str = "/";
    pub const ROOT_NODE_INO: usize = fuser::FUSE_ROOT_ID as usize;
    pub const TRASH_NODE_UID: &'static str = ".Trash";
    pub const TRASH_NODE_PATH: &'static str = ".Trash";
    pub const TRASH_NODE_INO: usize = Self::ROOT_NODE_INO + 1;

    const CONTENT_EXTENSION: &'static str = "content";

    pub fn new(ino: usize, filestat: SshFileStat) -> Self {
        Self {
            ino,
            metadata: None,
            content: None,
            filestat,
            parent: 0,
            children: vec![],
            handles: 0,
        }
    }

    pub fn new_root() -> Self {
        Self {
            ino: Self::ROOT_NODE_INO,
            metadata: Some(RkMetadata::from_str(Self::ROOT_NODE_PATH)),
            content: None,
            filestat: SshFileStat::build_from_special_path(Self::ROOT_NODE_UID),
            parent: 0,
            children: vec![],
            handles: 0,
        }
    }

    pub fn new_trash() -> Self {
        Self {
            ino: Self::TRASH_NODE_INO,
            metadata: Some(RkMetadata::from_str(Self::TRASH_NODE_PATH)),
            content: None,
            filestat: SshFileStat::build_from_special_path(Self::TRASH_NODE_UID),
            parent: Self::ROOT_NODE_INO,
            children: vec![],
            handles: 0,
        }
    }

    pub fn from_metadata(
        ino: usize,
        parent: usize,
        filestat: &mut SshFileStat,
        metadata: &str,
    ) -> Result<Self, RemarkableError> {
        match serde_json::from_str(metadata) {
            Ok(rkm) => Ok(Self {
                ino,
                metadata: Some(rkm),
                content: None,
                filestat: std::mem::take(filestat),
                parent,
                children: vec![],
                handles: 0,
            }),
            Err(e) => Err(RemarkableError::JsonError(e)),
        }
    }

    pub fn root_children(_ino: usize) -> Vec<SshFileStat> {
        /*        if ino == Self::ROOT_NODE_INO {
            debug!("this node is Root, adding Trash child");
            vec![SshFileStat::build_from_special_path(Node::TRASH_NODE_PATH)]
        } else {*/
        vec![]
        // }
    }
    /// is this node the root node ?
    pub fn is_root(&self) -> bool {
        self.ino == Self::ROOT_NODE_INO
    }

    /// is this node the trash node ?
    pub fn is_trash(&self) -> bool {
        self.ino == Self::TRASH_NODE_INO
    }

    /// does this node has a content json file ?
    pub fn is_document(&self) -> bool {
        match &self.metadata {
            Some(RkMetadata {
                type_: RkNodeType::DocumentType,
                ..
            }) => true,
            _ => false,
        }
    }

    /// get handle count to current node
    pub fn handles(&self) -> u64 {
        self.handles
    }
    /// acquire an new handle on current node
    pub fn open(&mut self) -> Result<u64, RemarkableError> {
        if self.handles < u64::max_value() {
            self.handles += 1;
            Ok(self.handles)
        } else {
            Err(RemarkableError::NodeIoError(libc::EACCES))
        }
    }
    /// release a handle on current node
    pub fn close(&mut self) -> Result<u64, RemarkableError> {
        if self.handles > 0 {
            self.handles -= 1;
            Ok(self.handles)
        } else {
            Err(RemarkableError::NodeIoError(libc::EINVAL))
        }
    }
    /// gets the number of links to the node
    pub fn get_links(&self) -> u32 {
        if self.get_kind_for_fuser() == fuser::FileType::Directory {
            2
        } else {
            1
        }
    }

    pub fn get_visible_name(&self) -> PathBuf {
        let mut res = PathBuf::from(self.get_basename().unwrap_or(Self::INVALID_NODE_NAME));
        if let Some(ext) = self.get_extension() {
            res.set_extension(ext);
        }
        res
    }

    /// get node base name
    pub fn get_basename(&self) -> Option<&str> {
        match self.ino {
            Self::ROOT_NODE_INO => Some(Self::ROOT_NODE_PATH),
            Self::TRASH_NODE_INO => Some(Self::TRASH_NODE_PATH),
            _ => {
                if let Some(metadata) = &self.metadata {
                    Some(&metadata.visible_name)
                } else {
                    None //Self::INVALID_NODE_NAME
                }
            }
        }
    }

    /// get node extension if any
    pub fn get_extension(&self) -> Option<&str> {
        match &self.content {
            Some(RkContentChoice::HasSome(c)) => match c.file_type {
                RkFileType::PDF => Some("pdf"),
                RkFileType::EPUB => Some("epub"),
                RkFileType::Lines | RkFileType::Notebook => None, //Some("rm"),
            },
            _ => None,
        }
    }

    /// get content json file path
    pub fn get_content_path(&self, document_root: &PathBuf) -> PathBuf {
        let mut res = PathBuf::from(document_root);
        res.push(self.get_unique());
        res.set_extension(Self::CONTENT_EXTENSION);
        res
    }

    /// get content file name for pdf & epub
    pub fn get_target_file_path(&self, document_root: &PathBuf) -> Option<PathBuf> {
        if let Some(ext) = self.get_extension() {
            let mut res = PathBuf::from(document_root);
            res.push(self.get_unique());
            res.set_extension(ext);
            Some(res)
        } else {
            None
        }
    }

    /// get ino
    pub fn get_ino(&self) -> usize {
        self.ino
    }

    pub fn get_unique(&self) -> &str {
        self.filestat.unique_id()
    }

    pub fn get_path(&self) -> &PathBuf {
        self.filestat.get_path()
    }

    /// TODO: return real size from contents !
    pub fn get_size(&self) -> u64 {
        match &self.metadata {
            Some(m) => match m.type_ {
                RkNodeType::DocumentType => {
                    if let Some(RkContentChoice::HasSome(c)) = &self.content {
                        match c.file_type {
                            RkFileType::PDF | RkFileType::EPUB => self.filestat.size().unwrap_or(0),
                            // TODO : implement size or lines files
                            _ => 0,
                        }
                    } else {
                        0
                    }
                }
                _ => self.filestat.size().unwrap_or(0),
            },
            None => 0,
        }
    }

    pub fn get_ctime(&self) -> SystemTime {
        // TODO ctime is taken from metadata
        //todo!("ctime shall be take from metadata?");
        SshFileStat::get_time_from(self.filestat.mtime())
        //SystemTime::UNIX_EPOCH
    }

    pub fn get_atime(&self) -> SystemTime {
        SshFileStat::get_time_from(self.filestat.atime())
    }

    pub fn get_mtime(&self) -> SystemTime {
        SshFileStat::get_time_from(self.filestat.mtime())
    }

    pub fn get_kind(&self) -> Option<RkNodeType> {
        self.metadata.as_ref().map(|m| m.type_.clone())
    }

    pub fn get_kind_for_fuser(&self) -> fuser::FileType {
        match self.get_kind() {
            Some(RkNodeType::DocumentType) => fuser::FileType::RegularFile,
            Some(RkNodeType::CollectionType) => fuser::FileType::Directory,
            None => fuser::FileType::Directory,
        }
    }

    pub fn get_uid(&self) -> u32 {
        self.filestat.uid().unwrap_or(0)
    }

    pub fn get_gid(&self) -> u32 {
        self.filestat.gid().unwrap_or(0)
    }

    pub fn get_perm(&self) -> u16 {
        self.filestat.perm()
    }

    pub fn get_parent(&self) -> usize {
        self.parent
    }

    pub fn set_parent(&mut self, parent: usize) {
        self.parent = parent;
    }

    pub fn get_children(&self, iofs: usize) -> &[FuserChild] {
        &self.children[iofs..]
    }

    pub fn get_children_ino(&self) -> Vec<usize> {
        self.children.iter().map(|c| c.ino()).collect::<Vec<_>>()
    }

    pub fn set_children(&mut self, children: &mut Vec<FuserChild>) {
        /*    let mut all_children = (self.children, children).concat();
        all_children.sort();
        all_children.dedup();
        self.children = all_children;*/
        self.children = std::mem::take(children);
    }

    pub fn needs_updating(&self, newfstat: &SshFileStat) -> bool {
        (!self.is_root())
            && (!self.is_trash())
            && (self.metadata.is_none() || newfstat.is_more_recent_than(&self.filestat))
    }

    pub fn update_metadata(
        &mut self,
        newfstat: &mut SshFileStat,
        parent_ino: usize,
        metadata: &str,
    ) -> Result<&Self, RemarkableError> {
        match serde_json::from_str(metadata) {
            Ok(m) => {
                self.parent = parent_ino;
                self.metadata = Some(m);
                std::mem::swap(&mut self.filestat, newfstat);
                Ok(self)
            }
            Err(e) => {
                error!("invalid metadata: {}", e);
                Err(RemarkableError::JsonError(e))
            }
        }
    }

    pub fn update_content(&mut self, contents: &str) -> Result<&Self, RemarkableError> {
        match serde_json::from_str(contents) {
            Ok(c) => {
                self.content = Some(c);
                Ok(self)
            }
            Err(e) => {
                error!("invalid contents: {}", e);
                Err(RemarkableError::JsonError(e))
            }
        }
    }

    pub fn update_target_fstat(&mut self, filestat: &mut SshFileStat) -> &Self {
        // TODO : FIXME this has impacts on update_metadata test since it relies on filestat !!
        std::mem::swap(&mut self.filestat, filestat);
        self
    }
}
