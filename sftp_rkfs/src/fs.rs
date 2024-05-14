use super::RemarkableFsBuilder;
use crate::nodes::{FuserChild, Node};
use crate::sshutils::{SshFileStat, SshWrapper};
use crate::RemarkableError;
use log::{debug, error, info, warn};
use std::borrow::{Borrow, BorrowMut};
use std::ops::Deref;
use std::path::PathBuf;
use std::time::Duration;
use std::usize;
use std::{cell::Ref, cell::RefCell, collections::HashMap};

impl From<&Node> for fuser::FileAttr {
    fn from(node: &Node) -> Self {
        fuser::FileAttr {
            ino: node.get_ino() as u64,
            size: node.get_size(),
            blocks: (node.get_size() + RemarkableFsBuilder::FB_BLOCK_SIZE as u64 - 1)
                / RemarkableFsBuilder::FB_BLOCK_SIZE as u64,
            atime: node.get_atime(),
            mtime: node.get_mtime(),
            ctime: node.get_ctime(),
            crtime: node.get_ctime(), //SystemTime::UNIX_EPOCH,
            kind: node.get_kind_for_fuser(),
            perm: node.get_perm(),
            nlink: node.get_links(),
            uid: node.get_uid(),
            gid: node.get_gid(),
            blksize: RemarkableFsBuilder::FB_BLOCK_SIZE,
            rdev: 0,
            flags: 0,
        }
    }
}

pub struct RemarkableFs {
    session: SshWrapper,
    document_root: PathBuf,
    mount_point: PathBuf,
    nodes: Vec<RefCell<Node>>,
    uid_map: HashMap<String, usize>,
}

/// private funcs and consts
impl RemarkableFs {
    /// Main assuption : all metadata files are under remarkable root folder
    /// So stripping the filename gives the uid
    /// At this point, an attempt to load node's metadata will be performed
    fn add_or_update_node_from_metadata(
        &mut self,
        parent_ino: usize,
        filestat: &mut SshFileStat,
    ) -> Result<&RefCell<Node>, RemarkableError> {
        let uid = filestat.unique_id().to_owned();
        if let Some(&node_id) = self.uid_map.get(&uid) {
            debug!("node {uid} exists : {node_id}");
            let node = self.get_node(node_id).unwrap();
            if node.borrow().needs_updating(filestat) {
                info!("refreshing metadata for node {node_id} : {filestat:?}");
                let strmetadata = self.session.read_as_string(filestat.get_path())?;
                let _res = node
                    .borrow_mut()
                    .update_metadata(filestat, parent_ino, &strmetadata)?;
            } else {
                debug!("unchanged node {node_id}")
            }
            Ok(node)
        } else {
            let nodeid = self.nodes.len();
            debug!("adding node with metadata {nodeid} : {filestat:?}");
            let strmetadata = self.session.read_as_string(filestat.get_path())?;
            let mut node = Node::from_metadata(nodeid, parent_ino, filestat, &strmetadata)?;
            if node.borrow().is_document() {
                let content_path = node.borrow().get_content_path(&self.document_root);
                //PathBuf::new();
                //                content_path.push(&self.document_root);
                //                content_path.push(node.borrow().get_unique());
                //                content_path.set_extension(Self::CONTENT_EXTENSION);
                info!("adding content for node {nodeid} : {content_path:?}");
                let _res = self.session.read_as_string(&content_path)?;
                node.borrow_mut().update_content(&_res)?;
                if let Some(target) = node.borrow().get_target_file_path(&self.document_root) {
                    debug!("stat content for size {target:?}");
                    // stat file for size
                    let mut fstat = self.session.stat(target.to_str().unwrap_or(""))?;
                    node.borrow_mut().update_target_fstat(&mut fstat);
                }
            }
            self.uid_map.insert(uid, nodeid);
            self.nodes.push(RefCell::new(node));
            Ok(&self.nodes[nodeid])
        }
    }

