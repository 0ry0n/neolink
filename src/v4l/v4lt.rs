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
use v4l::video::output::Parameters;
use v4l::video::Output;
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
        Ok(result)
    }

    pub(crate) fn run(&mut self) -> Result<()> {
        // After we have created the device stream we cannot
        // edit the height/width etc
        // So first we pull packets from the camera until we have
        // enough data to setup the height etc
        while self.video_width.is_none()
            || self.video_height.is_none()
            || self.video_fps.is_none()
            || self.video_format.is_none()
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
                    self.set_resolution(Some(info.video_width), Some(info.video_height));
                    self.set_fps(Some(info.fps));
                }
                BcMedia::InfoV2(info) => {
                    self.set_resolution(Some(info.video_width), Some(info.video_height));
                    self.set_fps(Some(info.fps));
                }
                _ => {
                    //Ignore other BcMedia
                }
            }
        }
        self.apply_format();

        // Now that we have fully determined the settings for the stream we can create the stream
        let mut stream = self.get_stream()?;

        // Loop until error
        loop {
            let media = self.receiver.recv()?;
            match media {
                BcMedia::Iframe(payload) => {
                    let (buf_out, buf_out_meta) = OutputStream::next(&mut stream)?;

                    let buf_out = &mut buf_out[0..payload.data.len()];

                    buf_out.copy_from_slice(&payload.data);
                    buf_out_meta.bytesused = payload.data.len() as u32;
                    //buf_out_meta.flags
                    buf_out_meta.field = 0;
                    //buf_out_meta.timestamp = Timestamp::new(0, payload.microseconds.into());
                    //buf_out_meta.sequence
                }
                BcMedia::Pframe(payload) => {
                    let (buf_out, buf_out_meta) = OutputStream::next(&mut stream)?;

                    let buf_out = &mut buf_out[0..payload.data.len()];

                    buf_out.copy_from_slice(&payload.data);
                    buf_out_meta.bytesused = payload.data.len() as u32;
                    //buf_out_meta.flags
                    buf_out_meta.field = 0;
                    //buf_out_meta.timestamp = Timestamp::new(0, payload.microseconds.into());
                    //buf_out_meta.sequence
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
        self.video_format = format;
    }

    fn set_resolution(&mut self, width: Option<u32>, height: Option<u32>) {
        self.video_width = width;
        self.video_height = height;
    }

    fn set_fps(&mut self, fps: Option<u8>) {
        self.video_fps = fps;
    }

    fn apply_format(&self) {
        let vid_format = match self.video_format {
            Some(StreamFormat::H264) => b"H264",
            Some(StreamFormat::H265) => b"HEVC",
            None => return,
        };

        if self.video_width.is_some() && self.video_height.is_some() && self.video_fps.is_some() {
            let fmt = Format::new(
                self.video_width.unwrap(),
                self.video_height.unwrap(),
                FourCC::new(vid_format),
            );

            let params = Parameters::with_fps(self.video_fps.unwrap() as u32);

            Output::set_format(&self.device, &fmt).unwrap();
            Output::set_params(&self.device, &params).unwrap();
        }
    }
}
