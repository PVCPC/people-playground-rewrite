//! What 1.28.45's BuzzerBehaviour uses to import a WAV file and to rebuild its clip, rewritten 1:1:
//! NAudio's WaveFileReader (WaveFileChunkReader, WaveFormat), RawSourceWaveStream, the PCM/float sample
//! providers, StereoToMonoSampleProvider, WdlResamplingSampleProvider + WdlResampler, plus the game's
//! BuzzerBehaviour.OpenFile delegate, BuzzerBehaviour.ClipFromData's sample read and FilePicker.
//! .NET exceptions come back as "Full.TypeName: message" strings. Float expressions follow the rewrite's
//! Mono rule (see `crate::game::D`): evaluated in double, rounded to float at every store.

use std::fmt;
use std::path::Path;

type NetResult<T> = Result<T, String>;

fn net_ex(type_name: &str, message: impl fmt::Display) -> String {
    format!("{type_name}: {message}")
}

/// C# `(int)double` on x64 Mono (cvttsd2si): NaN and out-of-range values give int.MinValue.
fn cs_int(x: f64) -> i32 {
    if x > -2147483649.0 && x < 2147483648.0 {
        x as i32
    } else {
        i32::MIN
    }
}

/// C# `x-- != 0`.
fn post_dec_nonzero(x: &mut i32) -> bool {
    let v = *x;
    *x = v.wrapping_sub(1);
    v != 0
}

const OOM: &str = "Insufficient memory to continue the execution of the program.";

/// `Array.Resize(ref a, n)` (and `new float[n]` when `a` is empty): keeps the head, zero-fills the rest.
fn array_resize(a: &mut Vec<f32>, n: i32) -> NetResult<()> {
    if n < 0 {
        return Err(net_ex("System.ArgumentOutOfRangeException", "Non-negative number required.\r\nParameter name: newSize"));
    }
    let n = n as usize;
    if n > a.len() {
        a.try_reserve_exact(n - a.len()).map_err(|_| net_ex("System.OutOfMemoryException", OOM))?;
    }
    a.resize(n, 0.0);
    Ok(())
}

// ---------------------------------------------------------------- WaveFormat

const PCM: u16 = 1;
const IEEE_FLOAT: u16 = 3;

#[derive(Clone, Debug)]
struct WaveFormat {
    encoding: u16,
    channels: i16,
    sample_rate: i32,
    average_bytes_per_second: i32,
    block_align: i16,
    bits_per_sample: i16,
}

impl WaveFormat {
    /// WaveFormat(int rate, int bits, int channels) (the game passes channels 1, so no ArgumentOutOfRangeException).
    fn new(rate: i32, bits: i32, channels: i32) -> Self {
        let block_align = channels.wrapping_mul(bits / 8) as i16;
        Self {
            encoding: PCM,
            channels: channels as i16,
            sample_rate: rate,
            bits_per_sample: bits as i16,
            block_align,
            average_bytes_per_second: rate.wrapping_mul(block_align as i32),
        }
    }

    /// WaveFormat.CreateIeeeFloatWaveFormat
    fn create_ieee_float(sample_rate: i32, channels: i32) -> Self {
        let block_align = channels.wrapping_mul(4) as i16;
        Self {
            encoding: IEEE_FLOAT,
            channels: channels as i16,
            sample_rate,
            bits_per_sample: 32,
            block_align,
            average_bytes_per_second: sample_rate.wrapping_mul(block_align as i32),
        }
    }
}

// ---------------------------------------------------------------- streams

const EOS: &str = "Unable to read beyond the end of the stream.";

/// The bytes behind a FileStream (WaveFileReader) or MemoryStream (RawSourceWaveStream), with BinaryReader reads.
struct ByteStream {
    data: Vec<u8>,
    pos: i64,
}

impl ByteStream {
    fn len(&self) -> i64 {
        self.data.len() as i64
    }

    /// Stream.Read: up to `count` bytes, fewer at the end; 0 past the end.
    fn read(&mut self, buffer: &mut [u8], offset: usize, count: i32) -> NetResult<i32> {
        if count < 0 {
            return Err(net_ex("System.ArgumentOutOfRangeException", "Non-negative number required.\r\nParameter name: count"));
        }
        let n = (self.len() - self.pos).clamp(0, count as i64) as usize;
        if n > 0 {
            let p = self.pos as usize;
            buffer[offset..offset + n].copy_from_slice(&self.data[p..p + n]);
        }
        self.pos += n as i64;
        Ok(n as i32)
    }

    /// BinaryReader.FillBuffer: exactly N bytes or EndOfStreamException.
    fn read_n<const N: usize>(&mut self) -> NetResult<[u8; N]> {
        let mut b = [0u8; N];
        if self.read(&mut b, 0, N as i32)? as usize != N {
            return Err(net_ex("System.IO.EndOfStreamException", EOS));
        }
        Ok(b)
    }
    fn read_i16(&mut self) -> NetResult<i16> {
        Ok(i16::from_le_bytes(self.read_n()?))
    }
    fn read_u16(&mut self) -> NetResult<u16> {
        Ok(u16::from_le_bytes(self.read_n()?))
    }
    fn read_i32(&mut self) -> NetResult<i32> {
        Ok(i32::from_le_bytes(self.read_n()?))
    }
    fn read_u32(&mut self) -> NetResult<u32> {
        Ok(u32::from_le_bytes(self.read_n()?))
    }
    fn read_i64(&mut self) -> NetResult<i64> {
        Ok(i64::from_le_bytes(self.read_n()?))
    }
}

/// ChunkIdentifier.ChunkIdentifierToInt32
fn chunk_id(s: &[u8; 4]) -> i32 {
    i32::from_le_bytes(*s)
}

fn format_ex(msg: &str) -> String {
    net_ex("System.FormatException", msg)
}

/// WaveFormat.FromFormatChunk (WaveFormat.ReadWaveFormat, then WaveFormatExtraData.ReadExtraData).
fn from_format_chunk(s: &mut ByteStream, format_chunk_length: i32) -> NetResult<WaveFormat> {
    if format_chunk_length < 16 {
        return Err(net_ex("NAudio.Utils.InvalidDataException", "Invalid WaveFormat Structure"));
    }
    let encoding = s.read_u16()?;
    let channels = s.read_i16()?;
    let sample_rate = s.read_i32()?;
    let average_bytes_per_second = s.read_i32()?;
    let block_align = s.read_i16()?;
    let bits_per_sample = s.read_i16()?;
    let mut extra_size = 0i16;
    if format_chunk_length > 16 {
        extra_size = s.read_i16()?;
        if extra_size as i32 != format_chunk_length - 18 {
            extra_size = (format_chunk_length - 18) as i16;
        }
    }
    // WaveFormatExtraData.ReadExtraData: BinaryReader.Read into its 100-byte extraData (short reads are fine).
    if extra_size > 0 {
        if extra_size > 100 {
            return Err(net_ex(
                "System.ArgumentException",
                "Offset and length were out of bounds for the array or count is greater than the number of elements from index to the end of the source collection.",
            ));
        }
        let mut extra = [0u8; 100];
        s.read(&mut extra, 0, extra_size as i32)?;
    }
    Ok(WaveFormat { encoding, channels, sample_rate, average_bytes_per_second, block_align, bits_per_sample })
}

