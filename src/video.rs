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
    pub position_secs: f64,
}

#[allow(dead_code)]
pub struct AudioBuffer {
    pub samples: VecDeque<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    last_sample: f32,
    fade_factor: f32,
}

impl AudioBuffer {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            samples: VecDeque::new(),
            sample_rate,
            channels,
            last_sample: 0.0,
            fade_factor: 1.0,
        }
    }

    /// Get next sample with smooth fade to avoid pops when buffer is empty
    pub fn next_sample(&mut self) -> f32 {
        if let Some(sample) = self.samples.pop_front() {
            self.last_sample = sample;
            self.fade_factor = 1.0;
            sample
        } else {
            // Smoothly fade out to avoid pop noise
            self.fade_factor *= 0.95;
            self.last_sample * self.fade_factor
        }
    }
}

/// Audio format info extracted from video
#[derive(Clone, Debug)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct FfmpegPlayer {
    pub frame: Arc<Mutex<VecDeque<VideoInfo>>>,
    pub audio_buffer: Arc<Mutex<Option<AudioBuffer>>>,
    pub audio_format: Arc<Mutex<Option<AudioFormat>>>,
    pub previous_pts: Arc<Mutex<u64>>,
    pub duration: Arc<Mutex<f64>>,
    pub current_position: Arc<Mutex<f64>>,

    pub is_playing: Arc<AtomicBool>,
    pub should_stop: Arc<AtomicBool>,
    pub is_ready: Arc<AtomicBool>,

    uri: String,
}

impl Clone for FfmpegPlayer {
    fn clone(&self) -> Self {
        Self {
            frame: Arc::clone(&self.frame),
            audio_buffer: Arc::clone(&self.audio_buffer),
            audio_format: Arc::clone(&self.audio_format),
            previous_pts: Arc::clone(&self.previous_pts),
            duration: Arc::clone(&self.duration),
            current_position: Arc::clone(&self.current_position),
            is_playing: Arc::clone(&self.is_playing),
            should_stop: Arc::clone(&self.should_stop),
            is_ready: Arc::clone(&self.is_ready),
            uri: self.uri.clone(),
        }
    }
}

