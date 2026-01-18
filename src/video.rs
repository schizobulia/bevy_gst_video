use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

extern crate ffmpeg_next as ffmpeg;

use ffmpeg::{
    format::{input, Pixel},
    media::Type,
    software::scaling::{context::Context as ScalingContext, flag::Flags},
    util::frame::video::Video as VideoFrame,
};

pub struct VideoInfo {
    pub height: u32,
    pub width: u32,
    pub data: Vec<u8>,
    pub pts: u64,
}

#[allow(dead_code)]
pub struct AudioBuffer {
    pub samples: VecDeque<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioBuffer {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            sample_rate: 44100,
            channels: 2,
        }
    }
}

pub struct FfmpegPlayer {
    pub frame: Arc<Mutex<VecDeque<VideoInfo>>>,
    pub audio_buffer: Arc<Mutex<AudioBuffer>>,
    pub previous_pts: Arc<Mutex<u64>>,
    pub duration: u64,

    pub is_playing: Arc<AtomicBool>,
    pub should_stop: Arc<AtomicBool>,

    uri: String,
}

impl Clone for FfmpegPlayer {
    fn clone(&self) -> Self {
        Self {
            frame: Arc::clone(&self.frame),
            audio_buffer: Arc::clone(&self.audio_buffer),
            previous_pts: Arc::clone(&self.previous_pts),
            duration: self.duration,
            is_playing: Arc::clone(&self.is_playing),
            should_stop: Arc::clone(&self.should_stop),
            uri: self.uri.clone(),
        }
    }
}

impl FfmpegPlayer {
    pub fn new(uri: &str) -> Self {
        ffmpeg::init().expect("Failed to initialize FFmpeg");

        Self {
            frame: Arc::new(Mutex::new(VecDeque::new())),
            audio_buffer: Arc::new(Mutex::new(AudioBuffer::new())),
            previous_pts: Arc::new(Mutex::new(0)),
            duration: 0,
            is_playing: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            uri: uri.to_string(),
        }
    }

    pub fn play(&self) {
        println!("[DEBUG] FfmpegPlayer::play() called");
        self.is_playing.store(true, Ordering::Relaxed);
        println!("[DEBUG] is_playing set to true");
    }

    pub fn pause(&self) {
        println!("[DEBUG] FfmpegPlayer::pause() called");
        self.is_playing.store(false, Ordering::Relaxed);
    }

    pub fn destroy(&self) {
        self.should_stop.store(true, Ordering::Relaxed);
        self.is_playing.store(false, Ordering::Relaxed);
    }

    pub fn start(&mut self) {
        let frame_queue = Arc::clone(&self.frame);
        let audio_buffer = Arc::clone(&self.audio_buffer);
        let is_playing = Arc::clone(&self.is_playing);
        let should_stop = Arc::clone(&self.should_stop);
        let uri = self.uri.clone();

        thread::spawn(move || {
            // Setup audio output in the decoder thread
            let audio_stream = Self::setup_audio_output(Arc::clone(&audio_buffer));
            if let Some(ref stream) = audio_stream {
                let _ = stream.play();
            }

            if let Err(e) = Self::decode_loop(
                &uri,
                frame_queue,
                audio_buffer,
                is_playing,
                should_stop,
            ) {
                eprintln!("Decode error: {}", e);
            }

            // Audio stream is dropped here when decode loop ends
        });
    }