/// WaveFileChunkReader.ReadWaveHeader (with ReadRiffHeader and ReadDs64Chunk): format, data position, data length.
fn read_wave_header(s: &mut ByteStream) -> NetResult<(WaveFormat, i64, i64)> {
    let mut data_chunk_position = -1i64;
    let mut wave_format = None;
    let mut data_chunk_length = 0i64;
    // WaveFileChunkReader.ReadRiffHeader
    let id = s.read_i32()?;
    let is_rf64 = id == chunk_id(b"RF64");
    if !is_rf64 && id != chunk_id(b"RIFF") {
        return Err(format_ex("Not a WAVE file - no RIFF header"));
    }
    let mut riff_size = s.read_u32()? as i64;
    if s.read_i32()? != chunk_id(b"WAVE") {
        return Err(format_ex("Not a WAVE file - no WAVE header"));
    }
    if is_rf64 {
        // WaveFileChunkReader.ReadDs64Chunk
        if s.read_i32()? != chunk_id(b"ds64") {
            return Err(format_ex("Invalid RF64 WAV file - No ds64 chunk found"));
        }
        let num2 = s.read_i32()?;
        riff_size = s.read_i64()?;
        data_chunk_length = s.read_i64()?;
        s.read_i64()?;
        // BinaryReader.ReadBytes(num2 - 24): negative throws, a short read does not.
        let n = num2.wrapping_sub(24);
        if n < 0 {
            return Err(net_ex("System.ArgumentOutOfRangeException", "Non-negative number required.\r\nParameter name: count"));
        }
        s.pos += (s.len() - s.pos).clamp(0, n as i64);
    }
    let data = chunk_id(b"data");
    let fmt = chunk_id(b"fmt ");
    let num3 = riff_size.wrapping_add(8).min(s.len());
    while s.pos <= num3.wrapping_sub(8) {
        let num4 = s.read_i32()?;
        let num5 = s.read_u32()?;
        if num4 == data {
            data_chunk_position = s.pos;
            if !is_rf64 {
                data_chunk_length = num5 as i64;
            }
            s.pos += num5 as i64;
            continue;
        }
        if num4 == fmt {
            if num5 > i32::MAX as u32 {
                return Err(net_ex("NAudio.Utils.InvalidDataException", format!("Format chunk length must be between 0 and {}.", i32::MAX)));
            }
            wave_format = Some(from_format_chunk(s, num5 as i32)?);
            continue;
        }
        if num5 as i64 > s.len() - s.pos {
            break;
        }
        // GetRiffChunk only records the chunk in ExtraChunks, which nothing here reads.
        if num5 > i32::MAX as u32 {
            return Err(net_ex("NAudio.Utils.InvalidDataException", format!("RiffChunk chunk length must be between 0 and {}.", i32::MAX)));
        }
        s.pos += num5 as i64;
    }
    let Some(wave_format) = wave_format else {
        return Err(format_ex("Invalid WAV file - No fmt chunk found"));
    };
    if data_chunk_position == -1 {
        return Err(format_ex("Invalid WAV file - No data chunk found"));
    }
    Ok((wave_format, data_chunk_position, data_chunk_length))
}

/// IWaveProvider
trait WaveProvider {
    fn wave_format(&self) -> &WaveFormat;
    fn read(&mut self, buffer: &mut [u8], offset: usize, count: i32) -> NetResult<i32>;
}

struct WaveFileReader {
    stream: ByteStream,
    wave_format: WaveFormat,
    data_position: i64,
    data_chunk_length: i64,
}

/// .NET's exception for a failed FileInfo.Length / File.OpenRead (Mono's wording).
fn io_ex(e: &std::io::Error, path: &Path) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => net_ex("System.IO.FileNotFoundException", format!("Could not find file \"{}\"", path.display())),
        std::io::ErrorKind::PermissionDenied => {
            net_ex("System.UnauthorizedAccessException", format!("Access to the path \"{}\" is denied.", path.display()))
        }
        _ => net_ex("System.IO.IOException", e),
    }
}

impl WaveFileReader {
    /// WaveFileReader(string waveFile): File.OpenRead, ReadWaveHeader, then Position = 0.
    fn open(path: &Path) -> NetResult<Self> {
        let data = std::fs::read(path).map_err(|e| io_ex(&e, path))?;
        let mut stream = ByteStream { data, pos: 0 };
        let (wave_format, data_position, data_chunk_length) = read_wave_header(&mut stream)?;
        let mut r = Self { stream, wave_format, data_position, data_chunk_length };
        r.set_position(0)?;
        Ok(r)
    }

    /// WaveFileReader.Position (get)
    fn position(&self) -> i64 {
        self.stream.pos - self.data_position
    }

    /// WaveFileReader.Position (set)
    fn set_position(&mut self, value: i64) -> NetResult<()> {
        let ba = self.wave_format.block_align as i64;
        if ba == 0 {
            return Err(net_ex("System.DivideByZeroException", "Attempted to divide by zero."));
        }
        let mut value = value.min(self.data_chunk_length);
        value -= value % ba;
        let p = value + self.data_position;
        if p < 0 {
            return Err(net_ex("System.ArgumentOutOfRangeException", "Non-negative number required.\r\nParameter name: value"));
        }
        self.stream.pos = p;
        Ok(())
    }

    /// WaveFileReader.Length
    fn length(&self) -> i64 {
        self.data_chunk_length
    }

    /// WaveStream.TotalTime: TimeSpan.FromSeconds(Length / AverageBytesPerSecond), as ticks.
    fn total_time_ticks(&self) -> NetResult<i64> {
        timespan_from_seconds(self.length() as f64 / self.wave_format.average_bytes_per_second as f64)
    }
}

/// TimeSpan.FromSeconds (TimeSpan.Interval): rounds to whole milliseconds; returns ticks.
fn timespan_from_seconds(value: f64) -> NetResult<i64> {
    if value.is_nan() {
        return Err(net_ex("System.ArgumentException", "TimeSpan does not accept floating point Not-a-Number values."));
    }
    let tmp = value * 1000.0;
    let millis = tmp + if value >= 0.0 { 0.5 } else { -0.5 };
    if millis > (i64::MAX / 10000) as f64 || millis < (i64::MIN / 10000) as f64 {
        return Err(net_ex("System.OverflowException", "TimeSpan overflowed because the duration is too long."));
    }
    Ok((millis as i64) * 10000)
}

impl WaveProvider for WaveFileReader {
    fn wave_format(&self) -> &WaveFormat {
        &self.wave_format
    }

    /// WaveFileReader.Read
    fn read(&mut self, buffer: &mut [u8], offset: usize, count: i32) -> NetResult<i32> {
        let ba = self.wave_format.block_align as i32;
        if count % ba != 0 {
            return Err(net_ex("System.ArgumentException", format!("Must read complete blocks: requested {count}, block align is {ba}")));
        }
        let mut count = count;
        if self.position() + count as i64 > self.data_chunk_length {
            count = (self.data_chunk_length - self.position()) as i32;
        }
        self.stream.read(buffer, offset, count)
    }
}