    /// Looks up parent node children for a specific file name
    fn lookup_node(
        &self,
        parent_ino: usize,
        name: &str,
    ) -> Result<Option<&RefCell<Node>>, RemarkableError> {
        if parent_ino == Node::ROOT_NODE_INO && name == Node::TRASH_NODE_PATH {
            Ok(Some(&self.nodes[Node::TRASH_NODE_INO]))
        } else if let Some(root_node) = self.get_node(parent_ino) {
            // get all child nodes
            let children = self.get_nodes(&root_node.borrow().get_children_ino());
            let found = children
                .into_iter()
                .flatten() //.filter(|n| n.is_some())
                //.map(|n| n.unwrap())
                .find(|&n| n.borrow().get_visible_name().as_os_str() == name);
            debug!("{name} in {parent_ino} gives empty?={}", found.is_none());
            Ok(found)
        } else {
            warn!("node {name} not found in inode={parent_ino}");
            Err(RemarkableError::NodeNotFound(parent_ino))
        }
    }

    /// get all children of nodeid node and create them with metadata if needed
    fn node_readdir(
        &mut self,
        node_ino: usize,
        ioffset: usize,
    ) -> Result<Ref<[FuserChild]>, RemarkableError> {
        if ioffset == 0 {
            let mut read_children = self.get_metadata_files_by_parent(node_ino)?;
            let mut children = Node::root_children(node_ino);
            // add root children and fuse with `children` when relevant
            children.append(&mut read_children);
            // check if nodes are known in nodes hashmap
            let mut readdir_nodes = children
                .iter_mut()
                .enumerate()
                .filter_map(|(o, f)| {
                    if let Ok(node) = self.add_or_update_node_from_metadata(node_ino, f) {
                        Some(FuserChild::new(
                            node.borrow().get_ino(),
                            o,
                            node.borrow().get_kind_for_fuser(), //.clone(),
                            node.borrow().get_visible_name(),
                        ))
                    } else {
                        warn!("node index {o}:{f:?} was not Ok");
                        None
                    }
                })
                .collect::<Vec<_>>();
            debug!("readdir got {} entries", readdir_nodes.len());
            // update child list
            if let Some(rootnode) = self.get_node(node_ino) {
                rootnode.borrow_mut().set_children(&mut readdir_nodes);
            }
            //            Ok(readdir_nodes.clone())
        }

        if let Some(root_node) = self.get_node(node_ino) {
            let ret = Ref::map(root_node.borrow(), |r| r.get_children(ioffset));
            Ok(ret)
        } else {
            Err(RemarkableError::NodeNotFound(node_ino))
        }
    }

    // TODO : replace Option by Result
    /// Gets RefCell to a node whose inode identifier is `ino`
    fn get_node(&self, ino: usize) -> Option<&RefCell<Node>> {
        if (ino < self.nodes.len()) && (ino > Node::INVALID_NODE_INO) {
            Some(&self.nodes[ino])
        } else {
            error!("Node {ino} not found or invalid !");
            None
        }
    }

    /// Get the remarkable unique id from inode identifer `ino`
    fn get_node_unique_id(&self, ino: usize) -> Option<String> {
        if ino == Node::ROOT_NODE_INO {
            Some(Node::ROOT_NODE_UID.to_string())
        } else {
            self.get_node(ino)
                .map(|n| n.borrow().get_unique().to_owned())
        }
    }

    /// Gets a vector of nodes from a vector of inode indentifiers
    // TODO : replace handling get_node return from Option to Error ?
    fn get_nodes(&self, inos: &[usize]) -> Vec<Option<&RefCell<Node>>> {
        inos.iter().map(|&i| self.get_node(i)).collect()
    }

    /// reads data from a node
    fn node_read_ofs_size(
        &self,
        node_ino: usize,
        offset: u64,
        size: u32,
    ) -> Result<Vec<u8>, RemarkableError> {
        if let Some(node) = self.get_node(node_ino) {
            if let Some(fpath) = node.borrow().get_target_file_path(&self.document_root) {
                let sz = node.borrow().get_size() - offset;
                let readsz = std::cmp::min(sz, size as u64);

                debug!(
                    "read request for {node_ino} : ofs={offset} reqsz = {size}, gotsz ={readsz} on {fpath:?}"
                );

                let mut buf = vec![0; readsz as usize];

                match self.session.read_as_bytes(&fpath, offset, readsz, &mut buf) {
                    Ok(_) => Ok(buf),
                    Err(e) => Err(e),
                }
            } else {
                Err(RemarkableError::NodeNotFound(node_ino))
            }
        } else {
            Err(RemarkableError::NodeNotFound(node_ino))
        }
    }

