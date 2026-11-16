//! UnityEngine.Video.VideoPlayer's decoding: the game's .webm clips (VP8 in Matroska), read from the user's own install,
//! decoded here to RGBA frames. The player shows the frame whose timestamp is the last at or before the clock.

use matroska_demuxer::{Frame, MatroskaFile, TrackType};
use std::io::Cursor;

pub struct Clip {
    bytes: std::sync::Arc<[u8]>,
    mkv: MatroskaFile<Cursor<std::sync::Arc<[u8]>>>,
    track: u64,
    /// Seconds per timestamp unit.
    scale: f64,
    vp8: vp8_decoder::Decoder,
    pub width: u32,
    pub height: u32,
    /// The shown frame: its time, and RGBA rows top first.
    shown: Option<(f64, Vec<u8>)>,
    /// The next video packet, read but not yet due.
    pending: Option<(f64, Vec<u8>)>,
    ended: bool,
    serial: u64,
}

impl Clip {
    pub fn new(bytes: Vec<u8>) -> Result<Self, String> {
        let bytes: std::sync::Arc<[u8]> = bytes.into();
        let mkv = MatroskaFile::open(Cursor::new(bytes.clone())).map_err(|e| e.to_string())?;
        let t = mkv.tracks().iter().find(|t| t.track_type() == TrackType::Video).ok_or("no video track")?;
        if t.codec_id() != "V_VP8" {
            unimplemented!("mod-only: VideoPlayer: video codec {}", t.codec_id());
        }
        let v = t.video().ok_or("video track without video settings")?;
        let (track, width, height) = (t.track_number().get(), v.pixel_width().get() as u32, v.pixel_height().get() as u32);
        let scale = mkv.info().timestamp_scale().get() as f64 / 1e9;
        Ok(Self { bytes, mkv, track, scale, vp8: vp8_decoder::Decoder::new(), width, height, shown: None, pending: None, ended: false, serial: 0 })
    }

    fn rewind(&mut self) {
        self.mkv = MatroskaFile::open(Cursor::new(self.bytes.clone())).expect("a clip that opened once");
        self.vp8 = vp8_decoder::Decoder::new();
        (self.shown, self.pending, self.ended) = (None, None, false);
    }

    /// The video packet after the last one read.
    fn packet(&mut self) -> Option<(f64, Vec<u8>)> {
        let mut f = Frame::default();
        while self.mkv.next_frame(&mut f).ok()? {
            if f.track == self.track {
                return Some((f.timestamp as f64 * self.scale, std::mem::take(&mut f.data)));
            }
        }
        self.ended = true;
        None
    }

    /// The frame shown at `t` seconds (the first one before any is due): RGBA, top row first, with how many frames have been shown.
    pub fn frame_at(&mut self, t: f64) -> Option<(u64, &[u8])> {
        if self.shown.as_ref().is_some_and(|s| s.0 > t) {
            self.rewind();
        }
        loop {
            let next = match self.pending.take() {
                Some(p) => p,
                None if self.ended => break,
                None => match self.packet() {
                    Some(p) => p,
                    None => break,
                },
            };
            if next.0 > t && self.shown.is_some() {
                self.pending = Some(next);
                break;
            }
            let rgba = match self.vp8.decode(&next.1) {
                Ok(Some(p)) => Some(i420_to_rgba(&p)),
                Ok(None) => None,
                Err(e) => panic!("VideoPlayer: VP8 frame at {}s: {e:?}", next.0),
            };
            if let Some(rgba) = rgba {
                self.shown = Some((next.0, rgba));
                self.serial += 1;
            }
        }
        let serial = self.serial;
        self.shown.as_ref().map(|s| (serial, s.1.as_slice()))
    }
}

/// BT.601 limited-range YUV to RGB (the VP8 colour space), 8-bit fixed point.
fn i420_to_rgba(p: &vp8_decoder::Picture) -> Vec<u8> {
    let (w, h) = (p.width as usize, p.height as usize);
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let l = p.y[y * p.y_stride + x] as i32;
            let u = p.u[(y / 2) * p.uv_stride + x / 2] as i32 - 128;
            let v = p.v[(y / 2) * p.uv_stride + x / 2] as i32 - 128;
            let c = (l - 16) * 298;
            let c8 = |n: i32| ((n + 128) >> 8).clamp(0, 255) as u8;
            out.extend([c8(c + 409 * v), c8(c - 100 * u - 208 * v), c8(c + 516 * u), 255]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::assets::components::VideoClip;

    /// Every VideoClip in the install reads to its serialized size, its webm matches AssetRipper's export, and it decodes
    /// its first, a middle and a late frame.
    #[test]
    fn install_clips_decode() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let data = game.join("People Playground_Data");
        let mut n = 0;
        for e in std::fs::read_dir(&data).unwrap() {
            let name = e.unwrap().file_name().into_string().unwrap();
            if !(name.ends_with(".assets") || name.starts_with("level")) {
                continue;
            }
            let f = crate::assets::serialized::SerializedFile::open(&data.join(&name)).unwrap();
            for o in f.objects.iter().filter(|o| o.class_id == 329) {
                let c = VideoClip::read(f.data(o)).unwrap();
                let bytes = c.webm(&data).unwrap();
                if let Ok(exported) = std::fs::read(format!("unity-export/ExportedProject/Assets/VideoClip/{}.webm", c.name)) {
                    assert!(exported == bytes, "{}: differs from the export", c.name);
                }
                let mut clip = super::Clip::new(bytes).unwrap();
                eprintln!("{}: {}x{} (clip {}x{}) {} frames at {} = {}s", c.name, clip.width, clip.height, c.width, c.height, c.frame_count, c.frame_rate, c.length());
                for t in [0.0, c.length() / 2.0, c.length() - 0.1, 0.0] {
                    let (_, px) = clip.frame_at(t).unwrap();
                    assert_eq!(px.len(), (clip.width * clip.height * 4) as usize);
                }
                n += 1;
            }
        }
        assert!(n >= 4, "{n} clips");
    }
}
