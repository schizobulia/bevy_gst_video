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
}
pub fn create_pipeline(ls: Arc<Mutex<VecDeque<VideoInfo>>>, video_rate: Arc<Mutex<f64>>) {
    let (_stream, stream_handle) = OutputStream::try_default().expect("Error");
    let player_audio = rodio::Sink::try_new(&stream_handle).expect("Error");

    gst::init().unwrap();
    let uri = "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm";
    let pipeline = gst::parse::launch(&format!(
        "uridecodebin uri={uri} name=decodebin ! videoconvert ! appsink name=video_sink \
 decodebin. ! audioconvert ! appsink name=audio_sink"
    ))
    .unwrap()
    .downcast::<gst::Pipeline>()
    .expect("Expected a gst::Pipeline");
    let appsink = pipeline
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
    pipeline.set_state(gst::State::Playing).unwrap();
    let mut tmp_list = VecDeque::new();
    let mut loading_tag = false;
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
                let pixel_data = frame.plane_data(0).unwrap();
                let video_info = VideoInfo {
                    width: frame.width(),
                    height: frame.height(),
                    data: pixel_data.to_vec(),
                };
                if loading_tag {
                    ls.lock().unwrap().push_back(video_info);
                } else {
                    tmp_list.push_back(video_info);
                    if tmp_list.len() > 20 {
                        ls.lock().unwrap().append(&mut tmp_list);
                        tmp_list.clear();
                        loading_tag = true;
                    }
                }
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );
    let audio_sink = pipeline
        .by_name("audio_sink")
        .expect("Audio sink element not found")
        .downcast::<gst_app::AppSink>()
        .expect("Audio sink element is expected to be an appsink!");
    let bus = pipeline.bus().unwrap();

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
                player_audio.append(s);

                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;
        match msg.view() {
            MessageView::StateChanged(state_changed) => {
                if state_changed.src().map(|s| s == &pipeline).unwrap_or(false)
                    && state_changed.current() == gst::State::Playing
                {
                    if let Some(duration) = pipeline.query_duration::<gst::ClockTime>() {
                        if let Some(clock) = pipeline.clock() {
                            if let Some(rate) = clock.control_rate() {
                                let mut tmp_rate = video_rate.lock().unwrap();
                                *tmp_rate = duration.seconds_f64() / rate.seconds_f64();
                                drop(tmp_rate);
                            }
                        }
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