    /// get fuse options
    fn options(&self) -> Vec<fuser::MountOption> {
        vec![
            fuser::MountOption::RO,
            fuser::MountOption::FSName("Remarkable".to_string()),
        ]
    }
}

/// basic fuser trait implementations
impl fuser::Filesystem for RemarkableFs {
    /// initialize remarkable filesystem
    fn init(
        &mut self,
        _req: &fuser::Request<'_>,
        _config: &mut fuser::KernelConfig,
    ) -> Result<(), libc::c_int> {
        if self.init_root().is_err() {
            error!("Error while initializing fs root");
            Err(libc::ENOSYS)
        } else {
            info!("Initialization done");
            Ok(())
        }
    }

    /*
    fn opendir(&mut self, _req: &fuser::Request, _ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        info!("opendir request {:?}", _req);
        //reply.opened(_ino, 0);
    }*/

    fn getattr(&mut self, _req: &fuser::Request<'_>, ino: u64, reply: fuser::ReplyAttr) {
        //info!("getattr request {:?}", _req);
        if let Some(node) = self.get_node(ino as usize) {
            let fileattr: fuser::FileAttr = node.borrow().deref().into();
            info!("node {ino} : {fileattr:?}");
            reply.attr(&Duration::new(0, 0), &fileattr);
        } else {
            error!("node {ino} not found");
            reply.error(libc::ENOENT)
        }
    }

    fn lookup(
        &mut self,
        _req: &fuser::Request<'_>,
        parent: u64,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        //info!("lookup request {:?}", _req);
        if let Some(nodestr) = name.to_str() {
            match self.lookup_node(parent as usize, nodestr) {
                Ok(res) => {
                    if let Some(node) = res {
                        let fileattr: fuser::FileAttr = node.borrow().deref().into();
                        info!("found node {nodestr}: {fileattr:?}");
                        reply.entry(&Duration::new(0, 0), &fileattr, 0);
                    } else {
                        // not found
                        error!("node {nodestr} not found in parent {parent}");
                        reply.error(libc::ENOENT)
                    }
                }
                Err(e) => {
                    error!("got error {e:?}");
                    // root node does not exist or general error (ssh channel?)
                    reply.error(libc::ENOSYS);
                }
            };
        } else {
            error!("provided name could not be converted to string");
            reply.error(libc::EINVAL);
        }
    }

    fn readdir(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        //info!("readdir request {:?}", _req);
        match self.node_readdir(ino as usize, offset as usize) {
            Ok(res) => {
                let _ = res.iter().try_for_each(|v| {
                    let (s_ino, s_offs, s_knd, s_nm) = (v.0, v.1, v.2, &v.3);
                    info!("adding {s_ino} {s_offs} {s_knd:?} {:?}", s_nm);
                    if reply.add(s_ino as u64, s_offs as i64 + 1, s_knd, s_nm.as_os_str()) {
                        Err(())
                    } else {
                        Ok(())
                    }
                });
                debug!("READDIR reply {reply:?}");
                reply.ok();
            }
            Err(e) => {
                error!("got error {e:?}");
                reply.error(libc::ENOENT);
            }
        };
    }

    fn open(&mut self, _req: &fuser::Request, _ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        if let Some(node) = self.get_node(_ino as usize) {
            match node.borrow_mut().open() {
                Ok(v) => {
                    reply.opened(v, 0);
                    debug!("open request for {_ino} = {v}");
                }
                Err(RemarkableError::NodeIoError(v)) => {
                    reply.error(v);
                    error!("open failed for {_ino} with io error {v}");
                }
                Err(e) => {
                    reply.error(libc::EBADFD);
                    error!("open failed for {_ino} with io error {e}");
                }
            }
        } else {
            error!("open failed : {_ino} not found");
            reply.error(libc::EBADFD);
        }
    }

    fn read(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        flags: i32,
        lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        debug!("read request for {ino} : {offset} {size} {fh} {flags} {lock_owner:?}");
        if size > 0 || offset < 0 {
            match self.node_read_ofs_size(ino as usize, offset as u64, size) {
                Ok(buffer) => {
                    reply.data(&buffer);
                }
                Err(RemarkableError::NodeIoError(e)) => {
                    reply.error(e);
                    error!("read failed for {ino} : {e}");
                }
                Err(e) => {
                    reply.error(libc::EBADFD);
                    error!("read failed for {ino} : {e:?}");
                }
            }
        } else {
            error!("read failed for {ino} : invalid size {size}");
            reply.error(libc::EINVAL);
        }
    }