struct RawSourceWaveStream {
    stream: ByteStream,
    wave_format: WaveFormat,
}

impl WaveProvider for RawSourceWaveStream {
    fn wave_format(&self) -> &WaveFormat {
        &self.wave_format
    }

    /// RawSourceWaveStream.Read (a MemoryStream never throws EndOfStreamException).
    fn read(&mut self, buffer: &mut [u8], offset: usize, count: i32) -> NetResult<i32> {
        self.stream.read(buffer, offset, count)
    }
}

// ---------------------------------------------------------------- sample providers

/// ISampleProvider
trait SampleProvider {
    fn wave_format(&self) -> &WaveFormat;
    fn read(&mut self, buffer: &mut [f32], offset: usize, count: i32) -> NetResult<i32>;
}

#[derive(Clone, Copy)]
enum ConverterKind {
    Pcm8,
    Pcm16,
    Pcm24,
    Pcm32,
    Float,
    Float64,
}

/// SampleProviderConverterBase and its subclasses (Pcm8/16/24/32BitToSampleProvider, WaveToSampleProvider(64)).
struct SampleProviderConverter {
    kind: ConverterKind,
    source: Box<dyn WaveProvider>,
    wave_format: WaveFormat,
    source_buffer: Vec<u8>,
}

impl SampleProviderConverter {
    /// SampleProviderConverterBase.EnsureSourceBuffer (BufferHelpers.Ensure): a new zeroed array only when too small.
    fn ensure_source_buffer(&mut self, n: i32) {
        if self.source_buffer.len() < n as usize {
            self.source_buffer = vec![0; n as usize];
        }
    }
}

/// SampleProviderConverters.ConvertWaveProviderIntoSampleProvider (WaveExtensionMethods.ToSampleProvider)
fn to_sample_provider(source: Box<dyn WaveProvider>) -> NetResult<Box<dyn SampleProvider>> {
    let f = source.wave_format();
    let kind = if f.encoding == PCM {
        match f.bits_per_sample {
            8 => ConverterKind::Pcm8,
            16 => ConverterKind::Pcm16,
            24 => ConverterKind::Pcm24,
            32 => ConverterKind::Pcm32,
            _ => return Err(net_ex("System.InvalidOperationException", "Unsupported bit depth")),
        }
    } else if f.encoding == IEEE_FLOAT {
        if f.bits_per_sample == 64 { ConverterKind::Float64 } else { ConverterKind::Float }
    } else {
        return Err(net_ex("System.ArgumentException", "Unsupported source encoding"));
    };
    // SampleProviderConverterBase(IWaveProvider source)
    let wave_format = WaveFormat::create_ieee_float(f.sample_rate, f.channels as i32);
    Ok(Box::new(SampleProviderConverter { kind, source, wave_format, source_buffer: Vec::new() }))
}

impl SampleProvider for SampleProviderConverter {
    fn wave_format(&self) -> &WaveFormat {
        &self.wave_format
    }

    fn read(&mut self, buffer: &mut [f32], offset: usize, count: i32) -> NetResult<i32> {
        // Bytes per sample read from the source for each subclass's Read.
        let width = match self.kind {
            ConverterKind::Pcm8 => 1,
            ConverterKind::Pcm16 => 2,
            ConverterKind::Pcm24 => 3,
            ConverterKind::Pcm32 | ConverterKind::Float => 4,
            ConverterKind::Float64 => 8,
        };
        let num = count.wrapping_mul(width);
        self.ensure_source_buffer(num);
        let num2 = self.source.read(&mut self.source_buffer, 0, num)?;
        let sb = &self.source_buffer;
        let mut o = offset;
        for i in (0..num2.max(0) as usize).step_by(width as usize) {
            buffer[o] = match self.kind {
                // Pcm8BitToSampleProvider.Read
                ConverterKind::Pcm8 => (sb[i] as f64 / 128.0 - 1.0) as f32,
                // Pcm16BitToSampleProvider.Read
                ConverterKind::Pcm16 => (i16::from_le_bytes([sb[i], sb[i + 1]]) as f64 / 32768.0) as f32,
                // Pcm24BitToSampleProvider.Read
                ConverterKind::Pcm24 => {
                    ((((sb[i + 2] as i8 as i32) << 16) | ((sb[i + 1] as i32) << 8) | sb[i] as i32) as f64 / 8388608.0) as f32
                }
                // Pcm32BitToSampleProvider.Read
                ConverterKind::Pcm32 => (i32::from_le_bytes([sb[i], sb[i + 1], sb[i + 2], sb[i + 3]]) as f64 / 2147483648.0) as f32,
                // WaveToSampleProvider.Read
                ConverterKind::Float => f32::from_le_bytes([sb[i], sb[i + 1], sb[i + 2], sb[i + 3]]),
                // WaveToSampleProvider64.Read
                ConverterKind::Float64 => f64::from_le_bytes(sb[i..i + 8].try_into().unwrap()) as f32,
            };
            o += 1;
        }
        Ok(num2 / width)
    }
}

struct StereoToMonoSampleProvider {
    source_provider: Box<dyn SampleProvider>,
    source_buffer: Vec<f32>,
    left_volume: f32,
    right_volume: f32,
    wave_format: WaveFormat,
}

/// WaveExtensionMethods.ToMono (with the StereoToMonoSampleProvider constructor).
fn to_mono(source: Box<dyn SampleProvider>, left_vol: f32, right_vol: f32) -> NetResult<Box<dyn SampleProvider>> {
    if source.wave_format().channels == 1 {
        return Ok(source);
    }
    if source.wave_format().channels != 2 {
        return Err(net_ex("System.ArgumentException", "Source must be stereo"));
    }
    let wave_format = WaveFormat::create_ieee_float(source.wave_format().sample_rate, 1);
    Ok(Box::new(StereoToMonoSampleProvider {
        source_provider: source,
        source_buffer: Vec::new(),
        left_volume: left_vol,
        right_volume: right_vol,
        wave_format,
    }))
}

impl SampleProvider for StereoToMonoSampleProvider {
    fn wave_format(&self) -> &WaveFormat {
        &self.wave_format
    }

    /// StereoToMonoSampleProvider.Read (writes from offset / 2, as NAudio does).
    fn read(&mut self, buffer: &mut [f32], offset: usize, count: i32) -> NetResult<i32> {
        let num = count.wrapping_mul(2);
        if self.source_buffer.len() < num as usize {
            self.source_buffer = vec![0.0; num as usize];
        }
        let num2 = self.source_provider.read(&mut self.source_buffer, 0, num)?;
        let mut num3 = offset / 2;
        for i in (0..num2.max(0) as usize).step_by(2) {
            let num4 = self.source_buffer[i] as f64;
            let num5 = self.source_buffer[i + 1] as f64;
            buffer[num3] = (num4 * self.left_volume as f64 + num5 * self.right_volume as f64) as f32;
            num3 += 1;
        }
        Ok(num2 / 2)
    }
}

// ---------------------------------------------------------------- WdlResampler

/// WdlResampler.WDL_Resampler_IIRFilter
struct IirFilter {
    m_fpos: f64,
    m_a1: f64,
    m_a2: f64,
    m_b0: f64,
    m_b1: f64,
    m_b2: f64,
    m_hist: Vec<[f64; 4]>,
}

