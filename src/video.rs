use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{traits::*, HeapRb};

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

/// Audio format info extracted from video
#[derive(Clone, Debug)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// Lock-free audio ring buffer for the audio callback
pub type AudioProducer = ringbuf::HeapProd<f32>;
pub type AudioConsumer = ringbuf::HeapCons<f32>;

pub struct FfmpegPlayer {
    pub frame: Arc<Mutex<VecDeque<VideoInfo>>>,
    pub audio_format: Arc<Mutex<Option<AudioFormat>>>,
    pub previous_pts: Arc<Mutex<u64>>,
    pub duration: Arc<Mutex<f64>>,
    pub current_position: Arc<Mutex<f64>>,
    
    /// Atomic counter for audio buffer size (for prebuffering check)
    pub audio_buffer_len: Arc<AtomicUsize>,

    pub is_playing: Arc<AtomicBool>,
    pub should_stop: Arc<AtomicBool>,
    pub is_ready: Arc<AtomicBool>,

    uri: String,
}

impl Clone for FfmpegPlayer {
    fn clone(&self) -> Self {
        Self {
            frame: Arc::clone(&self.frame),
            audio_format: Arc::clone(&self.audio_format),
            previous_pts: Arc::clone(&self.previous_pts),
            duration: Arc::clone(&self.duration),
            current_position: Arc::clone(&self.current_position),
            audio_buffer_len: Arc::clone(&self.audio_buffer_len),
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
            audio_format: Arc::new(Mutex::new(None)),
            previous_pts: Arc::new(Mutex::new(0)),
            duration: Arc::new(Mutex::new(0.0)),
            current_position: Arc::new(Mutex::new(0.0)),
            audio_buffer_len: Arc::new(AtomicUsize::new(0)),
            is_playing: Arc::new(AtomicBool::new(false)),
            should_stop: Arc::new(AtomicBool::new(false)),
            is_ready: Arc::new(AtomicBool::new(false)),
            uri: uri.to_string(),
        }
    }

    pub fn play(&self) {
        println!("[DEBUG] FfmpegPlayer::play() called");
        self.is_playing.store(true, Ordering::SeqCst);
        println!("[DEBUG] is_playing set to true");
    }

    pub fn pause(&self) {
        println!("[DEBUG] FfmpegPlayer::pause() called");
        self.is_playing.store(false, Ordering::SeqCst);
    }

    pub fn destroy(&self) {
        self.should_stop.store(true, Ordering::SeqCst);
        self.is_playing.store(false, Ordering::SeqCst);
    }

    pub fn start(&mut self) {
        let frame_queue = Arc::clone(&self.frame);
        let audio_format = Arc::clone(&self.audio_format);
        let audio_buffer_len = Arc::clone(&self.audio_buffer_len);
        let is_playing = Arc::clone(&self.is_playing);
        let should_stop = Arc::clone(&self.should_stop);
        let is_ready = Arc::clone(&self.is_ready);
        let duration = Arc::clone(&self.duration);
        let uri = self.uri.clone();

        thread::spawn(move || {
            if let Err(e) = Self::decode_loop(
                &uri,
                frame_queue,
                audio_format,
                audio_buffer_len,
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
        is_playing: Arc<AtomicBool>,
        audio_buffer_len: Arc<AtomicUsize>,
    ) -> Option<(cpal::Stream, AudioProducer, u32, u16)> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;

        // Get the default output config to use system's preferred sample rate
        let default_config = device.default_output_config().ok()?;
        let system_sample_rate = default_config.sample_rate().0;
        let system_channels = default_config.channels().min(2); // Limit to stereo

        println!(
            "[DEBUG] System audio format: {} Hz, {} channels",
            system_sample_rate, system_channels
        );

        // Create a large ring buffer for audio samples (3 seconds of audio)
        let buffer_size = system_sample_rate as usize * system_channels as usize * 3;
        let rb = HeapRb::<f32>::new(buffer_size);
        let (producer, mut consumer) = rb.split();

        // Track last samples for smooth interpolation when buffer underruns
        let mut last_left = 0.0f32;
        let mut last_right = 0.0f32;
        let mut underrun_fade = 1.0f32;
        let channels = system_channels as usize;

        let config = cpal::StreamConfig {
            channels: system_channels,
            sample_rate: cpal::SampleRate(system_sample_rate),
            buffer_size: cpal::BufferSize::Default, // Let the system choose optimal buffer size
        };

        let audio_buffer_len_clone = audio_buffer_len.clone();

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Always update the buffer length for prebuffering check
                    let available = consumer.occupied_len();
                    audio_buffer_len_clone.store(available, Ordering::Relaxed);
                    
                    // Check if playing
                    if !is_playing.load(Ordering::Relaxed) {
                        // Fade out smoothly when paused
                        for sample in data.iter_mut() {
                            last_left *= 0.95;
                            last_right *= 0.95;
                            *sample = last_left;
                        }
                        return;
                    }
                    
                    // Use pop_slice for more efficient bulk reading
                    let read = consumer.pop_slice(data);
                    
                    // Update last samples from what we read
                    if read >= channels {
                        if channels == 2 {
                            last_left = data[read - 2];
                            last_right = data[read - 1];
                        } else {
                            last_left = data[read - 1];
                            last_right = last_left;
                        }
                        underrun_fade = 1.0;
                    }
                    
                    // Handle underrun for remaining samples
                    if read < data.len() {
                        for i in read..data.len() {
                            underrun_fade *= 0.98;
                            if channels == 2 {
                                if i % 2 == 0 {
                                    data[i] = last_left * underrun_fade;
                                } else {
                                    data[i] = last_right * underrun_fade;
                                }
                            } else {
                                data[i] = last_left * underrun_fade;
                            }
                        }
                    }
                },
                |err| eprintln!("Audio stream error: {}", err),
                None,
            )
            .ok()?;

        Some((stream, producer, system_sample_rate, system_channels))
    }

