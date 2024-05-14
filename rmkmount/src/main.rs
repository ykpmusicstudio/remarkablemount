use clap::{Parser, Subcommand};

use log::{debug, error, info, trace, warn, LevelFilter};
use std::path::Path;

/// Remarkable tablet fuse driver
#[derive(Parser, Debug)]
#[command(version,about,long_about=None)]
struct Args {
    /// remarkable tablet IP address (defaults to 10.x.x.x)
    #[arg(short, long, default_value = "10.11.99.1")]
    address: String,
    /// port number for ssh to remarkable tablet
    #[arg(short, long, default_value = "22")]
    port: Option<u16>,
    /// username
    #[arg(short, long, default_value = "root")]
    username: Option<String>,
    /// hostname and user login as <[USER@]HOST[:PORT]>
    #[arg(long, default_value = "root@10.11.99.1:22")]
    host: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List identities
    Identities {},
    /// Mount remarkable tablet documents
    Mount {
        /// Mount point for documents
        #[arg(short, long)]
        mountpoint: String,
    },
    /// Unmount remarkable tablet documents if previously mounted
    Umount {},
}

// TODO remove password !!
const RK_PWD: &str = "i7GHdeZBqn";
// TODO handle Rk root path
const RK_ROOTPATH: &str = "/home/root/.local/share/remarkable/xochitl/";

fn mount_rkfs(mountpoint: &str, addr: &str, port: u16, user: &str) {
    info!("Mounting to {mountpoint} from {user}@{addr}");
    let _rfs = sftp_rkfs::RemarkableFsBuilder::new()
        .mountpoint(mountpoint)
        .host(addr)
        .port(port)
        .user(user)
        .password(RK_PWD)
        .document_root(RK_ROOTPATH)
        .build()
        .expect("Failed to build RemarkableFs structure");
    _rfs.mount()
        .expect("Mounting RemarkableFs encountered an unexpected error");
}

fn main() {
    simple_logger::init_with_level(log::Level::Trace).unwrap();

    let args = Args::parse();
    // match the requested command
    match &args.command {
        Commands::Identities {} => {
            println!("Available identities: ");
        }
        Commands::Mount { mountpoint } => {
            if let Some(usr) = args.username {
                mount_rkfs(mountpoint, &args.address, args.port.unwrap_or(22), &usr);
            }
        }
        Commands::Umount {} => {
            println!("Umounting");
        }
    }
}