impl IirFilter {
    /// WDL_Resampler_IIRFilter constructor
    fn new() -> Self {
        Self { m_fpos: -1.0, m_a1: 0.0, m_a2: 0.0, m_b0: 0.0, m_b1: 0.0, m_b2: 0.0, m_hist: vec![[0.0; 4]; 256] }
    }

    /// WDL_Resampler_IIRFilter.Reset
    fn reset(&mut self) {
        self.m_hist = vec![[0.0; 4]; 256];
    }

    /// WDL_Resampler_IIRFilter.setParms
    fn set_parms(&mut self, fpos: f64, q: f64) {
        if !((fpos - self.m_fpos).abs() < 1E-06) {
            self.m_fpos = fpos;
            let num = fpos * std::f64::consts::PI;
            let num2 = num.cos();
            let num3 = num.sin() / (2.0 * q);
            let num4 = 1.0 / (1.0 + num3);
            self.m_b1 = (1.0 - num2) * num4;
            self.m_b0 = self.m_b1 * 0.5;
            self.m_b2 = self.m_b0;
            self.m_a1 = -2.0 * num2 * num4;
            self.m_a2 = (1.0 - num3) * num4;
        }
    }

    /// WDL_Resampler_IIRFilter.Apply (every NAudio call filters a buffer in place, so one slice).
    fn apply(&mut self, buf: &mut [f32], mut in_index: i32, mut out_index: i32, mut ns: i32, span: i32, w: i32) {
        let (b, b2, b3, a, a2) = (self.m_b0, self.m_b1, self.m_b2, self.m_a1, self.m_a2);
        let h = &mut self.m_hist[w as usize];
        while post_dec_nonzero(&mut ns) {
            let num = buf[in_index as usize] as f64;
            in_index += span;
            let x = num * b + h[0] * b2 + h[1] * b3 - h[2] * a - h[3] * a2;
            h[1] = h[0];
            h[0] = num;
            h[3] = h[2];
            h[2] = x; // denormal_filter is the identity
            buf[out_index as usize] = h[2] as f32;
            out_index += span;
        }
    }
}

struct WdlResampler {
    m_sratein: f64,
    m_srateout: f64,
    m_fracpos: f64,
    m_ratio: f64,
    m_filter_ratio: f64,
    m_filterq: f32,
    m_filterpos: f32,
    m_rsinbuf: Vec<f32>,
    m_filter_coeffs: Vec<f32>,
    m_iirfilter: Option<IirFilter>,
    m_filter_coeffs_size: i32,
    m_last_requested: i32,
    m_filtlatency: i32,
    m_samples_in_rsinbuf: i32,
    m_lp_oversize: i32,
    m_sincsize: i32,
    m_filtercnt: i32,
    m_sincoversize: i32,
    m_interp: bool,
    m_feedmode: bool,
}

impl WdlResampler {
    /// WdlResampler constructor
    fn new() -> Self {
        let mut r = Self {
            m_filterq: 0.707,
            m_filterpos: 0.693,
            m_sincoversize: 0,
            m_lp_oversize: 1,
            m_sincsize: 0,
            m_filtercnt: 1,
            m_interp: true,
            m_feedmode: false,
            m_filter_coeffs_size: 0,
            m_sratein: 44100.0,
            m_srateout: 44100.0,
            m_ratio: 1.0,
            m_filter_ratio: -1.0,
            m_fracpos: 0.0,
            m_rsinbuf: Vec::new(),
            m_filter_coeffs: Vec::new(),
            m_iirfilter: None,
            m_last_requested: 0,
            m_filtlatency: 0,
            m_samples_in_rsinbuf: 0,
        };
        r.reset(0.0);
        r
    }

    /// WdlResampler.SetMode
    fn set_mode(&mut self, interp: bool, filtercnt: i32, sinc: bool, sinc_size: i32, sinc_interpsize: i32) {
        self.m_sincsize = if sinc && sinc_size >= 4 { sinc_size.min(8192) } else { 0 };
        self.m_sincoversize = if self.m_sincsize == 0 { 1 } else if sinc_interpsize <= 1 { 1 } else { sinc_interpsize.min(4096) };
        self.m_filtercnt = if self.m_sincsize == 0 { if filtercnt > 0 { filtercnt.min(4) } else { 0 } } else { 0 };
        self.m_interp = interp && self.m_sincsize == 0;
        if self.m_sincsize == 0 {
            self.m_filter_coeffs = Vec::new();
            self.m_filter_coeffs_size = 0;
        }
        if self.m_filtercnt == 0 {
            self.m_iirfilter = None;
        }
    }

    /// WdlResampler.SetFilterParms
    fn set_filter_parms(&mut self, filterpos: f32, filterq: f32) {
        self.m_filterpos = filterpos;
        self.m_filterq = filterq;
    }

    /// WdlResampler.SetFeedMode
    fn set_feed_mode(&mut self, want_input_driven: bool) {
        self.m_feedmode = want_input_driven;
    }

    /// WdlResampler.Reset
    fn reset(&mut self, fracpos: f64) {
        self.m_last_requested = 0;
        self.m_filtlatency = 0;
        self.m_fracpos = fracpos;
        self.m_samples_in_rsinbuf = 0;
        if let Some(f) = &mut self.m_iirfilter {
            f.reset();
        }
    }

    /// WdlResampler.SetRates
    fn set_rates(&mut self, mut rate_in: f64, mut rate_out: f64) {
        if rate_in < 1.0 {
            rate_in = 1.0;
        }
        if rate_out < 1.0 {
            rate_out = 1.0;
        }
        if rate_in != self.m_sratein || rate_out != self.m_srateout {
            self.m_sratein = rate_in;
            self.m_srateout = rate_out;
            self.m_ratio = self.m_sratein / self.m_srateout;
        }
    }

    /// WdlResampler.ResamplePrepare: input samples wanted and where they go in m_rsinbuf (the `inbuffer`).
    fn resample_prepare(&mut self, out_samples: i32, nch: i32) -> NetResult<(i32, usize)> {
        if nch > 64 || nch < 1 {
            return Ok((0, 0));
        }
        let mut num = 0;
        if self.m_sincsize > 1 {
            num = self.m_sincsize;
        }
        let num2 = num / 2;
        if num2 > 1 && self.m_samples_in_rsinbuf < num2 - 1 {
            self.m_filtlatency += num2 - 1 - self.m_samples_in_rsinbuf;
            self.m_samples_in_rsinbuf = num2 - 1;
            if self.m_samples_in_rsinbuf > 0 {
                self.m_rsinbuf = Vec::new();
                array_resize(&mut self.m_rsinbuf, self.m_samples_in_rsinbuf * nch)?;
            }
        }
        let mut num3 = if self.m_feedmode {
            out_samples
        } else {
            cs_int(self.m_ratio * out_samples as f64).wrapping_add(4).wrapping_add(num).wrapping_sub(self.m_samples_in_rsinbuf)
        };
        if num3 < 0 {
            num3 = 0;
        }
        loop {
            array_resize(&mut self.m_rsinbuf, self.m_samples_in_rsinbuf.wrapping_add(num3).wrapping_mul(nch))?;
            let num4 = self.m_rsinbuf.len() as i32 / nch - self.m_samples_in_rsinbuf;
            if num4 == num3 {
                break;
            }
            if num3 > 4 && num4 == 0 {
                num3 /= 2;
                continue;
            }
            num3 = num4;
            break;
        }
        let inbuffer_offset = (self.m_samples_in_rsinbuf * nch) as usize;
        self.m_last_requested = num3;
        Ok((num3, inbuffer_offset))
    }

