//! AudioClip sample data: the FSB5 bank an AudioClip's m_Resource points at in a .resource file, decoded to 16-bit
//! PCM as FMOD decodes it. PPG's banks hold one sample each in one of three codecs: Vorbis (15), FMOD's IMA ADPCM
//! (7) and PCM16 (2).
//!
//! FSB5 Vorbis keeps the audio packets but drops the three Vorbis headers; the sample header has only the CRC32 of
//! the setup header libvorbisenc wrote. The identification header is rebuilt from the bank's channels and rate
//! (python-fsb5's rebuild_id_header), and the setup header, with its block sizes, comes from
//! `fsb_vorbis_setups.bin`: libvorbisenc's setup packet for each CRC the game's banks use (crc32 u32, blocksize
//! byte, length u32, packet), each one checked against its CRC when it is looked up.

use super::classes::AudioClip;
use std::path::Path;

/// Decoded samples, interleaved.
#[derive(Clone, Debug, Default)]
pub struct Pcm {
    pub channels: usize,
    pub frequency: u32,
    pub data: Vec<i16>,
}

impl Pcm {
    /// Sample frames (AudioClip.samples).
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels.max(1)
    }
}

static SETUPS: &[u8] = include_bytes!("fsb_vorbis_setups.bin");

/// One FSB5 bank's only sample.
struct Fsb {
    mode: u32,
    frequency: u32,
    channels: usize,
    samples: usize,
    vorbis_crc: Option<u32>,
    data: Vec<u8>,
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(b.get(at..at + 4).ok_or("FSB5: short header")?.try_into().unwrap()))
}

fn fsb5(b: &[u8]) -> Result<Fsb, String> {
    if b.get(..4) != Some(b"FSB5") {
        return Err("no FSB5 bank at the clip's offset".into());
    }
    let (version, count, sample_headers, names, data_size, mode) = (u32_at(b, 4)?, u32_at(b, 8)?, u32_at(b, 12)?, u32_at(b, 16)?, u32_at(b, 20)?, u32_at(b, 24)?);
    if count != 1 {
        unimplemented!("mod-only: an FSB5 bank with {count} samples");
    }
    // magic, version, numSamples, sampleHeadersSize, nameTableSize, dataSize, mode, zero[8], hash[16], dummy[8];
    // version 0 has one more u32.
    let header = if version == 0 { 64 } else { 60 };
    let raw = u64::from_le_bytes(b.get(header..header + 8).ok_or("FSB5: short sample header")?.try_into().unwrap());
    let mut frequency = match (raw >> 1) & 15 {
        0 => 4000,
        1 => 8000,
        2 => 11000,
        3 => 11025,
        4 => 16000,
        5 => 22050,
        6 => 24000,
        7 => 32000,
        8 => 44100,
        9 => 48000,
        10 => 96000,
        _ => 0,
    };
    let mut channels = ((raw >> 5) & 1) as usize + 1;
    let offset = ((raw >> 6) & 0x0fff_ffff) as usize * 16;
    let samples = ((raw >> 34) & 0x3fff_ffff) as usize;
    let (mut at, mut next, mut vorbis_crc) = (header + 8, raw & 1 != 0, None);
    while next {
        let c = u32_at(b, at)?;
        at += 4;
        next = c & 1 != 0;
        let size = ((c >> 1) & 0xff_ffff) as usize;
        match c >> 25 {
            1 => channels = *b.get(at).ok_or("FSB5: short chunk")? as usize,
            2 => frequency = u32_at(b, at)?,
            11 => vorbis_crc = Some(u32_at(b, at)?),
            _ => {}
        }
        at += size;
    }
    if frequency == 0 {
        return Err("FSB5: no frequency".into());
    }
    let start = header + sample_headers as usize + names as usize + offset;
    let data = b.get(start..header + sample_headers as usize + names as usize + data_size as usize).ok_or("FSB5: data past the bank")?.to_vec();
    Ok(Fsb { mode, frequency, channels, samples, vorbis_crc, data })
}

/// The setup packet and the (short, long) block size exponents for a setup header CRC.
fn setup(crc: u32) -> Result<(&'static [u8], (u8, u8)), String> {
    let mut at = 0;
    while at < SETUPS.len() {
        let key = u32::from_le_bytes(SETUPS[at..at + 4].try_into().unwrap());
        let sizes = SETUPS[at + 4];
        let n = u32::from_le_bytes(SETUPS[at + 5..at + 9].try_into().unwrap()) as usize;
        let packet = &SETUPS[at + 9..at + 9 + n];
        if key == crc {
            if crc32(packet) != crc {
                return Err(format!("the Vorbis setup header for CRC {crc:08x} does not match it"));
            }
            return Ok((packet, (sizes & 15, sizes >> 4)));
        }
        at += 9 + n;
    }
    unimplemented!("mod-only: an FSB5 Vorbis bank with setup header CRC {crc:08x} (not one of the game's)")
}