impl FfmpegPlayer {
    pub fn new(uri: &str) -> Self {
        ffmpeg::init().expect("Failed to initialize FFmpeg");

        Self {
            frame: Arc::new(Mutex::new(VecDeque::new())),
            audio_buffer: Arc::new(Mutex::new(None)),
            audio_format: Arc::new(Mutex::new(None)),
            previous_pts: Arc::new(Mutex::new(0)),
            duration: Arc::new(Mutex::new(0.0)),
            current_position: Arc::new(Mutex::new(0.0)),
            is_playing: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            is_ready: Arc::new(AtomicBool::new(false)),
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
        let audio_format = Arc::clone(&self.audio_format);
        let is_playing = Arc::clone(&self.is_playing);
        let should_stop = Arc::clone(&self.should_stop);
        let is_ready = Arc::clone(&self.is_ready);
        let duration = Arc::clone(&self.duration);
        let uri = self.uri.clone();

        thread::spawn(move || {
            if let Err(e) = Self::decode_loop(
                &uri,
                frame_queue,
                audio_buffer,
                audio_format,
                is_playing,
                should_stop,
                is_ready,
                duration,
            ) {
                eprintln!("Decode error: {}", e);
            }
        });
    }

    fn setup_audio_output(
        audio_buffer: Arc<Mutex<Option<AudioBuffer>>>,
        is_playing: Arc<AtomicBool>,
        format: &AudioFormat,
    ) -> Option<cpal::Stream> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;

        println!(
            "[DEBUG] Setting up audio output: {} Hz, {} channels",
            format.sample_rate, format.channels
        );

        let config = cpal::StreamConfig {
            channels: format.channels,
            sample_rate: cpal::SampleRate(format.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if let Ok(mut buffer_opt) = audio_buffer.lock() {
                        if let Some(ref mut buffer) = *buffer_opt {
                            // Check if playing
                            if !is_playing.load(Ordering::Relaxed) {
                                // Output silence when paused, with fade out
                                for sample in data.iter_mut() {
                                    let s = buffer.next_sample();
                                    buffer.fade_factor *= 0.9;
                                    *sample = s * buffer.fade_factor;
                                }
                                return;
                            }
                            for sample in data.iter_mut() {
                                *sample = buffer.next_sample();
                            }
                        } else {
                            // Buffer not initialized yet
                            for sample in data.iter_mut() {
                                *sample = 0.0;
                            }
                        }
                    } else {
                        // Lock failed, output silence
                        for sample in data.iter_mut() {
                            *sample = 0.0;
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
        audio_buffer: Arc<Mutex<Option<AudioBuffer>>>,
        audio_format: Arc<Mutex<Option<AudioFormat>>>,
        is_playing: Arc<AtomicBool>,
        should_stop: Arc<AtomicBool>,
        is_ready: Arc<AtomicBool>,
        duration: Arc<Mutex<f64>>,
    ) -> Result<(), ffmpeg::Error> {
        println!("[DEBUG] decode_loop starting, opening: {}", uri);
        let mut ictx = input(&uri)?;
        println!("[DEBUG] Input opened successfully");

        // Get video duration in seconds
        let duration_secs = ictx.duration() as f64 / f64::from(ffmpeg::ffi::AV_TIME_BASE);
        if let Ok(mut d) = duration.lock() {
            *d = duration_secs;
        }
        println!("[DEBUG] Video duration: {:.2} seconds", duration_secs);

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

        // Read audio format from the decoder and set up audio output
        let detected_format = if let Some(ref decoder) = audio_decoder {
            let rate = decoder.rate();
            let channels = decoder.channels() as u16;
            println!(
                "[DEBUG] Detected audio format: {} Hz, {} channels, format: {:?}",
                rate, channels, decoder.format()
            );
            Some(AudioFormat {
                sample_rate: rate,
                channels,
            })
        } else {
            println!("[DEBUG] No audio stream found");
            None
        };

        // Store audio format and initialize audio buffer
        let audio_stream = if let Some(ref fmt) = detected_format {
            if let Ok(mut format_lock) = audio_format.lock() {
                *format_lock = Some(fmt.clone());
            }
            if let Ok(mut buffer_lock) = audio_buffer.lock() {
                *buffer_lock = Some(AudioBuffer::new(fmt.sample_rate, fmt.channels));
            }
            // Setup audio output with detected format
            let stream = Self::setup_audio_output(
                Arc::clone(&audio_buffer),
                Arc::clone(&is_playing),
                fmt,
            );
            if let Some(ref s) = stream {
                let _ = s.play();
            }
            stream
        } else {
            None
        };

        // Keep audio stream alive
        let _audio_stream = audio_stream;

        let video_time_base = video_stream_index
            .and_then(|idx| ictx.stream(idx))
            .map(|s| s.time_base());

        let mut scaler: Option<ScalingContext> = None;
        let mut resampler: Option<ffmpeg::software::resampling::Context> = None;

        // Use detected format for resampler target, or default
        let target_sample_rate = detected_format.as_ref().map(|f| f.sample_rate).unwrap_or(44100);
        let target_channels = detected_format.as_ref().map(|f| f.channels).unwrap_or(2);

        println!("[DEBUG] Starting packet loop, waiting for is_playing...");
        let mut frame_count = 0u64;
        let mut prebuffering = true;
        const PREBUFFER_FRAMES: usize = 30; // Prebuffer ~1 second of video
        let prebuffer_audio_samples = (target_sample_rate as usize) / 2; // Prebuffer ~0.5 second of audio

        for (stream, packet) in ictx.packets() {
            if should_stop.load(Ordering::Relaxed) {
                println!("[DEBUG] should_stop is true, breaking");
                break;
            }

            // During prebuffering, decode without waiting for is_playing
            if prebuffering {
                let video_ready = frame_queue.lock().map(|f| f.len() >= PREBUFFER_FRAMES).unwrap_or(false);
                let audio_ready = audio_buffer
                    .lock()
                    .map(|b| b.as_ref().map(|buf| buf.samples.len() >= prebuffer_audio_samples).unwrap_or(true))
                    .unwrap_or(false);

                if video_ready && audio_ready {
                    println!("[DEBUG] Prebuffering complete, marking as ready");
                    is_ready.store(true, Ordering::Relaxed);
                    prebuffering = false;
                }
            }

            // Wait for play signal only after prebuffering is done
            if !prebuffering {
                while !is_playing.load(Ordering::Relaxed) && !should_stop.load(Ordering::Relaxed) {
                    thread::sleep(std::time::Duration::from_millis(10));
                }
            }

            if frame_count == 0 && is_playing.load(Ordering::Relaxed) {
                println!("[DEBUG] is_playing became true, starting playback");
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
                            let (pts_us, pos_secs) = if let Some(tb) = video_time_base {
                                let secs = pts as f64 * tb.numerator() as f64 / tb.denominator() as f64;
                                let us = (secs * 1_000_000_000.0) as u64;
                                (us, secs)
                            } else {
                                (pts as u64, 0.0)
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
                                position_secs: pos_secs,
                            };

                            // Wait if frame queue is full to maintain sync
                            loop {
                                if let Ok(mut frames) = frame_queue.lock() {
                                    if frames.len() < 100 {
                                        frames.push_back(video_info);
                                        break;
                                    }
                                }
                                // Queue full, wait a bit
                                thread::sleep(std::time::Duration::from_millis(5));
                                if should_stop.load(Ordering::Relaxed) {
                                    break;
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
                        // Create resampler based on detected format (convert to F32 packed for cpal)
                        if resampler.is_none() {
                            let target_layout = if target_channels == 1 {
                                ffmpeg::channel_layout::ChannelLayout::MONO
                            } else {
                                ffmpeg::channel_layout::ChannelLayout::STEREO
                            };
                            resampler = Some(ffmpeg::software::resampling::Context::get(
                                decoded.format(),
                                decoded.channel_layout(),
                                decoded.rate(),
                                ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed),
                                target_layout,
                                target_sample_rate,
                            )?);
                            println!(
                                "[DEBUG] Created resampler: {:?} {} Hz -> F32 packed {} Hz",
                                decoded.format(),
                                decoded.rate(),
                                target_sample_rate
                            );
                        }

                        if let Some(ref mut resampler) = resampler {
                            let mut resampled = ffmpeg::util::frame::audio::Audio::empty();

                            // Run resampler and collect output
                            if resampler.run(&decoded, &mut resampled).is_ok() {
                                // Process output samples
                                let push_samples = |resampled: &ffmpeg::util::frame::audio::Audio,
                                                   audio_buffer: &Arc<Mutex<Option<AudioBuffer>>>,
                                                   should_stop: &Arc<AtomicBool>,
                                                   num_channels: usize| {
                                    if resampled.samples() == 0 {
                                        return;
                                    }
                                    let actual_samples = resampled.samples() * num_channels;
                                    let plane = resampled.plane::<f32>(0);

                                    let max_buffer_size = target_sample_rate as usize * num_channels * 2;
                                    loop {
                                        if let Ok(mut buffer_opt) = audio_buffer.lock() {
                                            if let Some(ref mut buffer) = *buffer_opt {
                                                if buffer.samples.len() < max_buffer_size {
                                                    for i in 0..actual_samples.min(plane.len()) {
                                                        buffer.samples.push_back(plane[i]);
                                                    }
                                                    break;
                                                }
                                            } else {
                                                break; // No buffer initialized
                                            }
                                        }
                                        thread::sleep(std::time::Duration::from_millis(5));
                                        if should_stop.load(Ordering::Relaxed) {
                                            break;
                                        }
                                    }
                                };

                                push_samples(&resampled, &audio_buffer, &should_stop, target_channels as usize);

                                // Flush any remaining samples from resampler delay buffer
                                if let Some(delay) = resampler.delay() {
                                    if delay.output > 0 {
                                        let mut flush_frame = ffmpeg::util::frame::audio::Audio::empty();
                                        if resampler.flush(&mut flush_frame).is_ok() {
                                            push_samples(&flush_frame, &audio_buffer, &should_stop, target_channels as usize);
                                        }
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