    /// WdlResampler.ResampleOut
    fn resample_out(&mut self, out_buffer: &mut [f32], out_buffer_index: i32, nsamples_in: i32, nsamples_out: i32, nch: i32) -> NetResult<i32> {
        if nch > 64 || nch < 1 {
            return Ok(0);
        }
        if self.m_filtercnt > 0 && self.m_ratio > 1.0 && nsamples_in > 0 {
            let filt = self.m_iirfilter.get_or_insert_with(IirFilter::new);
            filt.set_parms(1.0 / self.m_ratio * self.m_filterpos as f64, self.m_filterq as f64);
            let num = self.m_samples_in_rsinbuf * nch;
            let mut num2 = 0;
            for i in 0..nch {
                for _ in 0..self.m_filtercnt {
                    filt.apply(&mut self.m_rsinbuf, num + i, num + i, nsamples_in, nch, num2);
                    num2 += 1;
                }
            }
        }
        self.m_samples_in_rsinbuf += nsamples_in.min(self.m_last_requested);
        let mut num3 = self.m_samples_in_rsinbuf;
        if nsamples_in < self.m_last_requested {
            let num4 = (self.m_last_requested - nsamples_in) * 2 + self.m_sincsize * 2;
            let num5 = (self.m_samples_in_rsinbuf + num4) * nch;
            array_resize(&mut self.m_rsinbuf, num5)?;
            if self.m_rsinbuf.len() == num5 as usize {
                // Array.Clear
                let a = (self.m_samples_in_rsinbuf * nch) as usize;
                self.m_rsinbuf[a..a + (num4 * nch) as usize].fill(0.0);
                num3 = self.m_samples_in_rsinbuf + num4;
            }
        }
        let mut num6 = 0i32;
        let mut num7 = self.m_fracpos;
        let ratio = self.m_ratio;
        let num8 = 0i32;
        let mut num9 = out_buffer_index;
        let mut num10 = nsamples_out;
        let mut num11 = 0i32;
        let rs = &self.m_rsinbuf;
        if self.m_sincsize != 0 {
            if self.m_ratio > 1.0 {
                self.build_low_pass(1.0 / (self.m_ratio * 1.03));
            } else {
                self.build_low_pass(1.0);
            }
            let rs = &self.m_rsinbuf;
            let fc = &self.m_filter_coeffs;
            let lp = self.m_lp_oversize;
            let filter_coeffs_size = self.m_filter_coeffs_size;
            let num12 = num3 - filter_coeffs_size;
            num11 = filter_coeffs_size / 2 - 1;
            let filter_index = 0;
            if nch == 1 {
                while post_dec_nonzero(&mut num10) {
                    let num13 = cs_int(num7);
                    if num13 >= num12 - 1 {
                        break;
                    }
                    sinc_sample1(out_buffer, num9, rs, num8 + num13, num7 - num13 as f64, fc, filter_index, filter_coeffs_size, lp);
                    num9 += 1;
                    num7 += ratio;
                    num6 += 1;
                }
            } else if nch == 2 {
                while post_dec_nonzero(&mut num10) {
                    let num14 = cs_int(num7);
                    if num14 >= num12 - 1 {
                        break;
                    }
                    sinc_sample2(out_buffer, num9, rs, num8 + num14 * 2, num7 - num14 as f64, fc, filter_index, filter_coeffs_size, lp);
                    num9 += 2;
                    num7 += ratio;
                    num6 += 1;
                }
            } else {
                while post_dec_nonzero(&mut num10) {
                    let num15 = cs_int(num7);
                    if num15 >= num12 - 1 {
                        break;
                    }
                    sinc_sample(out_buffer, num9, rs, num8 + num15 * nch, num7 - num15 as f64, nch, fc, filter_index, filter_coeffs_size, lp);
                    num9 += nch;
                    num7 += ratio;
                    num6 += 1;
                }
            }
        } else if !self.m_interp {
            if nch == 1 {
                while post_dec_nonzero(&mut num10) {
                    let num16 = cs_int(num7);
                    if num16 >= num3 {
                        break;
                    }
                    out_buffer[num9 as usize] = rs[(num8 + num16) as usize];
                    num9 += 1;
                    num7 += ratio;
                    num6 += 1;
                }
            } else if nch == 2 {
                while post_dec_nonzero(&mut num10) {
                    let mut num17 = cs_int(num7);
                    if num17 >= num3 {
                        break;
                    }
                    num17 += num17;
                    out_buffer[num9 as usize] = rs[(num8 + num17) as usize];
                    out_buffer[num9 as usize + 1] = rs[(num8 + num17 + 1) as usize];
                    num9 += 2;
                    num7 += ratio;
                    num6 += 1;
                }
            } else {
                while post_dec_nonzero(&mut num10) {
                    let num18 = cs_int(num7);
                    if num18 >= num3 {
                        break;
                    }
                    let s = (num8 + num18 * nch) as usize;
                    out_buffer[num9 as usize..(num9 + nch) as usize].copy_from_slice(&rs[s..s + nch as usize]);
                    num9 += nch;
                    num7 += ratio;
                    num6 += 1;
                }
            }
        } else if nch == 1 {
            while post_dec_nonzero(&mut num10) {
                let num19 = cs_int(num7);
                let num20 = num7 - num19 as f64;
                if num19 >= num3 - 1 {
                    break;
                }
                let num21 = 1.0 - num20;
                let num22 = (num8 + num19) as usize;
                out_buffer[num9 as usize] = (rs[num22] as f64 * num21 + rs[num22 + 1] as f64 * num20) as f32;
                num9 += 1;
                num7 += ratio;
                num6 += 1;
            }
        } else if nch == 2 {
            while post_dec_nonzero(&mut num10) {
                let num23 = cs_int(num7);
                let num24 = num7 - num23 as f64;
                if num23 >= num3 - 1 {
                    break;
                }
                let num25 = 1.0 - num24;
                let num26 = (num8 + num23 * 2) as usize;
                out_buffer[num9 as usize] = (rs[num26] as f64 * num25 + rs[num26 + 2] as f64 * num24) as f32;
                out_buffer[num9 as usize + 1] = (rs[num26 + 1] as f64 * num25 + rs[num26 + 3] as f64 * num24) as f32;
                num9 += 2;
                num7 += ratio;
                num6 += 1;
            }
        } else {
            while post_dec_nonzero(&mut num10) {
                let num27 = cs_int(num7);
                let num28 = num7 - num27 as f64;
                if num27 >= num3 - 1 {
                    break;
                }
                let num29 = 1.0 - num28;
                let mut num30 = nch;
                let mut num31 = (num8 + num27 * nch) as usize;
                while post_dec_nonzero(&mut num30) {
                    out_buffer[num9 as usize] = (rs[num31] as f64 * num29 + rs[num31 + nch as usize] as f64 * num28) as f32;
                    num9 += 1;
                    num31 += 1;
                }
                num7 += ratio;
                num6 += 1;
            }
        }
        if self.m_filtercnt > 0 && self.m_ratio < 1.0 && num6 > 0 {
            let filt = self.m_iirfilter.get_or_insert_with(IirFilter::new);
            filt.set_parms(self.m_ratio * self.m_filterpos as f64, self.m_filterq as f64);
            let mut num32 = 0;
            for k in 0..nch {
                for _ in 0..self.m_filtercnt {
                    filt.apply(out_buffer, k, k, num6, nch, num32);
                    num32 += 1;
                }
            }
        }
        if num6 > 0 && num3 > self.m_samples_in_rsinbuf {
            let num33 = (num7 - self.m_samples_in_rsinbuf as f64 + num11 as f64) / ratio;
            if num33 > 0.0 {
                num6 = num6.wrapping_sub(cs_int(num33 + 0.5));
                if num6 < 0 {
                    num6 = 0;
                }
            }
        }
        let num34 = cs_int(num7);
        self.m_fracpos = num7 - num34 as f64;
        self.m_samples_in_rsinbuf -= num34;
        if self.m_samples_in_rsinbuf <= 0 {
            self.m_samples_in_rsinbuf = 0;
        } else {
            // Array.Copy within m_rsinbuf
            let s = (num8 + num34 * nch) as usize;
            let n = (self.m_samples_in_rsinbuf * nch) as usize;
            self.m_rsinbuf.copy_within(s..s + n, num8 as usize);
        }
        Ok(num6)
    }

