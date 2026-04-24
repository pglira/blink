use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context as _, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use ffmpeg_next as ff;
use tracing::{debug, error, info, warn};

use crate::capture::Signal;
use crate::config::Config;
use crate::staging;
use crate::state::AppState;

/// One open MKV segment. Holds libav encoder + muxer state.
struct Segment {
    path: PathBuf,
    octx: ff::format::context::Output,
    encoder: ff::codec::encoder::Video,
    scaler: ff::software::scaling::Context,
    width: u32,
    height: u32,
    pts: i64,
    opened_at: Instant,
    // Encoder emits packet ts in 1/fps ticks, but MKV's muxer snaps the
    // stream time_base to milliseconds. We rescale on the way out.
    enc_tb: ff::Rational,
    stream_tb: ff::Rational,
}

impl Segment {
    fn open(path: PathBuf, w: u32, h: u32, fps: i32, cfg: &Config) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut octx = ff::format::output(&path)
            .with_context(|| format!("opening output {}", path.display()))?;

        // Capture whether the container wants a global header before we take
        // a mutable borrow on octx for add_stream.
        let needs_global_header = octx
            .format()
            .flags()
            .contains(ff::format::flag::Flags::GLOBAL_HEADER);

        let codec_name = codec_name_for(&cfg.video.codec)?;
        let codec = ff::codec::encoder::find_by_name(codec_name)
            .ok_or_else(|| anyhow!(
                "codec '{codec_name}' not available in your libavcodec build"
            ))?;

        let mut stream = octx.add_stream(codec)?;
        // Empty context — we'll bind the codec explicitly at open_as_with
        // time. `from_parameters(stream.parameters())` would hand back an
        // empty codec id at this point and libav rejects it in avcodec_open2.
        let mut enc = ff::codec::context::Context::new()
            .encoder()
            .video()?;

        enc.set_width(w);
        enc.set_height(h);
        enc.set_format(ff::format::Pixel::YUV420P);
        enc.set_time_base(ff::Rational::new(1, fps));
        enc.set_frame_rate(Some(ff::Rational::new(fps, 1)));
        // avcodec_alloc_context3(NULL) leaves AVCodecContext fields at legacy
        // defaults (qmin=2, qmax=31, max_qdiff=3, me_range=0, b_quant_factor=1.25,
        // me_subpel_quality=8) which libx264 forwards verbatim to x264; x264
        // then fingerprints that exact combination as "broken ffmpeg default
        // settings detected" and refuses to open. We override enough of them
        // to drop the score below x264's threshold.
        enc.set_gop(30);
        enc.set_max_b_frames(0);
        enc.set_qmin(0);
        enc.set_qmax(51);
        enc.set_me_range(16);
        enc.set_me_subpel_quality(7);
        enc.set_b_quant_factor(1.3);

        if needs_global_header {
            enc.set_flags(ff::codec::flag::Flags::GLOBAL_HEADER);
        }

        let mut opts = ff::Dictionary::new();
        opts.set("crf", &cfg.video.crf.to_string());
        opts.set("preset", "medium");

        let opened = enc
            .open_as_with(codec, opts)
            .context("opening video encoder")?;
        stream.set_parameters(&opened);
        stream.set_time_base(ff::Rational::new(1, fps));

        // flush_packets=1 pushes every packet to the OS file descriptor as soon
        // as libav writes it, instead of buffering in the AVIO context. Combined
        // with the fsync() after each frame this makes a hard shutdown lose at
        // most the single in-flight frame rather than up to an AVIO buffer worth.
        let mut mux_opts = ff::Dictionary::new();
        mux_opts.set("flush_packets", "1");
        octx.write_header_with(mux_opts)
            .context("writing MKV header")?;

