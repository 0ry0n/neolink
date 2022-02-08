use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender};
use neolink_core::{
    bc_protocol::{StreamOutput, StreamOutputError},
    bcmedia::model::*,
    Error as NeolinkError,
};
use v4l::buffer::Type;
use v4l::io::traits::OutputStream;
use v4l::prelude::*;
use v4l::video::Output;
use v4l::video::output::Parameters;
use v4l::{Format, FourCC};

pub(crate) struct V4lDevice {
    device: Device,
    receiver: Receiver<BcMedia>,
    video_width: Option<u32>,
    video_height: Option<u32>,
    video_fps: Option<u8>,
    video_format: Option<StreamFormat>,
}

pub(crate) struct V4lOutputs {
    sender: Sender<BcMedia>,
}

// The stream from the camera will be using one of these formats
//
// This is used as part of `StreamOutput` to give hints about
// the format of the stream
#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
enum StreamFormat {
    // H264 (AVC) video format
    H264,
    // H265 (HEVC) video format
    H265,
}

impl V4lOutputs {
    pub(crate) fn new(sender: Sender<BcMedia>) -> Self {
        Self { sender }
    }
}

impl StreamOutput for V4lOutputs {
    fn write(&mut self, media: BcMedia) -> StreamOutputError {
        self.sender
            .send(media)
            .map_err(|_| NeolinkError::Other("V4l Device dropped"))
    }
}

impl V4lDevice {
    pub(crate) fn from_device(device_index: u8, receiver: Receiver<BcMedia>) -> Result<Self> {
        let result = Self {
            device: Device::new(device_index as usize).expect("Failed to create device"),
            receiver,
            video_width: None,
            video_height: None,
            video_fps: None,
            video_format: None,
        };
        result.apply_format();
        Ok(result)
    }

    pub(crate) fn run(&mut self) -> Result<()> {
        let mut packets: usize = 0;
        // After we have created the device stream we cannot
        // edit the height/width etc
        // So first we pull packets from the camera until we have
        // enough data to setup the height etc
        while (self.video_width.is_none()
            || self.video_height.is_none()
            || self.video_fps.is_none()
            || self.video_format.is_none())
            && packets <= 10
        {
            let media = self.receiver.recv()?;
            match media {
                BcMedia::Iframe(payload) => {
                    let video_type = match payload.video_type {
                        VideoType::H264 => StreamFormat::H264,
                        VideoType::H265 => StreamFormat::H265,
                    };
                    self.set_format(Some(video_type));
                }
                BcMedia::Pframe(payload) => {
                    let video_type = match payload.video_type {
                        VideoType::H264 => StreamFormat::H264,
                        VideoType::H265 => StreamFormat::H265,
                    };
                    self.set_format(Some(video_type));
                }
                BcMedia::InfoV1(info) => {
                    self.set_resolution(info.video_width, info.video_height);
                    self.set_fps(info.fps);
                }
                BcMedia::InfoV2(info) => {
                    self.set_resolution(info.video_width, info.video_height);
                    self.set_fps(info.fps);
                }
                _ => {
                    //Ignore other BcMedia
                }
            }
            packets += 1;
        }
        // Now that we have fully determined the settings for the stream we can create the stream
        let mut stream = self.get_stream()?;
        // Loop until error
        loop {
            let media = self.receiver.recv()?;
            match media {
                BcMedia::Iframe(payload) => {
                    let (buf_out, buf_out_meta) = OutputStream::next(&mut stream)?;

                    // Output devices generally cannot know the exact size of the output buffers for
                    // compressed formats (e.g. MJPG). They will however allocate a size that is always
                    // large enough to hold images of the format in question. We know how big a buffer we need
                    // since we control the input buffer - so just enforce that size on the output buffer.
                    let buf_out = &mut buf_out[0..payload.data.len()];

                    buf_out.copy_from_slice(&payload.data);
                    buf_out_meta.field = 0;
                    //buf_out_meta.bytesused = buf_in_meta.bytesused;
                }
                BcMedia::Pframe(payload) => {
                    let (buf_out, buf_out_meta) = OutputStream::next(&mut stream)?;

                    // Output devices generally cannot know the exact size of the output buffers for
                    // compressed formats (e.g. MJPG). They will however allocate a size that is always
                    // large enough to hold images of the format in question. We know how big a buffer we need
                    // since we control the input buffer - so just enforce that size on the output buffer.
                    let buf_out = &mut buf_out[0..payload.data.len()];

                    buf_out.copy_from_slice(&payload.data);
                    buf_out_meta.field = 0;
                    //buf_out_meta.bytesused = buf_in_meta.bytesused;
                }
                _ => {
                    //Ignore other BcMedia
                }
            }
        }
    }

    pub(crate) fn get_stream(&self) -> Result<MmapStream> {
        Ok(MmapStream::new(&self.device, Type::VideoOutput)
            .context("Failed to create buffer stream")?)
    }

    fn set_format(&mut self, format: Option<StreamFormat>) {
        match format {
            Some(StreamFormat::H264) | Some(StreamFormat::H265) => {
                if format != self.video_format {
                    self.video_format = format;
                    self.apply_format();
                }
            }
            _ => {}
        }
    }

    fn set_resolution(&mut self, width: u32, height: u32) {
        self.video_width = Some(width);
        self.video_height = Some(height);

        self.apply_format();
    }

    fn set_fps(&mut self, fps: u8) {
        self.video_fps = Some(fps);

        self.apply_format();
    }

    fn apply_format(&self) {
        let vid_format = match self.video_format {
            Some(StreamFormat::H264) => b"AVC1",
            Some(StreamFormat::H265) => b"HEVC",
            None => return,
        };

        if self.video_width.is_some() && self.video_height.is_some() && self.video_fps.is_some() {
            let fmt = Format::new(
                self.video_width.unwrap(),
                self.video_height.unwrap(),
                FourCC::new(vid_format),
            );

            let params = Parameters::with_fps(
                self.video_fps.unwrap() as u32
            );

            let sink_fmt = Output::set_format(&self.device, &fmt).unwrap();

            println!("New out format:\n{}", sink_fmt);

            let sink_params = Output::set_params(&self.device, &params);

            println!("New out params:\n{}", sink_params.unwrap());
        }
    }
}
