use v4l::buffer::Type;
use v4l::io::traits::OutputStream;
use v4l::prelude::*;
use v4l::video::Output;
use v4l::{Format, FourCC};
use neolink_core::{
    bc_protocol::{StreamOutput, StreamOutputError},
    bcmedia::model::*,
};

type Result<T> = std::result::Result<T, ()>;

pub(crate) struct V4lDevice {
    device: Device,
}

pub(crate) struct V4ltOutputs<'a> {
    device: Device,
    stream: &'a mut MmapStream<'a>,
    video_width: Option<u32>,
    video_height: Option<u32>,
    video_format: Option<StreamFormat>,
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

impl<'a> StreamOutput for V4ltOutputs<'a> {
    fn write(&mut self, media: BcMedia) -> StreamOutputError {
        match media {
            BcMedia::Iframe(payload) => {
                let video_type = match payload.video_type {
                    VideoType::H264 => StreamFormat::H264,
                    VideoType::H265 => StreamFormat::H265,
                };
                self.set_format(Some(video_type));

                let (buf_out, buf_out_meta) = OutputStream::next(self.stream).unwrap();

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
                let video_type = match payload.video_type {
                    VideoType::H264 => StreamFormat::H264,
                    VideoType::H265 => StreamFormat::H265,
                };
                self.set_format(Some(video_type));

                let (buf_out, buf_out_meta) = OutputStream::next(self.stream).unwrap();

                // Output devices generally cannot know the exact size of the output buffers for
                // compressed formats (e.g. MJPG). They will however allocate a size that is always
                // large enough to hold images of the format in question. We know how big a buffer we need
                // since we control the input buffer - so just enforce that size on the output buffer.
                let buf_out = &mut buf_out[0..payload.data.len()];

                buf_out.copy_from_slice(&payload.data);
                buf_out_meta.field = 0;
                //buf_out_meta.bytesused = buf_in_meta.bytesused;
            }
            BcMedia::InfoV1(info) => {
                self.set_resolution(info.video_width, info.video_height);
            }
            BcMedia::InfoV2(info) => {
                self.set_resolution(info.video_width, info.video_height);
            }
            _ => {
                //Ignore other BcMedia
            }
        }

        Ok(())
    }
}

impl<'a> V4ltOutputs<'a> {
    pub(crate) fn from_device(device: Device, stream: &'a mut MmapStream<'a>) -> V4ltOutputs<'a> {
        let result = V4ltOutputs {
            device,
            stream,
            video_width: None,
            video_height: None,
            video_format: None,
        };
        result.apply_format();
        result
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

    fn apply_format(&self) {
        let vid_format = match self.video_format {
            Some(StreamFormat::H264) => {
                b"AVC1"
            }
            Some(StreamFormat::H265) => {
                b"HEVC"
            }
            _ => {
                unreachable!();
            }
        };

        if self.video_width.is_some() && self.video_height.is_some() {
            let fmt = Format::new(self.video_width.unwrap(), self.video_height.unwrap(), FourCC::new(vid_format));

            let sink_fmt = Output::set_format(&self.device, &fmt).unwrap();
    
            println!("New out format:\n{}", sink_fmt);
        }        
    }
}

impl V4lDevice {
    pub(crate) fn new(
        device_index: usize,
    ) -> V4lDevice {
        V4lDevice {
            device: Device::new(device_index).expect("Failed to create device"),
        }
    }

    pub(crate) fn add_stream(
        &self,
    ) -> Result<V4ltOutputs> {
        let mut stream = MmapStream::new(&self.device, Type::VideoOutput).expect("Failed to create buffer stream");

        let outputs = V4ltOutputs::from_device(self.device, &mut stream);

        Ok(outputs)
    }
}
