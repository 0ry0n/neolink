use std::path::PathBuf;
use structopt::{clap::AppSettings, StructOpt};

/// A standards-compliant bridge to Reolink IP cameras
#[derive(StructOpt, Debug)]
#[structopt(
    name = "neolink",
    setting(AppSettings::ArgRequiredElseHelp),
    setting(AppSettings::UnifiedHelpMessage)
)]
pub struct Opt {
    #[structopt(short, long, global(true), parse(from_os_str))]
    pub config: Option<PathBuf>,
    #[structopt(subcommand)]
    pub cmd: Option<Command>,
}

#[derive(StructOpt, Debug)]
pub enum Command {
    Rtsp(super::rtsp::Opt),
    #[cfg(target_os = "linux")]
    V4l(super::v4l::Opt),
    StatusLight(super::statusled::Opt),
    Reboot(super::reboot::Opt),
    Talk(super::talk::Opt),
}