    fn decode_loop(
        uri: &str,
        frame_queue: Arc<Mutex<VecDeque<VideoInfo>>>,
        audio_format: Arc<Mutex<Option<AudioFormat>>>,
        audio_buffer_len: Arc<AtomicUsize>,
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

        // Read audio format from the decoder
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

        // Setup audio output and get the producer for the ring buffer
        let (audio_stream, mut audio_producer, target_sample_rate, target_channels) = 
            if detected_format.is_some() {
                if let Some((stream, producer, rate, channels)) = Self::setup_audio_output(
                    Arc::clone(&is_playing),
                    Arc::clone(&audio_buffer_len),
                ) {
                    if let Ok(mut format_lock) = audio_format.lock() {
                        *format_lock = Some(AudioFormat {
                            sample_rate: rate,
                            channels,
                        });
                    }
                    let _ = stream.play();
                    (Some(stream), Some(producer), rate, channels)
                } else {
                    (None, None, 44100, 2)
                }
            } else {
                (None, None, 44100, 2)
            };

        // Keep audio stream alive
        let _audio_stream = audio_stream;

        let video_time_base = video_stream_index
            .and_then(|idx| ictx.stream(idx))
            .map(|s| s.time_base());

        let mut scaler: Option<ScalingContext> = None;
        let mut resampler: Option<ffmpeg::software::resampling::Context> = None;

        println!("[DEBUG] Starting packet loop, waiting for is_playing...");
        let mut frame_count = 0u64;
        let mut prebuffering = true;
        const PREBUFFER_FRAMES: usize = 30; // Prebuffer ~1 second of video
        // Prebuffer 0.5 second of audio for smoother playback
        let prebuffer_audio_samples = (target_sample_rate as usize) * (target_channels as usize) / 2;
        let mut audio_samples_pushed: usize = 0;
        
        println!("[DEBUG] Prebuffer targets: {} video frames, {} audio samples", PREBUFFER_FRAMES, prebuffer_audio_samples);

        for (stream, packet) in ictx.packets() {
            if should_stop.load(Ordering::Relaxed) {
                println!("[DEBUG] should_stop is true, breaking");
                break;
            }

            // During prebuffering, decode without waiting for is_playing
            if prebuffering {
                let video_ready = frame_queue.lock().map(|f| f.len() >= PREBUFFER_FRAMES).unwrap_or(false);
                let audio_ready = audio_samples_pushed >= prebuffer_audio_samples;
                
                if frame_count % 10 == 0 {
                    let video_len = frame_queue.lock().map(|f| f.len()).unwrap_or(0);
                    println!("[DEBUG] Prebuffering: video {}/{}, audio {}/{}", 
                             video_len, PREBUFFER_FRAMES, audio_samples_pushed, prebuffer_audio_samples);
                }

                if video_ready && audio_ready {
                    println!("[DEBUG] Prebuffering complete, marking as ready");
                    is_ready.store(true, Ordering::SeqCst);
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
                    if let Some(ref mut producer) = audio_producer {
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
                                    "[DEBUG] Created resampler: {:?} {} Hz -> F32 packed {} Hz, {} channels",
                                    decoded.format(),
                                    decoded.rate(),
                                    target_sample_rate,
                                    target_channels
                                );
                            }

                            if let Some(ref mut resampler) = resampler {
                                let mut resampled = ffmpeg::util::frame::audio::Audio::empty();

                                // Run resampler and collect output
                                if resampler.run(&decoded, &mut resampled).is_ok() && resampled.samples() > 0 {
                                    // For packed format, samples() returns number of samples per channel
                                    // Total float samples = samples_per_channel * num_channels
                                    let samples_per_channel = resampled.samples();
                                    let total_floats = samples_per_channel * target_channels as usize;
                                    
                                    // Get raw data and convert to f32 slice
                                    let data = resampled.data(0);
                                    let float_data: &[f32] = unsafe {
                                        std::slice::from_raw_parts(
                                            data.as_ptr() as *const f32,
                                            total_floats.min(data.len() / 4)
                                        )
                                    };
                                    
                                    let samples_to_push = float_data.len();
                                    
                                    // Push samples to ring buffer, waiting if full
                                    let mut pushed = 0;
                                    while pushed < samples_to_push {
                                        let remaining = &float_data[pushed..samples_to_push];
                                        let n = producer.push_slice(remaining);
                                        pushed += n;
                                        audio_samples_pushed += n;
                                        
                                        if n == 0 {
                                            // Buffer full, wait a bit
                                            thread::sleep(std::time::Duration::from_millis(2));
                                            if should_stop.load(Ordering::Relaxed) {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Flush resampler at end of stream
        if let Some(ref mut resampler) = resampler {
            if let Some(ref mut producer) = audio_producer {
                let mut flush_frame = ffmpeg::util::frame::audio::Audio::empty();
                while resampler.flush(&mut flush_frame).is_ok() && flush_frame.samples() > 0 {
                    let num_samples = flush_frame.samples() * target_channels as usize;
                    let plane = flush_frame.plane::<f32>(0);
                    let samples_to_push = num_samples.min(plane.len());
                    let _ = producer.push_slice(&plane[..samples_to_push]);
                    flush_frame = ffmpeg::util::frame::audio::Audio::empty();
                }
            }
        }

        println!("EOS");
        Ok(())
    }
}