        // write_header_with may renegotiate the stream's time_base (MKV
        // prefers ms precision). Capture the final value after the header
        // is written so rescale_ts targets the right denominator.
        let stream_tb = octx
            .stream(0)
            .ok_or_else(|| anyhow!("output has no stream 0"))?
            .time_base();
        let enc_tb = ff::Rational::new(1, fps);

        let scaler = ff::software::scaling::Context::get(
            ff::format::Pixel::RGB24, w, h,
            ff::format::Pixel::YUV420P, w, h,
            ff::software::scaling::Flags::BILINEAR,
        )?;

        info!(
            path = %path.display(),
            w, h, fps,
            codec = codec_name,
            crf = cfg.video.crf,
            "segment opened"
        );

        Ok(Self {
            path,
            octx,
            encoder: opened,
            scaler,
            width: w,
            height: h,
            pts: 0,
            opened_at: Instant::now(),
            enc_tb,
            stream_tb,
        })
    }

    fn append(&mut self, jpeg_path: &Path) -> Result<()> {
        let img = image::open(jpeg_path)
            .with_context(|| format!("decoding {}", jpeg_path.display()))?
            .to_rgb8();
        if img.width() != self.width || img.height() != self.height {
            bail!(
                "frame dims {}x{} differ from segment dims {}x{}",
                img.width(), img.height(), self.width, self.height
            );
        }

        // RGB24 source frame (respect libav's row stride).
        let mut src = ff::frame::Video::new(ff::format::Pixel::RGB24, self.width, self.height);
        {
            let stride = src.stride(0);
            let data = src.data_mut(0);
            let row_bytes = (self.width as usize) * 3;
            let raw = img.as_raw();
            for y in 0..self.height as usize {
                let dst_off = y * stride;
                let src_off = y * row_bytes;
                data[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&raw[src_off..src_off + row_bytes]);
            }
        }

        let mut dst = ff::frame::Video::new(ff::format::Pixel::YUV420P, self.width, self.height);
        self.scaler.run(&src, &mut dst)?;
        dst.set_pts(Some(self.pts));
        self.pts += 1;

        self.encoder.send_frame(&dst)?;
        self.drain()?;
        Ok(())
    }

    fn drain(&mut self) -> Result<()> {
        let mut pkt = ff::packet::Packet::empty();
        while self.encoder.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(0);
            pkt.rescale_ts(self.enc_tb, self.stream_tb);
            pkt.write_interleaved(&mut self.octx)?;
        }
        Ok(())
    }

    /// Best-effort sync of the segment file to disk. Done from a fresh FD
    /// since libav holds the write FD internally.
    fn sync(&self) {
        if let Ok(f) = fs::File::open(&self.path) {
            let _ = f.sync_all();
        }
    }

    fn close(mut self) -> Result<()> {
        // Flush encoder.
        let _ = self.encoder.send_eof();
        self.drain()?;
        self.octx.write_trailer()?;
        self.sync();
        info!(path = %self.path.display(), frames = self.pts, "segment closed");
        Ok(())
    }
}

fn codec_name_for(codec: &str) -> Result<&'static str> {
    Ok(match codec {
        "h264"         => "libx264",
        "h265" | "hevc" => "libx265",
        "av1"          => "libsvtav1",
        other          => bail!("unsupported codec in config: {other}"),
    })
}

