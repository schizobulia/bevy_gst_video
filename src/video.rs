extern crate gstreamer as gst;
extern crate gstreamer_app as gst_app;
extern crate gstreamer_audio as gst_audio;
extern crate gstreamer_video as gst_video;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use byteorder::{ByteOrder, LittleEndian};
use gst::{element_error, prelude::*};
use gstreamer_video::VideoFrameExt;
use rodio::OutputStream;

pub struct VideoInfo {
    pub height: u32,
    pub width: u32,
    pub data: Vec<u8>,
    pub pts: u64,
}

#[derive(Clone)]
pub struct GstPlayer {
    pipeline: gst::Pipeline,
    pub frame: Arc<Mutex<VecDeque<VideoInfo>>>,
    pub previous_pts: Arc<Mutex<u64>>,
    pub duration: u64,
}

impl GstPlayer {
    pub fn new(uri: &str) -> Self {
        gst::init().expect("Failed to initialize gstreamer");
        let pipeline = gst::parse::launch(&format!(
            "uridecodebin uri={uri} name=decodebin ! \
            videoconvert ! appsink name=video_sink \
            decodebin. ! audioconvert ! appsink name=audio_sink"
        ))
        .expect("Failed to create pipeline")
        .downcast::<gst::Pipeline>()
        .expect("Expected a gst::Pipeline");
        GstPlayer {
            pipeline: pipeline,
            frame: Arc::new(Mutex::new(VecDeque::new())),
            duration: 0,
            previous_pts: Arc::new(Mutex::new(0)),
        }
    }

    pub fn play(&self) {
        self.pipeline
            .set_state(gst::State::Playing)
            .expect("play error");
    }

    pub fn pause(&self) {
        self.pipeline
            .set_state(gst::State::Paused)
            .expect("pause error");
    }
    pub fn destroy(&self) {
        self.pipeline
            .set_state(gst::State::Null)
            .expect("destroy error");
    }
    pub fn start(&mut self) {
        let (_stream, stream_handle) = OutputStream::try_default().expect("Error");
        let ps = rodio::Sink::try_new(&stream_handle).expect("Error");

        let appsink = self
            .pipeline
            .by_name("video_sink")
            .expect("Sink element not found")
            .downcast::<gst_app::AppSink>()
            .expect("Sink element is expected to be an appsink!");

        appsink.set_property("sync", true);
        appsink.set_caps(Some(
            &gst_video::VideoCapsBuilder::new()
                .format(gst_video::VideoFormat::Rgbx)
                .build(),
        ));
        appsink.set_max_buffers(100);
        self.pipeline
            .set_state(gst::State::Paused)
            .expect("paused error");
        let self_frame = Arc::clone(&self.frame);
        appsink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |appsink| {
                    let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buffer = sample.buffer().ok_or_else(|| {
                        element_error!(
                            appsink,
                            gst::ResourceError::Failed,
                            ("Failed to get buffer from appsink")
                        );
                        gst::FlowError::Error
                    })?;
                    let caps = sample.caps().expect("Sample without caps");
                    let info = gst_video::VideoInfo::from_caps(caps).expect("Failed to parse caps");
                    let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info)
                        .map_err(|_| {
                        element_error!(
                            appsink,
                            gst::ResourceError::Failed,
                            ("Failed to map buffer readable")
                        );

                        gst::FlowError::Error
                    })?;
                    let pixel_data = frame.plane_data(0).expect("Failed to get pixel data");
                    let video_info = VideoInfo {
                        width: frame.width(),
                        height: frame.height(),
                        data: pixel_data.to_vec(),
                        pts: buffer.pts().expect("pts error").nseconds(),
                    };
                    self_frame
                        .lock()
                        .expect("self_frame error")
                        .push_back(video_info);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        let audio_sink = self
            .pipeline
            .by_name("audio_sink")
            .expect("Audio sink element not found")
            .downcast::<gst_app::AppSink>()
            .expect("Audio sink element is expected to be an appsink!");
        let bus = self.pipeline.bus().expect("Pipeline without bus");
        audio_sink.set_caps(Some(
            &gst_audio::AudioCapsBuilder::new()
                .format(gst_audio::AudioFormat::F32le)
                .build(),
        ));
        audio_sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |audio_sink| {
                    let sample = audio_sink
                        .pull_sample()
                        .map_err(|_| gst::FlowError::Eos)
                        .expect("Error");
                    let buffer = sample
                        .buffer()
                        .ok_or_else(|| {
                            element_error!(
                                audio_sink,
                                gst::ResourceError::Failed,
                                ("Failed to get buffer from appsink")
                            );
                            gst::FlowError::Error
                        })
                        .expect("Error");

                    let caps = sample.caps().expect("Sample without caps");
                    let info = gst_audio::AudioInfo::from_caps(caps).expect("Failed to parse caps");
                    let map: gstreamer::BufferMap<gstreamer::buffer::Readable> =
                        buffer.map_readable().map_err(|_| {
                            element_error!(
                                appsink,
                                gst::ResourceError::Failed,
                                ("Failed to map buffer readable")
                            );
                            gst::FlowError::Error
                        })?;
                    let u8_data: &[u8] = map.as_slice();
                    let mut f32_data = vec![0f32; u8_data.len() / 4];
                    LittleEndian::read_f32_into(u8_data, &mut f32_data);
                    let ch = info.channels() as u16;
                    let rate = info.rate();
                    let s = rodio::buffer::SamplesBuffer::new(ch, rate, f32_data);
                    ps.append(s);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        for msg in bus.iter_timed(gst::ClockTime::NONE) {
            use gst::MessageView;
            match msg.view() {
                MessageView::StateChanged(state_changed) => {
                    if state_changed
                        .src()
                        .map(|s| s == &self.pipeline)
                        .unwrap_or(false)
                        && state_changed.current() == gst::State::Playing
                    {
                    } else if state_changed
                        .src()
                        .map(|s| s == &self.pipeline)
                        .unwrap_or(false)
                        && state_changed.current() == gst::State::Paused
                    {
                        if let Some(duration) = self.pipeline.query_duration::<gst::ClockTime>() {
                            self.duration = duration.mseconds();
                        }
                    }
                }
                MessageView::Eos(..) => {
                    println!("EOS");
                    break;
                }
                MessageView::Error(err) => {
                    eprintln!(
                        "Error from {:?}: {} ({:?})",
                        err.src().map(|s| s.path_string()),
                        err.error(),
                        err.debug()
                    );
                    break;
                }
                _ => (),
            }
        }
    }
}