    /// WdlResampler.BuildLowPass
    fn build_low_pass(&mut self, filtpos: f64) {
        let sincsize = self.m_sincsize;
        let sincoversize = self.m_sincoversize;
        if self.m_filter_ratio == filtpos && self.m_filter_coeffs_size == sincsize && self.m_lp_oversize == sincoversize {
            return;
        }
        self.m_lp_oversize = sincoversize;
        self.m_filter_ratio = filtpos;
        let lp = self.m_lp_oversize;
        let num = (sincsize + 1) * lp;
        self.m_filter_coeffs.resize(num as usize, 0.0);
        self.m_filter_coeffs_size = sincsize;
        let num2 = sincsize * lp;
        let num3 = num2 / 2;
        let mut num4 = 0.0f64;
        let mut num5 = 0.0f64;
        let pi = std::f64::consts::PI;
        let num6 = pi * 2.0 / num2 as f64;
        let num7 = pi / lp as f64 * filtpos;
        let mut num8 = num7 * (-num3) as f64;
        for i in -num3..num3 + lp {
            let mut num9 = 287.0 / 800.0 - 0.48829 * num5.cos() + 0.14128 * (2.0 * num5).cos() - 0.01168 * (6.0 * num5).cos();
            if i != 0 {
                num9 *= num8.sin() / num8;
            }
            num5 += num6;
            num8 += num7;
            self.m_filter_coeffs[(num3 + i) as usize] = num9 as f32;
            if i < num3 {
                num4 += num9;
            }
        }
        num4 = lp as f64 / num4;
        for i in 0..(num2 + lp) as usize {
            self.m_filter_coeffs[i] = (self.m_filter_coeffs[i] as f64 * num4) as f32;
        }
    }
}

/// WdlResampler.SincSample
#[allow(clippy::too_many_arguments)]
fn sinc_sample(out: &mut [f32], out_index: i32, inb: &[f32], in_index: i32, mut fracpos: f64, nch: i32, filter: &[f32], mut filter_index: i32, filtsz: i32, lp: i32) {
    fracpos *= lp as f64;
    let num = cs_int(fracpos);
    filter_index += lp - 1 - num;
    fracpos -= num as f64;
    for i in 0..nch {
        let mut num2 = 0.0f64;
        let mut num3 = 0.0f64;
        let mut num4 = filter_index as usize;
        let mut num5 = (in_index + i) as usize;
        let mut num6 = filtsz;
        while post_dec_nonzero(&mut num6) {
            num2 += filter[num4] as f64 * inb[num5] as f64;
            num3 += filter[num4 + 1] as f64 * inb[num5] as f64;
            num5 += nch as usize;
            num4 += lp as usize;
        }
        out[(out_index + i) as usize] = (num2 * fracpos + num3 * (1.0 - fracpos)) as f32;
    }
}

/// WdlResampler.SincSample1: the same sums as SincSample with one channel.
#[allow(clippy::too_many_arguments)]
fn sinc_sample1(out: &mut [f32], out_index: i32, inb: &[f32], in_index: i32, fracpos: f64, filter: &[f32], filter_index: i32, filtsz: i32, lp: i32) {
    sinc_sample(out, out_index, inb, in_index, fracpos, 1, filter, filter_index, filtsz, lp);
}

/// WdlResampler.SincSample2
#[allow(clippy::too_many_arguments)]
fn sinc_sample2(out: &mut [f32], out_index: i32, inb: &[f32], in_index: i32, mut fracpos: f64, filter: &[f32], mut filter_index: i32, filtsz: i32, lp: i32) {
    fracpos *= lp as f64;
    let num = cs_int(fracpos);
    filter_index += lp - 1 - num;
    fracpos -= num as f64;
    let (mut num2, mut num3, mut num4, mut num5) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut f = filter_index as usize;
    let mut s = in_index as usize;
    let lp = lp as usize;
    let mut num8 = filtsz / 2;
    let m = |a: f32, b: f32| a as f64 * b as f64;
    while post_dec_nonzero(&mut num8) {
        num2 += m(filter[f], inb[s]);
        num3 += m(filter[f], inb[s + 1]);
        num4 += m(filter[f + 1], inb[s]);
        num5 += m(filter[f + 1], inb[s + 1]);
        num2 += m(filter[f + lp], inb[s + 2]);
        num3 += m(filter[f + lp], inb[s + 3]);
        num4 += m(filter[f + lp + 1], inb[s + 2]);
        num5 += m(filter[f + lp + 1], inb[s + 3]);
        s += 4;
        f += lp * 2;
    }
    out[out_index as usize] = (num2 * fracpos + num4 * (1.0 - fracpos)) as f32;
    out[out_index as usize + 1] = (num3 * fracpos + num5 * (1.0 - fracpos)) as f32;
}

struct WdlResamplingSampleProvider {
    resampler: WdlResampler,
    out_format: WaveFormat,
    source: Box<dyn SampleProvider>,
    channels: i32,
}

impl WdlResamplingSampleProvider {
    /// WdlResamplingSampleProvider constructor
    fn new(source: Box<dyn SampleProvider>, new_sample_rate: i32) -> Self {
        let channels = source.wave_format().channels as i32;
        let out_format = WaveFormat::create_ieee_float(new_sample_rate, channels);
        let mut resampler = WdlResampler::new();
        resampler.set_mode(true, 2, false, 64, 32);
        resampler.set_filter_parms(0.693, 0.707);
        resampler.set_feed_mode(false);
        resampler.set_rates(source.wave_format().sample_rate as f64, new_sample_rate as f64);
        Self { resampler, out_format, source, channels }
    }
}