fn crc32(b: &[u8]) -> u32 {
    let mut c = !0u32;
    for &x in b {
        c ^= x as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn vorbis(f: &Fsb) -> Result<Vec<i16>, String> {
    use lewton::audio::{read_audio_packet_generic, PreviousWindowRight};
    use lewton::samples::InterleavedSamples;
    let (packet, sizes) = setup(f.vorbis_crc.ok_or("FSB5 Vorbis without a VORBISDATA chunk")?)?;
    // The identification header: type 1, "vorbis", version 0, channels, rate, three zero bitrates, block sizes, framing.
    let mut id = vec![1];
    id.extend_from_slice(b"vorbis");
    id.extend_from_slice(&0u32.to_le_bytes());
    id.push(f.channels as u8);
    id.extend_from_slice(&f.frequency.to_le_bytes());
    id.extend_from_slice(&[0; 12]);
    id.push(sizes.0 | sizes.1 << 4);
    id.push(1);
    let ident = lewton::header::read_header_ident(&id).map_err(|e| format!("Vorbis ident: {e:?}"))?;
    let setup = lewton::header::read_header_setup(packet, f.channels as u8, sizes).map_err(|e| format!("Vorbis setup: {e:?}"))?;
    let mut pwr = PreviousWindowRight::new();
    let mut out = Vec::with_capacity(f.samples * f.channels);
    // The packets, each after its u16 size; a zero size (or the end) closes the stream.
    let mut at = 0;
    while at + 2 <= f.data.len() {
        let n = u16::from_le_bytes([f.data[at], f.data[at + 1]]) as usize;
        if n == 0 {
            break;
        }
        let p = f.data.get(at + 2..at + 2 + n).ok_or("FSB5 Vorbis: a packet past the data")?;
        at += 2 + n;
        let s: InterleavedSamples<i16> = read_audio_packet_generic(&ident, &setup, p, &mut pwr).map_err(|e| format!("Vorbis packet: {e:?}"))?;
        out.extend_from_slice(&s.samples);
    }
    Ok(out)
}

const IMA_STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371,
    408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132,
    7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const IMA_INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// FMOD's IMA ADPCM: 36-byte blocks per channel of 64 sample frames. Each channel's block starts with a 4-byte
/// header (the first sample, the step index); the nibbles follow, low first, in 4-byte groups that alternate
/// between the channels. The predictor wraps at 16 bits instead of clamping (checked against the exported clips).
fn ima_adpcm(f: &Fsb) -> Vec<i16> {
    let ch = f.channels;
    let block = 36 * ch;
    let mut out = Vec::with_capacity(f.samples * ch);
    for b in f.data.chunks_exact(block) {
        let start = out.len();
        out.resize(start + 64 * ch, 0);
        for c in 0..ch {
            let mut h = i16::from_le_bytes([b[4 * c], b[4 * c + 1]]) as i32;
            let mut index = (b[4 * c + 2] as i32).min(88);
            out[start + c] = h as i16;
            for i in 1..64 {
                let k = i - 1;
                let byte = b[4 * ch + ((k / 8) * ch + c) * 4 + (k % 8) / 2];
                let n = if k % 2 == 0 { byte & 15 } else { byte >> 4 } as i32;
                let step = IMA_STEPS[index as usize];
                let mut delta = step >> 3;
                if n & 1 != 0 {
                    delta += step >> 2;
                }
                if n & 2 != 0 {
                    delta += step >> 1;
                }
                if n & 4 != 0 {
                    delta += step;
                }
                h = if n & 8 != 0 { h - delta } else { h + delta } as i16 as i32;
                index = (index + IMA_INDEX[(n & 7) as usize]).clamp(0, 88);
                out[start + i * ch + c] = h as i16;
            }
        }
    }
    out
}

/// The clip's samples. `data_dir` holds the .resource files.
pub fn decode(clip: &AudioClip, data_dir: &Path) -> Result<Pcm, String> {
    let (file, offset, size) = &clip.resource;
    if file.is_empty() {
        unimplemented!("mod-only: AudioClip {}: sample data inside the serialized file (no m_Resource)", clip.name);
    }
    let path = data_dir.join(file);
    let bytes = {
        use std::io::{Read, Seek, SeekFrom};
        let mut fh = std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        fh.seek(SeekFrom::Start(*offset)).map_err(|e| e.to_string())?;
        let mut b = vec![0; *size as usize];
        fh.read_exact(&mut b).map_err(|e| format!("{}: {e}", path.display()))?;
        b
    };
    let f = fsb5(&bytes)?;
    let mut data = match f.mode {
        2 => f.data.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect(),
        7 => ima_adpcm(&f),
        15 => vorbis(&f)?,
        m => unimplemented!("mod-only: AudioClip {}: FSB5 codec {m}", clip.name),
    };
    data.resize(f.samples * f.channels, 0);
    Ok(Pcm { channels: f.channels, frequency: f.frequency, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every AudioClip of the build decodes to the samples AssetRipper exported (unity-export's AudioClip folder):
    /// the .wav clips (PCM16 and IMA ADPCM, which FMOD decoded for it) sample for sample, the .ogg clips (rebuilt
    /// Ogg streams) to what lewton decodes from them, over the clip's length.
    #[test]
    fn clips_match_the_export() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let export = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("unity-export/ExportedProject/Assets/AudioClip");
        if !export.is_dir() {
            return;
        }
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let (mut checked, mut seen) = ([0usize; 2], std::collections::HashSet::new());
        for f in &db.files {
            for o in f.objects.iter().filter(|o| o.class_id == 83) {
                let clip = AudioClip::read(f.data(o)).unwrap();
                let pcm = decode(&clip, &db.data_dir).unwrap_or_else(|e| panic!("{}: {e}", clip.name));
                assert_eq!(pcm.channels as i32, clip.channels, "{}", clip.name);
                assert_eq!(pcm.frequency as i32, clip.frequency, "{}", clip.name);
                assert!((pcm.frames() as f32 / pcm.frequency as f32 - clip.length).abs() < 1e-3, "{}: {} frames, length {}", clip.name, pcm.frames(), clip.length);
                // Names repeat (AssetRipper numbers the duplicates); only the first of each name is checked.
                if !seen.insert(clip.name.clone()) {
                    continue;
                }
                let wav = export.join(format!("{}.wav", clip.name));
                let ogg = export.join(format!("{}.ogg", clip.name));
                let theirs: Vec<i16> = if wav.is_file() {
                    std::fs::read(&wav).unwrap()[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect()
                } else if ogg.is_file() {
                    let mut r = lewton::inside_ogg::OggStreamReader::new(std::fs::File::open(&ogg).unwrap()).unwrap();
                    let mut v = Vec::new();
                    while let Some(p) = r.read_dec_packet_itl().unwrap() {
                        v.extend(p);
                    }
                    v
                } else {
                    continue;
                };
                // A same-named export of another clip has another length; skip it.
                if theirs.len() < pcm.data.len() {
                    continue;
                }
                let first = pcm.data.iter().zip(&theirs).position(|(a, b)| a != b);
                assert_eq!(first, None, "{}: first differing sample", clip.name);
                checked[wav.is_file() as usize] += 1;
            }
        }
        assert!(checked[0] > 700 && checked[1] > 150, "checked {checked:?}");
    }
}

/// Utils.FileToAudioClip's samples: a mono 16-bit clip at the file's rate, through NAudio's readers. A stereo
/// stream goes through StereoToMonoProvider16 (each pair `(short)(l * 0.5f + r * 0.5f)`), which wants 16-bit PCM;
/// a mono one is read as raw bytes and reinterpreted as shorts whatever its format. Err is any exception there
/// (JukeboxSongLoader.ExtractData logs it and skips the file).
pub fn file_to_pcm(path: &Path) -> Result<Pcm, String> {
    let (bytes, channels, frequency, pcm16, block_align) = match path.extension().and_then(|e| e.to_str()) {
        Some("wav") => wav(&std::fs::read(path).map_err(|e| e.to_string())?)?,
        Some("mp3") => mp3(path)?,
        // DecodeUnknownFile: NAudio's AudioFileReader (Media Foundation); only ModAPI.LoadSound reaches it.
        _ => unimplemented!("mod-only: Utils.FileToAudioClip of {} (AudioFileReader)", path.display()),
    };
    let short = |b: &[u8], i: usize| b.get(2 * i..2 * i + 2).map_or(0, |x| i16::from_le_bytes([x[0], x[1]]));
    let data: Vec<i16> = if channels == 1 {
        // stream.Read(array, 0, Length): WaveFileReader wants whole blocks; Buffer.BlockCopy of an odd count throws.
        if bytes.len() % block_align != 0 || bytes.len() % 2 != 0 {
            return Err("ArgumentException".into());
        }
        (0..bytes.len() / 2).map(|i| short(&bytes, i)).collect()
    } else {
        if channels != 2 || !pcm16 {
            return Err("ArgumentException: StereoToMonoProvider16 wants 16-bit PCM stereo".into());
        }
        // Read(array, 0, Length) asks the source for 2 * Length bytes.
        if (2 * bytes.len()) % block_align != 0 {
            return Err("ArgumentException".into());
        }
        // It returns half the bytes read; Buffer.BlockCopy of an odd count throws.
        let shorts = bytes.len() / 2;
        if shorts % 2 != 0 {
            return Err("ArgumentException".into());
        }
        (0..shorts / 2).map(|k| (short(&bytes, 2 * k) as f32 * 0.5 + short(&bytes, 2 * k + 1) as f32 * 0.5) as i16).collect()
    };
    // AudioClip.Create throws for no samples or no rate.
    if data.is_empty() || frequency == 0 {
        return Err("ArgumentException: AudioClip.Create".into());
    }
    Ok(Pcm { channels: 1, frequency, data })
}

/// WaveFileReader: the data chunk's bytes (as far as the file has them), channels, rate, PCM 16-bit, BlockAlign.
fn wav(b: &[u8]) -> Result<(Vec<u8>, usize, u32, bool, usize), String> {
    let u16_at = |at: usize| b.get(at..at + 2).map(|x| u16::from_le_bytes([x[0], x[1]])).ok_or("EndOfStream");
    if b.get(0..4) != Some(b"RIFF") || b.get(8..12) != Some(b"WAVE") {
        return Err("FormatException: Not a WAVE file".into());
    }
    let (mut at, mut fmt, mut data) = (12, None, None);
    while at + 8 <= b.len() {
        let (id, len) = (&b[at..at + 4], u32_at(b, at + 4)? as usize);
        at += 8;
        match id {
            b"data" => data = Some((at, len)),
            b"fmt " => fmt = Some((u16_at(at)?, u16_at(at + 2)? as usize, u32_at(b, at + 4)?, u16_at(at + 12)? as usize, u16_at(at + 14)?)),
            _ if len > b.len() - at => return Err("InvalidDataException: chunk past the end".into()),
            _ => {}
        }
        at += len;
        if len % 2 == 1 && b.get(at) == Some(&0) {
            at += 1;
        }
    }
    let (tag, channels, frequency, block_align, bits) = fmt.ok_or("FormatException: Invalid WAV file - No fmt chunk found")?;
    let (start, len) = data.ok_or("FormatException: Invalid WAV file - No data chunk found")?;
    let end = start.saturating_add(len).min(b.len());
    Ok((b[start.min(end)..end].to_vec(), channels, frequency, tag == 1 && bits == 16, block_align.max(1)))
}

/// Mp3FileReader: every frame decoded to 16-bit (no gapless trim), channels and rate of the stream.
fn mp3(path: &Path) -> Result<(Vec<u8>, usize, u32, bool, usize), String> {
    use symphonia::core::{audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint};
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let probed = symphonia::default::get_probe().format(Hint::new().with_extension("mp3"), mss, &FormatOptions::default(), &MetadataOptions::default()).map_err(|e| e.to_string())?;
    let mut format = probed.format;
    let track = format.default_track().ok_or("no mp3 frames")?;
    let (track_id, params) = (track.id, track.codec_params.clone());
    let mut dec = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(|e| e.to_string())?;
    let channels = params.channels.map_or(2, |c| c.count());
    let frequency = params.sample_rate.ok_or("no sample rate")?;
    let mut out = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match dec.decode(&packet) {
            Ok(buf) => {
                let mut s = SampleBuffer::<i16>::new(buf.capacity() as u64, *buf.spec());
                s.copy_interleaved_ref(buf);
                out.extend(s.samples().iter().flat_map(|x| x.to_le_bytes()));
            }
            Err(Error::DecodeError(_)) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok((out, channels, frequency, true, 2 * channels))
}

#[cfg(test)]
mod file_tests {
    /// StereoToMonoProvider16 truncates `l * 0.5f + r * 0.5f`; an odd data length throws.
    #[test]
    fn wav_stereo_to_mono() {
        let wav = |data: &[u8]| {
            let mut b = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x44\xac\0\0\x10\xb1\x02\0\x04\0\x10\0data".to_vec();
            b.extend((data.len() as u32).to_le_bytes());
            b.extend(data);
            let p = std::env::temp_dir().join(format!("ppg_wav_test_{}.wav", data.len()));
            std::fs::write(&p, b).unwrap();
            let r = super::file_to_pcm(&p);
            std::fs::remove_file(&p).unwrap();
            r
        };
        let s: Vec<u8> = [1i16, 0, -3, 0, 100, 201].iter().flat_map(|x| x.to_le_bytes()).collect();
        let p = wav(&s).unwrap();
        assert_eq!((p.channels, p.frequency, p.data), (1, 44100, vec![0, -1, 150]));
        assert!(wav(&s[..10]).is_err());
    }
}