    fn setup_audio_output(audio_buffer: Arc<Mutex<AudioBuffer>>) -> Option<cpal::Stream> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;

        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(44100),
            buffer_size: cpal::BufferSize::Default,
        };

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if let Ok(mut buffer) = audio_buffer.lock() {
                        for sample in data.iter_mut() {
                            *sample = buffer.samples.pop_front().unwrap_or(0.0);
                        }
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
            .ok()?;

        Some(stream)
    }

    fn decode_loop(
        uri: &str,
        frame_queue: Arc<Mutex<VecDeque<VideoInfo>>>,
        audio_buffer: Arc<Mutex<AudioBuffer>>,
        is_playing: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
    ) -> Result<(), ffmpeg::Error> {
        println!("[DEBUG] decode_loop starting, opening: {}", uri);
        let mut ictx = input(&uri)?;
        println!("[DEBUG] Input opened successfully");

        let video_stream_index = ictx
            .streams()
            .best(Type::Video)
            .map(|s| s.index());
        println!("[DEBUG] Video stream index: {:?}", video_stream_index);

        let audio_stream_index = ictx
            .streams()
            .best(Type::Audio)
            .map(|s| s.index());
        println!("[DEBUG] Audio stream index: {:?}", audio_stream_index);

        let mut video_decoder = video_stream_index
            .and_then(|idx| ictx.stream(idx))
            .map(|stream| {
                let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters()).ok()?;
                context.decoder().video().ok()
            })
            .flatten();

        let mut audio_decoder = audio_stream_index
            .and_then(|idx| ictx.stream(idx))
            .map(|stream| {
                let context = ffmpeg::codec::context::Context::from_parameters(stream.parameters()).ok()?;
                context.decoder().audio().ok()
            })
            .flatten();

        let video_time_base = video_stream_index
            .and_then(|idx| ictx.stream(idx))
            .map(|s| s.time_base());

        let mut scaler: Option<ScalingContext> = None;
        let mut resampler: Option<ffmpeg::software::resampling::Context> = None;

        println!("[DEBUG] Starting packet loop, waiting for is_playing...");
        let mut frame_count = 0u64;
        for (stream, packet) in ictx.packets() {
            if should_stop.load(Ordering::Relaxed) {
                println!("[DEBUG] should_stop is true, breaking");
                break;
            }

            while !is_playing.load(Ordering::Relaxed) && !should_stop.load(Ordering::Relaxed) {
                thread::sleep(std::time::Duration::from_millis(10));
            }

            if frame_count == 0 {
                println!("[DEBUG] is_playing became true, starting decode");
            }

            if should_stop.load(Ordering::Relaxed) {
                break;
            }

            if Some(stream.index()) == video_stream_index {
                frame_count += 1;
                if frame_count % 100 == 0 {
                    println!("[DEBUG] Decoded {} video frames", frame_count);
                }
                if let Some(ref mut decoder) = video_decoder {
                    decoder.send_packet(&packet)?;

                    let mut decoded = VideoFrame::empty();
                    while decoder.receive_frame(&mut decoded).is_ok() {
                        if scaler.is_none() {
                            scaler = Some(ScalingContext::get(
                                decoded.format(),
                                decoded.width(),
                                decoded.height(),
                                Pixel::RGBA,
                                decoded.width(),
                                decoded.height(),
                                Flags::BILINEAR,
                            )?);
                        }

                        if let Some(ref mut scaler) = scaler {
                            let mut rgb_frame = VideoFrame::empty();
                            scaler.run(&decoded, &mut rgb_frame)?;

                            let pts = decoded.pts().unwrap_or(0);
                            let pts_us = if let Some(tb) = video_time_base {
                                (pts as f64 * tb.numerator() as f64 / tb.denominator() as f64 * 1_000_000_000.0) as u64
                            } else {
                                pts as u64
                            };

                            // Handle stride/padding: copy only the actual pixel data
                            let width = rgb_frame.width() as usize;
                            let height = rgb_frame.height() as usize;
                            let stride = rgb_frame.stride(0);
                            let src_data = rgb_frame.data(0);

                            let mut pixel_data = Vec::with_capacity(width * height * 4);
                            for y in 0..height {
                                let row_start = y * stride;
                                let row_end = row_start + width * 4;
                                pixel_data.extend_from_slice(&src_data[row_start..row_end]);
                            }

                            let video_info = VideoInfo {
                                width: width as u32,
                                height: height as u32,
                                data: pixel_data,
                                pts: pts_us,
                            };

                            if let Ok(mut frames) = frame_queue.lock() {
                                if frames.len() < 100 {
                                    frames.push_back(video_info);
                                }
                            }
                        }
                    }
                }
            } else if Some(stream.index()) == audio_stream_index {
                if let Some(ref mut decoder) = audio_decoder {
                    decoder.send_packet(&packet)?;

                    let mut decoded = ffmpeg::util::frame::audio::Audio::empty();
                    while decoder.receive_frame(&mut decoded).is_ok() {
                        if resampler.is_none() {
                            resampler = Some(ffmpeg::software::resampling::Context::get(
                                decoded.format(),
                                decoded.channel_layout(),
                                decoded.rate(),
                                ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
                                ffmpeg::channel_layout::ChannelLayout::STEREO,
                                44100,
                            )?);
                        }

                        if let Some(ref mut resampler) = resampler {
                            // Estimate output samples
                            let in_samples = decoded.samples();
                            let out_rate = 44100u64;
                            let in_rate = decoded.rate() as u64;
                            let estimated_out = ((in_samples as u64 * out_rate / in_rate) + 256) as usize;

                            let mut resampled = ffmpeg::util::frame::audio::Audio::new(
                                ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
                                estimated_out,
                                ffmpeg::channel_layout::ChannelLayout::STEREO,
                            );

                            if resampler.run(&decoded, &mut resampled).is_ok() {
                                let plane = resampled.plane::<f32>(0);
                                if let Ok(mut buffer) = audio_buffer.lock() {
                                    for &sample in plane {
                                        buffer.samples.push_back(sample);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        println!("EOS");
        Ok(())
    }
}
