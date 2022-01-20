///
/// # Neolink V4l
///
/// This module serves the v4l streams for the
/// `neolink v4l` subcommand
///
/// All camera specified in the config.toml will be served
/// over v4l.
///
/// You can view the streams with any v4l compliement program
/// such as ffmpeg, vlc, blue-iris, home-assistant, zone-minder etc.
///
/// `/dev/video0`
///
/// # Usage
///
/// To start the subcommand use the following in a shell.
///
/// ```bash
/// neolink v4l --config=config.toml
/// ```
///
use anyhow::{Context, Result};
use log::*;
use neolink_core::bc_protocol::{BcCamera, Stream};
use neolink_core::Never;
use std::sync::Arc;
use std::time::Duration;

// mod adpcm;
/// The command line parameters for this subcommand
mod cmdline;
/// The errors this subcommand can raise
mod v4lt;

use super::config::{CameraConfig, Config};
use crate::utils::AddressOrUid;
pub(crate) use cmdline::Opt;
use v4lt::{V4ltOutputs, V4lDevice};

/// Entry point for the v4l subcommand
///
/// Opt is the command line options
pub(crate) fn main(_opt: Opt, config: Config) -> Result<()> {
    if config.certificate == None && !config.users.is_empty() {
        warn!(
            "Without a server certificate, usernames and passwords will be exchanged in plaintext!"
        )
    }

    crossbeam::scope(|s| {
        for camera in config.cameras {
            if camera.format.is_some() {
                warn!("The format config option of the camera has been removed in favour of auto detection.")
            }
            // Let subthreads share the camera object; in principle I think they could share
            // the object as it sits in the config.cameras block, but I have not figured out the
            // syntax for that.
            let arc_cam = Arc::new(camera);

            if ["mainStream"].iter().any(|&e| e == arc_cam.v4lstream) {
                let v4l = V4lDevice::new(arc_cam.v4ldevice as usize);
                let mut outputs = v4l
                    .add_stream()
                    .unwrap();
                let main_camera = arc_cam.clone();
                s.spawn(move |_| camera_loop(&*main_camera, Stream::Main, &mut outputs, true));
            }
            if ["subStream"].iter().any(|&e| e == arc_cam.v4lstream) {
                let v4l = V4lDevice::new(arc_cam.v4ldevice as usize);
                let mut outputs = v4l
                    .add_stream()
                    .unwrap();
                let sub_camera = arc_cam.clone();
                let manage = arc_cam.stream == "subStream";
                s.spawn(move |_| camera_loop(&*sub_camera, Stream::Sub, &mut outputs, manage));
            }
            if ["externStream"].iter().any(|&e| e == arc_cam.v4lstream) {
                let v4l = V4lDevice::new(arc_cam.v4ldevice as usize);
                let mut outputs = v4l
                    .add_stream()
                    .unwrap();
                let sub_camera = arc_cam.clone();
                let manage = arc_cam.stream == "externStream";
                s.spawn(move |_| camera_loop(&*sub_camera, Stream::Extern, &mut outputs, manage));
            }
        }
    })
    .unwrap();

    Ok(())
}

fn camera_loop(
    camera_config: &CameraConfig,
    stream_name: Stream,
    outputs: &mut V4ltOutputs,
    manage: bool,
) -> Result<Never> {
    let min_backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(15);
    let mut current_backoff = min_backoff;

    loop {
        let cam_err = camera_main(camera_config, stream_name, outputs, manage).unwrap_err();
        // Authentication failures are permanent; we retry everything else
        if cam_err.connected {
            current_backoff = min_backoff;
        }
        if cam_err.login_fail {
            error!(
                "Authentication failed to camera {}, not retrying",
                camera_config.name
            );
            return Err(cam_err.err);
        } else {
            error!(
                "Error streaming from camera {}, will retry in {}s: {:?}",
                camera_config.name,
                current_backoff.as_secs(),
                cam_err.err
            )
        }

        std::thread::sleep(current_backoff);
        current_backoff = std::cmp::min(max_backoff, current_backoff * 2);
    }
}

struct CameraErr {
    connected: bool,
    login_fail: bool,
    err: anyhow::Error,
}

fn camera_main(
    camera_config: &CameraConfig,
    stream_name: Stream,
    outputs: &mut V4ltOutputs,
    manage: bool,
) -> Result<Never, CameraErr> {
    let mut connected = false;
    let mut login_fail = false;
    (|| {
        let camera_addr =
            AddressOrUid::new(&camera_config.camera_addr, &camera_config.camera_uid).unwrap();
        let mut camera =
            camera_addr.connect_camera(camera_config.channel_id)
                .with_context(|| {
                    format!(
                        "Failed to connect to camera {} at {} on channel {}",
                        camera_config.name, camera_addr, camera_config.channel_id
                    )
                })?;

        if camera_config.timeout.is_some() {
            warn!("The undocumented `timeout` config option has been removed and is no longer needed.");
            warn!("Please update your config file.");
        }

        info!(
            "{}: Connecting to camera at {}",
            camera_config.name, camera_addr
        );

        info!("{}: Logging in", camera_config.name);
        camera.login(&camera_config.username, camera_config.password.as_deref()).map_err(|e|
            {
                if let neolink_core::Error::AuthFailed = e {
                    login_fail = true;
                }
                e
            }
        ).with_context(|| format!("Failed to login to {}", camera_config.name))?;

        connected = true;
        info!("{}: Connected and logged in", camera_config.name);

        if manage {
            do_camera_management(&mut camera, camera_config).context("Failed to manage the camera settings")?;
        }

        let stream_display_name = match stream_name {
            Stream::Main => "Main Stream (Clear)",
            Stream::Sub => "Sub Stream (Fluent)",
            Stream::Extern => "Extern Stream (Balanced)",
        };

        info!(
            "{}: Starting video stream {}",
            camera_config.name, stream_display_name
        );
        camera.start_video(outputs, stream_name).with_context(|| format!("Error while streaming {}", camera_config.name))
    })().map_err(|e| CameraErr{
        connected,
        login_fail,
        err: e,
    })
}

fn do_camera_management(camera: &mut BcCamera, camera_config: &CameraConfig) -> Result<()> {
    let cam_time = camera.get_time()?;
    if let Some(time) = cam_time {
        info!(
            "{}: Camera time is already set: {}",
            camera_config.name, time
        );
    } else {
        use time::OffsetDateTime;
        // We'd like now_local() but it's deprecated - try to get the local time, but if no
        // time zone, fall back to UTC.
        let new_time =
            OffsetDateTime::try_now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());

        warn!(
            "{}: Camera has no time set, setting to {}",
            camera_config.name, new_time
        );
        camera.set_time(new_time)?;
        let cam_time = camera.get_time()?;
        if let Some(time) = cam_time {
            info!("{}: Camera time is now set: {}", camera_config.name, time);
        } else {
            error!(
                "{}: Camera did not accept new time (is {} an admin?)",
                camera_config.name, camera_config.username
            );
        }
    }

    use neolink_core::bc::xml::VersionInfo;
    if let Ok(VersionInfo {
        firmwareVersion: firmware_version,
        ..
    }) = camera.version()
    {
        info!(
            "{}: Camera reports firmware version {}",
            camera_config.name, firmware_version
        );
    } else {
        info!(
            "{}: Could not fetch version information",
            camera_config.name
        );
    }

    Ok(())
}