    fn release(
        &mut self,
        _req: &fuser::Request<'_>,
        _ino: u64,
        _fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        if let Some(node) = self.get_node(_ino as usize) {
            match node.borrow_mut().close() {
                Ok(v) => {
                    reply.ok();
                    debug!("release request for {_ino} = {v}");
                }
                Err(RemarkableError::NodeIoError(v)) => {
                    reply.error(v);
                    error!("release failed for {_ino} with io error {v}");
                }
                Err(e) => {
                    reply.error(libc::EBADFD);
                    error!("open failed for {_ino} with io error {e}");
                }
            }
        } else {
            error!("open failed : {_ino} not found");
            reply.error(libc::EBADFD);
        }
    }
}

/// Public implementations
impl RemarkableFs {
    /// Creates a new RemarkableFs struct from a connected ssh wrapper, a path to remarkable
    /// document root and a desitnation mount_point for fuser filesystem
    pub fn new(session: SshWrapper, mount_point: PathBuf, document_root: PathBuf) -> Self {
        Self {
            session,
            document_root,
            mount_point,
            nodes: vec![],
            uid_map: HashMap::new(),
        }
    }

    /// initialize basic root nodes (Invalid node(0), Root(ROOT_NODE_UID) and Trash)
    pub fn init_root(&mut self) -> Result<(), RemarkableError> {
        // push invalid node at ino = 0
        self.nodes.push(RefCell::new(Node::new(
            Node::INVALID_NODE_INO,
            SshFileStat::default(),
        )));
        // add empty root node
        let root_node = RefCell::new(Node::new_root());
        /* connect trash_node as a child of root_node
        let childs = vec![FuserChild(
            Node::TRASH_NODE_INO,
            1,
            fuser::FileType::Directory,
            OsString::from(Node::TRASH_NODE_PATH),
        )];
        root_node.borrow_mut().set_children(&childs);*/
        self.nodes.push(root_node);
        self.uid_map
            .insert(Node::ROOT_NODE_UID.to_string(), Node::ROOT_NODE_INO);
        // add empty trash node
        let trash_node = RefCell::new(Node::new_trash());
        trash_node.borrow_mut().set_parent(Node::ROOT_NODE_INO);
        self.nodes.push(trash_node);
        self.uid_map
            .insert(Node::TRASH_NODE_UID.to_string(), Node::TRASH_NODE_INO);
        // TODO stat root
        // let root_metadata = self.get_metadata_files_by_parent("")?;
        //
        //todo!("Build root node and trash node");
        Ok(())
    }

    /// Queries the remarkable tablet for all children of a specific parent node
    pub fn get_metadata_files_by_parent(
        &self,
        parent_ino: usize,
    ) -> Result<Vec<SshFileStat>, RemarkableError> {
        if let Some(n_id) = self.get_node_unique_id(parent_ino) {
            if let Some(path) = self.document_root.to_str() {
                let grepcmd = format!(r#"grep -l \"parent\":\ \"{n_id}\" {path}*.metadata"#);
                debug!("{grepcmd}");
                let cmd_res = self.session.execute_cmd(&grepcmd)?;
                let file_list = cmd_res
                    .split('\n')
                    //            .map(|s| format!("{s}.metadata"))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>();
                Ok(self.session.stat_files(&file_list)?)
            } else {
                Err(RemarkableError::RkError("invalid document root".into()))
            }
        } else {
            Err(RemarkableError::NodeNotFound(parent_ino))
        }
    }

    /// RemarkableFs is consumed by mount
    pub fn mount(self) -> Result<(), std::io::Error> {
        let mountpoint = &self.mount_point.clone();
        let options = &self.options().clone();
        fuser::mount2(self, mountpoint, options)
    }

    #[cfg(test)]
    /// For tests purposes of node_readir from library main lib.rs
    pub fn pub_readdir(&mut self, ino: usize) -> Result<&[FuserChild], RemarkableError> {
        self.node_readdir(ino, 0)
    }
}