impl SampleProvider for WdlResamplingSampleProvider {
    fn wave_format(&self) -> &WaveFormat {
        &self.out_format
    }

    /// WdlResamplingSampleProvider.Read
    fn read(&mut self, buffer: &mut [f32], offset: usize, count: i32) -> NetResult<i32> {
        let num = count / self.channels;
        let (num2, inbuffer_offset) = self.resampler.resample_prepare(num, self.out_format.channels as i32)?;
        let nsamples_in = self.source.read(&mut self.resampler.m_rsinbuf, inbuffer_offset, num2 * self.channels)? / self.channels;
        Ok(self.resampler.resample_out(buffer, offset as i32, nsamples_in, num, self.channels)? * self.channels)
    }
}

// ---------------------------------------------------------------- game side

// PpgPlayerException..ctor(message): base(message), the text after "PpgPlayerException: ".
/// PpgPlayerException: the game's own exception type, thrown after the notification.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PpgPlayerException(pub String);

impl fmt::Display for PpgPlayerException {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "PpgPlayerException: {}", self.0)
    }
}

/// Why BuzzerBehaviour.OpenFile's delegate stopped.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum OpenFileError {
    /// FileInfo.Length > 1000000.
    TooBig,
    /// WaveStream.TotalTime > 15 s.
    TooLong,
    /// Any other .NET exception, as "Full.TypeName: message".
    Exception(String),
}

impl OpenFileError {
    /// The DialogBoxManager.Notification text shown before the PpgPlayerException.
    pub(crate) fn notification(&self) -> Option<&'static str> {
        match self {
            Self::TooBig => Some("Buzzer audio file size may not exceed 1MB"),
            Self::TooLong => Some("Buzzer audio cannot last longer than 15 seconds"),
            Self::Exception(_) => None,
        }
    }

    /// The exception the delegate throws, as .NET would print "Type: message".
    pub(crate) fn exception_text(&self) -> String {
        match self {
            Self::TooBig => PpgPlayerException("File size may not exceed 1MB".into()).to_string(),
            Self::TooLong => PpgPlayerException("Audio cannot last longer than 15 seconds".into()).to_string(),
            Self::Exception(s) => s.clone(),
        }
    }
}

/// `new float[long n]`
fn new_float_array(n: i64) -> NetResult<Vec<f32>> {
    if n < 0 || n > i32::MAX as i64 {
        return Err(net_ex("System.OverflowException", "Arithmetic operation resulted in an overflow."));
    }
    let mut v = Vec::new();
    v.try_reserve_exact(n as usize).map_err(|_| net_ex("System.OutOfMemoryException", OOM))?;
    v.resize(n as usize, 0.0);
    Ok(v)
}

/// BuzzerBehaviour.OpenFile's FilePicker delegate after `paths.Length == 1`: size and length checks, then
/// WdlResamplingSampleProvider(ToMono(ToSampleProvider(reader)), 8000) into bytes (array2) for ClipFromData.
pub(crate) fn buzzer_open_file_convert(path: &Path) -> Result<Vec<u8>, OpenFileError> {
    use crate::game::mathf::{clamp, map_range};
    let ex = OpenFileError::Exception;
    // FileInfo.Length
    let meta = std::fs::metadata(path).map_err(|e| ex(io_ex(&e, path)))?;
    if meta.is_dir() {
        return Err(ex(net_ex("System.IO.FileNotFoundException", format!("Could not find file \"{}\"", path.display()))));
    }
    if meta.len() as f64 > 1000000.0 {
        return Err(OpenFileError::TooBig);
    }
    let reader = WaveFileReader::open(path).map_err(ex)?;
    if reader.total_time_ticks().map_err(ex)? > 150_000_000 {
        return Err(OpenFileError::TooLong);
    }
    let length = reader.length();
    let source = to_mono(to_sample_provider(Box::new(reader)).map_err(ex)?, 0.5, 0.5).map_err(ex)?;
    let mut provider = WdlResamplingSampleProvider::new(source, 8000);
    let mut array = new_float_array(length).map_err(ex)?;
    let num = provider.read(&mut array, 0, length as i32).map_err(ex)?;
    // (byte) of the float MapRange returns: truncation (NaN gives 0 on x64 either way).
    Ok(array[..num.max(0) as usize].iter().map(|&v| map_range(-1.0, 1.0, 0.0, 255.0, clamp(v, -1.0, 1.0)) as u8).collect())
}

/// BuzzerBehaviour.ClipFromData's sample read: RawSourceWaveStream(bytes, 0, min(120000, len), WaveFormat(8000, 8, 1)),
/// ToSampleProvider, Read into float[stream.Length]; returns the `num` samples AudioClip.Create(name, num, 1, 8000) holds.
pub(crate) fn clip_from_data_samples(bytes: &[u8]) -> Vec<f32> {
    let n = bytes.len().min(120000);
    let stream = RawSourceWaveStream { stream: ByteStream { data: bytes[..n].to_vec(), pos: 0 }, wave_format: WaveFormat::new(8000, 8, 1) };
    let mut provider = to_sample_provider(Box::new(stream)).expect("8-bit PCM always converts");
    let mut array = vec![0.0f32; n];
    let num = provider.read(&mut array, 0, n as i32).expect("a MemoryStream read cannot fail");
    array.truncate(num as usize);
    array
}

// ---------------------------------------------------------------- FilePicker

/// char.IsLetterOrDigit || char.IsWhiteSpace of one UTF-16 char: the general categories Lu, Ll, Lt, Lm, Lo, Nd, and
/// Zs, Zl, Zp with the Latin-1 white space (U+0009..U+000D, U+0085). A char past U+FFFF is two surrogates (Cs) in .NET, so
/// neither half is kept. The categories come from unicode-general-category's tables (Unicode 16), not Mono's own; the
/// game only ever cleans its two ASCII filter constants.
fn letter_digit_or_space(c: char) -> bool {
    use unicode_general_category::{get_general_category, GeneralCategory as G};
    if (c as u32) > 0xFFFF {
        return false;
    }
    matches!(c, '\u{9}'..='\u{d}' | '\u{85}')
        || matches!(
            get_general_category(c),
            G::UppercaseLetter | G::LowercaseLetter | G::TitlecaseLetter | G::ModifierLetter | G::OtherLetter | G::DecimalNumber | G::SpaceSeparator | G::LineSeparator | G::ParagraphSeparator
        )
}

/// FilePicker.CleanString: Trim, then keep letters, digits and white space.
fn clean_string(x: &str) -> String {
    x.trim().chars().filter(|&c| letter_digit_or_space(c)).collect()
}

/// FilePicker.CleanExtensions
fn clean_extensions(x: &[&str]) -> Vec<String> {
    x.iter().map(|&e| if e == "*" { e.to_string() } else { format!("*.{}", clean_string(&e.to_lowercase())) }).collect()
}

/// FilePicker.Windows.WindowsFileSelector.SelectFile's lpstrFilter text. Each (name, extensions) pair is a
/// FilePicker.Filter (Filter..ctor(name, params extensions) only stores the two).
pub(crate) fn file_picker_filter_string(filters: &[(&str, &[&str])]) -> String {
    let parts: Vec<String> = filters.iter().map(|(name, ext)| clean_string(name) + "\0" + &clean_extensions(ext).join(";")).collect();
    parts.join("\0") + "\0\0"
}