pub fn run(cfg: Config, state: Arc<AppState>, rx: Receiver<Signal>) -> Result<()> {
    ff::init().context("ffmpeg init")?;

    let output_dir = cfg.output_dir();
    fs::create_dir_all(&output_dir)?;
    let staging_dir = cfg.staging_dir();

    // Drain any frames left behind by a previous run into a recovery segment.
    recover(&cfg, &staging_dir, &output_dir)?;

    let segment_cap = Duration::from_secs(
        cfg.video.segment_minutes.saturating_mul(60).max(60),
    );
    let fps: i32 = cfg.video.fps.max(1) as i32;

    let mut current: Option<Segment> = None;

    loop {
        if state.shutting_down.load(Ordering::SeqCst) {
            break;
        }

        // Wake on a new frame, or periodically to poll staging + check shutdown.
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Signal::FrameReady) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let frames = match staging::pending_frames(&staging_dir) {
            Ok(v) => v,
            Err(e) => { error!("reading staging dir: {e:#}"); continue; }
        };

        for frame_path in frames {
            match handle_frame(&cfg, &output_dir, &mut current, &frame_path, fps, segment_cap) {
                Ok(()) => {
                    if let Some(seg) = current.as_ref() { seg.sync(); }
                    if !cfg.staging.keep_after_encode {
                        if let Err(e) = fs::remove_file(&frame_path) {
                            warn!(path = %frame_path.display(), "removing staged frame: {e:#}");
                        }
                    }
                    debug!(path = %frame_path.display(), "frame encoded");
                }
                Err(e) => {
                    error!(path = %frame_path.display(), "encode failed: {e:#}");
                    // leave the JPEG in staging; next iteration may retry
                }
            }
        }
    }

    if let Some(seg) = current.take() {
        if let Err(e) = seg.close() { warn!("closing final segment: {e:#}"); }
    }
    info!("encoder thread exiting");
    Ok(())
}

fn handle_frame(
    cfg: &Config,
    output_dir: &Path,
    current: &mut Option<Segment>,
    frame_path: &Path,
    fps: i32,
    segment_cap: Duration,
) -> Result<()> {
    let (w, h) = read_jpeg_dims(frame_path)?;

    let need_rotate = match current.as_ref() {
        None => true,
        Some(seg) => {
            seg.width != w || seg.height != h || seg.opened_at.elapsed() >= segment_cap
        }
    };

    if need_rotate {
        if let Some(seg) = current.take() {
            if let Err(e) = seg.close() { warn!("closing previous segment: {e:#}"); }
        }
        let path = output_dir.join(segment_filename("blink", w, h));
        *current = Some(Segment::open(path, w, h, fps, cfg)?);
    }

    current
        .as_mut()
        .expect("segment was just opened")
        .append(frame_path)
}

fn read_jpeg_dims(path: &Path) -> Result<(u32, u32)> {
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    Ok(reader.into_dimensions()?)
}

fn segment_filename(prefix: &str, w: u32, h: u32) -> String {
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    format!("{prefix}_{ts}_{w}x{h}.mkv")
}

/// On startup: encode any JPEGs orphaned in staging into a recovery segment.
fn recover(cfg: &Config, staging_dir: &Path, output_dir: &Path) -> Result<()> {
    let frames = staging::pending_frames(staging_dir)?;
    if frames.is_empty() {
        return Ok(());
    }
    info!(count = frames.len(), "recovering orphan frames");

    let fps: i32 = cfg.video.fps.max(1) as i32;
    let mut seg: Option<Segment> = None;

    for fp in frames {
        let (w, h) = match read_jpeg_dims(&fp) {
            Ok(d) => d,
            Err(e) => { warn!(path = %fp.display(), "unreadable JPEG: {e:#}"); continue; }
        };
        let reopen = match seg.as_ref() {
            None => true,
            Some(s) => s.width != w || s.height != h,
        };
        if reopen {
            if let Some(s) = seg.take() { let _ = s.close(); }
            let path = output_dir.join(segment_filename("recovery", w, h));
            seg = Some(Segment::open(path, w, h, fps, cfg)?);
        }
        if let Some(s) = seg.as_mut() {
            if let Err(e) = s.append(&fp) {
                warn!(path = %fp.display(), "recovery append failed: {e:#}");
                continue;
            }
            s.sync();
        }
        if !cfg.staging.keep_after_encode {
            let _ = fs::remove_file(&fp);
        }
    }
    if let Some(s) = seg.take() {
        if let Err(e) = s.close() { warn!("closing recovery segment: {e:#}"); }
    }
    Ok(())
}
