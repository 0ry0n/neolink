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
    vidsrc: MmapStream<'a>,
    video_format: Option<StreamFormat>,
    device: &'a Device,
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

impl StreamOutput for V4ltOutputs<'_> {
    fn write(&mut self, media: BcMedia) -> StreamOutputError {
        match media {
            BcMedia::Iframe(payload) => {
                let video_type = match payload.video_type {
                    VideoType::H264 => StreamFormat::H264,
                    VideoType::H265 => StreamFormat::H265,
                };
                self.set_format(Some(video_type));
                //self.vidsrc.write_all(&payload.data)?;

                let (buf_out, buf_out_meta) = OutputStream::next(&mut self.vidsrc).unwrap();

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
                //self.vidsrc.write_all(&payload.data)?;

                let (buf_out, buf_out_meta) = OutputStream::next(&mut self.vidsrc).unwrap();

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

        Ok(())
    }
}

impl V4ltOutputs<'_> {
    pub(crate) fn from_appsrcs<'a>(device: &'a Device, vidsrc: MmapStream<'a>) -> V4ltOutputs<'a> {
        let result = V4ltOutputs {
            vidsrc,
            video_format: None,
            device,
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

    fn apply_format(&self) {
        let launch_vid = match self.video_format {
            Some(StreamFormat::H264) => {
                b"AVC1"
            }
            Some(StreamFormat::H265) => {
                b"HEVC"
            }
            // TODO
            _ => b"AVC1",
        };
        // TODO
        let fmt = Format::new(640, 480, FourCC::new(launch_vid));

        let sink_fmt = Output::set_format(self.device, &fmt).unwrap();

        println!("New out format:\n{}", sink_fmt);
    }
}

impl V4lDevice {
    pub(crate) fn new(
        device_index: usize,
    ) -> V4lDevice {
        V4lDevice {
            device: Device::new(device_index).unwrap(),
        }
    }

    pub(crate) fn add_stream(
        &self,
    ) -> Result<V4ltOutputs> {
        let stream = MmapStream::with_buffers(&self.device, Type::VideoOutput, 4).unwrap();//.expect("Failed to create buffer stream");

        let outputs = V4ltOutputs::from_appsrcs(&self.device, stream);

        Ok(outputs)
    }
}