/// FilePicker.OpenFile's Task body: WindowsFileSelector.SelectFile("Select file", filters, null, multiselect).
/// Blocks until the dialog closes; empty on cancel or failure (the C# then calls back with the empty array).
pub(crate) fn file_picker_open_file(filters: &[(&str, &[&str])], multiselect: bool) -> Vec<String> {
    select_file("Select file", filters, multiselect)
}

/// FilePicker.Windows.WindowsFileSelector.SelectFile (NativeWindowsFileExplorer.GetOpenFileName), initialPath null.
#[cfg(windows)]
fn select_file(dialog_title: &str, filters: &[(&str, &[&str])], multiselect: bool) -> Vec<String> {
    use windows_sys::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OPENFILENAMEW};
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    // The marshaler copies each string with a terminator: the filter keeps its embedded NULs.
    let filter = wide(&file_picker_filter_string(filters));
    let mut file = vec![0u16; 1024 + 1];
    let mut file_title = vec![0u16; 512 + 1];
    let title = wide(dialog_title);
    // OpenFileName..ctor: hwndOwner, hInstance, lCustData, lpfnHook and pvReserved IntPtr.Zero, the rest 0 or null
    // (Default), then SelectFile's writes.
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter.as_ptr(),
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: 1024,
        lpstrFileTitle: file_title.as_mut_ptr(),
        nMaxFileTitle: 512,
        lpstrTitle: title.as_ptr(),
        Flags: 6152 | if multiselect { 640 } else { 0 },
        ..Default::default()
    };
    // SAFETY: every pointer in `ofn` points into a live buffer of the stated size for the whole call.
    if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
        return Vec::new();
    }
    // The string field comes back up to its first NUL, then Split('\0', RemoveEmptyEntries): a multi-select
    // result ("dir\0a\0b\0\0") therefore yields only the directory, as in the game.
    let end = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    String::from_utf16_lossy(&file[..end]).split('\0').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

#[cfg(not(windows))]
fn select_file(_dialog_title: &str, _filters: &[(&str, &[&str])], _multiselect: bool) -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A canonical 44-byte-header WAV.
    fn wav(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let ba = channels * (bits / 8);
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&tag.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * ba as u32).to_le_bytes());
        v.extend_from_slice(&ba.to_le_bytes());
        v.extend_from_slice(&bits.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(data);
        v
    }

    fn convert(name: &str, bytes: &[u8]) -> Result<Vec<u8>, OpenFileError> {
        let dir = std::env::temp_dir().join("ppg_naudio_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p: PathBuf = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        let r = buzzer_open_file_convert(&p);
        let _ = std::fs::remove_file(&p);
        r
    }

    fn stereo16(frames: usize, l: i16, r: i16) -> Vec<u8> {
        (0..frames).flat_map(|_| [l.to_le_bytes(), r.to_le_bytes()].concat()).collect()
    }

    #[test]
    fn stereo16_44100_downsamples() {
        // 4410 frames: the read asks 17640 samples, num7 ends at 17640 * 5.5125 = 97240.5, latency
        // correction (97240.5 - 4410) / 5.5125 + 0.5 -> 16840, so 17640 - 16840 = 800 bytes.
        let silent = convert("s16_silent.wav", &wav(1, 2, 44100, 16, &stereo16(4410, 0, 0))).unwrap();
        assert_eq!(silent.len(), 800);
        // 0 maps to 127.5, truncated to 127.
        assert!(silent.iter().all(|&b| b == 127));
        // 16384/32768 = 0.5 on both sides -> mono 0.5; once the two low-pass passes settle: 191.25 -> 191.
        let dc = convert("s16_dc.wav", &wav(1, 2, 44100, 16, &stereo16(4410, 16384, 16384))).unwrap();
        assert_eq!(dc.len(), 800);
        assert!(dc[50..].iter().all(|&b| b == 191), "{:?}", &dc[40..60]);
        assert!(dc[0] < 191);
    }

    #[test]
    fn mono8_8000_passes_through() {
        // Equal rates: ratio 1, no filter, interpolation weight 0 -> the samples themselves.
        // s/128 - 1 mapped by (x + 1) * 255 / 2: 0 -> 0, 128 -> 127.5, 255 -> 254.0039, 64 -> 63.75, 192 -> 191.25.
        let out = convert("m8.wav", &wav(1, 1, 8000, 8, &[0, 128, 255, 64, 192])).unwrap();
        assert_eq!(out, vec![0, 127, 254, 63, 191]);
    }

    #[test]
    fn limits_and_errors() {
        assert_eq!(convert("big.wav", &vec![0u8; 1_000_001]), Err(OpenFileError::TooBig));
        assert_eq!(convert("16s.wav", &wav(1, 1, 8000, 8, &vec![128u8; 128000])), Err(OpenFileError::TooLong));
        assert_eq!(convert("15s.wav", &wav(1, 1, 8000, 8, &vec![128u8; 120000])).unwrap().len(), 120000);
        let ex = |s: &str| Err(OpenFileError::Exception(s.into()));
        assert_eq!(convert("empty.wav", &[]), ex("System.IO.EndOfStreamException: Unable to read beyond the end of the stream."));
        assert_eq!(convert("rifx.wav", b"RIFX\0\0\0\0WAVE"), ex("System.FormatException: Not a WAVE file - no RIFF header"));
        assert_eq!(convert("ext.wav", &wav(0xFFFE, 2, 44100, 16, &[0; 8])), ex("System.ArgumentException: Unsupported source encoding"));
        assert_eq!(convert("bits.wav", &wav(1, 1, 8000, 12, &[0; 8])), ex("System.InvalidOperationException: Unsupported bit depth"));
        assert_eq!(convert("3ch.wav", &wav(1, 3, 8000, 8, &[0; 9])), ex("System.ArgumentException: Source must be stereo"));
        assert_eq!(OpenFileError::TooBig.exception_text(), "PpgPlayerException: File size may not exceed 1MB");
    }

    #[test]
    fn clip_samples() {
        assert_eq!(clip_from_data_samples(&[0, 128, 255]), vec![-1.0, 0.0, 0.9921875]);
        assert_eq!(clip_from_data_samples(&vec![7u8; 130000]).len(), 120000);
    }

    #[test]
    fn filter_string() {
        let f: &[(&str, &[&str])] = &[("WAVE files", &["wav"]), ("All files", &["*"])];
        assert_eq!(file_picker_filter_string(f), "WAVE files\0*.wav\0All files\0*\0\0");
        let g: &[(&str, &[&str])] = &[(" Pics! ", &["PNG", ".jpg"])];
        assert_eq!(file_picker_filter_string(g), "Pics\0*.png;*.jpg\0\0");
        // char.IsLetterOrDigit: letters and decimal digits only (not Nl, No or marks); a supplementary char is two surrogates.
        assert_eq!(clean_string("\u{e9}\u{2160}\u{b2}a\u{301}1\u{a0}x\u{1d400}_"), "\u{e9}a1\u{a0}x");
    }
}
