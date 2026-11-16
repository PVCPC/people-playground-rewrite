//! Utils: the small pure helpers (angles, byte and temperature strings, bounds, contact impulses, energy transfer).
use super::physical::dot2;
use super::{d, mathf, Game};
use crate::b2::contacts2d::ContactPoint2D;

/// Utils.NormaliseAngle: Mod(degrees, 360).
pub fn normalise_angle(degrees: f32) -> f32 {
    super::physical::utils_mod(degrees, 360.0)
}

/// Utils.StandariseAngle: the angle of (cos, sin) of the radians, in degrees (-180..180].
pub fn standarise_angle(degrees: f32) -> f32 {
    let f = degrees * (std::f32::consts::PI / 180.0);
    let v = [mathf::cos(f), mathf::sin(f)];
    (crate::game::mathf::atan2(v[1], v[0])) * 57.29578
}

/// Utils.GetFormattedByteString: whole units of kB, MB and GB (powers of 1000).
pub fn get_formatted_byte_string(b: u64) -> String {
    if b < 1000 {
        format!("{b} B")
    } else if b < 1_000_000 {
        format!("{} kB", b / 1000)
    } else if b < 1_000_000_000 {
        format!("{} MB", b / 1_000_000)
    } else {
        format!("{} GB", b / 1_000_000_000)
    }
}

/// Utils.GetTemperatureUnitSuffix (TemperatureUnit: Celsius 0, Fahrenheit 1, Kelvin 2).
pub fn get_temperature_unit_suffix(unit: i32) -> &'static str {
    match unit {
        0 => "\u{b0}C",
        1 => "\u{b0}F",
        2 => "K",
        _ => "",
    }
}

/// Utils.EscapeRichText: empty for blank text; otherwise the text without the whole word "noparse" (Regex \bnoparse\b), inside <noparse> tags.
pub fn escape_rich_text(v: &str) -> String {
    if v.trim().is_empty() {
        return String::new();
    }
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let chars: Vec<char> = v.chars().collect();
    let pat: Vec<char> = "noparse".chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(&pat) && (i == 0 || !word(chars[i - 1])) && chars.get(i + pat.len()).map_or(true, |&c| !word(c)) {
            i += pat.len();
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    format!("<noparse>{out}</noparse>")
}

/// Utils.IsPointInsideBounds (strict): Bounds min and max from the center and extents.
pub fn is_point_inside_bounds(center: [f32; 2], extents: [f32; 2], point: [f32; 2]) -> bool {
    point[0] > center[0] - extents[0] && point[0] < center[0] + extents[0] && point[1] > center[1] - extents[1] && point[1] < center[1] + extents[1]
}

/// Utils.GetMaxImpulse: the largest normalImpulse that is not NaN (float.MinValue when none).
pub fn get_max_impulse(buffer: &[ContactPoint2D], count: usize) -> f32 {
    let mut num = f32::MIN;
    for i in 0..count {
        let n = buffer[i].normal_impulse;
        if !n.is_nan() {
            num = mathf::max(num, n);
        }
    }
    num
}

/// Utils.GetAverageImpulse: the mean of the finite normalImpulses.
pub fn get_average_impulse(buffer: &[ContactPoint2D], count: usize) -> f32 {
    let (mut num, mut num2) = (0.0f32, 0i32);
    for i in 0..count {
        let n = buffer[i].normal_impulse;
        if !n.is_nan() && !n.is_infinite() {
            num += n;
            num2 += 1;
        }
    }
    // A NaN answer is one this made (0 / 0 without a finite impulse, inf - inf): x86's default NaN 0xffc00000, as
    // GetAverageImpulseRemoveOutliers (ARM gives +NaN; structs2 r13 caught it on the Mac).
    let r = num / num2 as f32;
    if r.is_nan() { f32::from_bits(0xffc0_0000) } else { r }
}

impl Game {
    /// Utils.TransferEnergyDeltaTime: TransferEnergyFixedRate with GetLerpFactorDeltaTime(rate, dt).
    pub fn transfer_energy_delta_time(&mut self, a: usize, b: usize, rate: f32, dt: f32) {
        self.transfer_energy_fixed_rate(a, b, mathf::lerp_factor_dt(rate, dt));
    }

    /// Utils.SetPointVelocity(body, vel, position): blends the body's velocity toward `vel` and its spin toward the spin that moves `position` at `vel`.
    pub fn set_point_velocity(&mut self, p: usize, vel: [f32; 2], position: [f32; 2]) {
        let rb = self.physicals[p].rigidbody.clone();
        // Rigidbody2D::GetWorldCenterOfMass (180c200e0): the zero vector without a body or for a Static one.
        let c = if rb.detached.is_some() || rb.bodyType == 2 { super::V::new(0.0, 0.0) } else { self.world.body(rb.body).sweep.c };
        let vector = [c.x - position[0], c.y - position[1]];
        // vel.sqrMagnitude, Vector2.Dot and Vector2.Lerp evaluate in double, rounded at their returns.
        if (d(vel[0]) * vel[0] + d(vel[1]) * vel[1]).f() < 1e-15 {
            return;
        }
        let num = 1.0 - dot2(vector, vel).abs();
        let b = 0.0 - dot2([-vector[1], vector[0]], vel);
        let v = rb.velocity(&self.world);
        let t = mathf::clamp01(num);
        rb.set_velocity(&mut self.world, [(d(v[0]) + (d(vel[0]) - v[0]) * t).f(), (d(v[1]) + (d(vel[1]) - v[1]) * t).f()]);
        let av = rb.angular_velocity(&self.world);
        rb.set_angular_velocity(&mut self.world, mathf::lerp(av, b, 1.0 - num));
    }
}

/// Utils.AddRange(HashSet<T>, range): Add of each item in order.
pub fn add_range<T: std::hash::Hash + Eq>(set: &mut std::collections::HashSet<T>, range: impl IntoIterator<Item = T>) {
    for item in range {
        set.insert(item);
    }
}

/// What Utils.IsSerialisableType asks of a System.Type.
#[derive(Clone, Debug, PartialEq)]
pub enum ManagedType {
    /// HasElementType (arrays, pointers, references): GetElementType().
    Element(Box<ManagedType>),
    ValueType,
    Primitive,
    String,
    Float,
    Int,
    Bool,
    /// Any other reference type.
    Class,
}

/// Utils.IsSerialisableType and ReflectionCache.IsSerialisableType (its answer cached per type): an element type decides for its array; otherwise value types, primitives, string, float, int and bool.
pub fn is_serialisable_type(ty: &ManagedType) -> bool {
    match ty {
        ManagedType::Element(e) => is_serialisable_type(e),
        ManagedType::Class => false,
        _ => true,
    }
}

/// Utils.FBSF: Convert.FromBase64String (ASCII whitespace is skipped, '=' pads the end; anything else is a FormatException).
pub fn fbsf(bsf: &str) -> Vec<u8> {
    let bad = || super::menu::throw("FormatException: The input is not a valid Base-64 string as it contains a non-base 64 character, more than two padding characters, or an illegal character among the padding characters.");
    let text: Vec<char> = bsf.chars().filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n')).collect();
    if text.len() % 4 != 0 {
        bad();
    }
    let mut out = Vec::new();
    let (mut acc, mut bits, mut pad) = (0u32, 0, 0);
    for c in text {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            '=' => {
                pad += 1;
                continue;
            }
            _ => bad(),
        };
        if pad > 0 {
            bad();
        }
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if pad > 2 {
        bad();
    }
    out
}

/// Utils.DrawCross: presentation (Debug.DrawLine draws only in the editor and development gizmos); the two segments it would draw,
/// `center + Vector3.one * -size` to `center + Vector3.one * size` and `center + (-size, size)` to `center + (size, -size)`.
pub fn draw_cross_segments(center: [f32; 3], size: f32) -> [([f32; 3], [f32; 3]); 2] {
    let n = 0.0 - size;
    [
        ([center[0] + n, center[1] + n, center[2] + n], [center[0] + size, center[1] + size, center[2] + size]),
        ([center[0] + n, center[1] + size, center[2] + 0.0], [center[0] + size, center[1] + n, center[2] + 0.0]),
    ]
}

/// Utils.AreHdrRenderTexturesSupported: the rewrite renders to Rgba16Float targets (a core wgpu render attachment format), which every adapter it runs on supports.
pub fn are_hdr_render_textures_supported() -> bool {
    true
}

/// System.Random (mscorlib 4.x, Knuth's subtractive generator).
#[derive(Clone, Debug)]
pub struct NetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl NetRandom {
    /// new System.Random(): seeded with Environment.TickCount.
    pub fn new() -> Self {
        Self::with_seed(tick_count())
    }

    /// A recorded System.Random's state (inext, inextp, SeedArray).
    pub fn from_state(seed_array: [i32; 56], inext: usize, inextp: usize) -> Self {
        Self { seed_array, inext, inextp }
    }

    /// The state from_state takes, as the recorder writes it ("<inext>,<inextp>,<s0>:<s1>:..").
    pub fn state_string(&self) -> String {
        format!("{},{},{}", self.inext, self.inextp, self.seed_array.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(":"))
    }

    /// new System.Random(Seed)
    pub fn with_seed(seed: i32) -> Self {
        let mut a = [0i32; 56];
        let mut num = 161803398 - if seed == i32::MIN { i32::MAX } else { seed.abs() };
        a[55] = num;
        let mut num2 = 1i32;
        for i in 1..55 {
            let num3 = 21 * i % 55;
            a[num3] = num2;
            num2 = num.wrapping_sub(num2);
            if num2 < 0 {
                num2 = num2.wrapping_add(i32::MAX);
            }
            num = a[num3];
        }
        for _ in 1..5 {
            for k in 1..56 {
                a[k] = a[k].wrapping_sub(a[1 + (k + 30) % 55]);
                if a[k] < 0 {
                    a[k] = a[k].wrapping_add(i32::MAX);
                }
            }
        }
        Self { seed_array: a, inext: 0, inextp: 21 }
    }

    fn internal_sample(&mut self) -> i32 {
        let mut n = self.inext + 1;
        if n >= 56 {
            n = 1;
        }
        let mut np = self.inextp + 1;
        if np >= 56 {
            np = 1;
        }
        let mut v = self.seed_array[n].wrapping_sub(self.seed_array[np]);
        if v == i32::MAX {
            v -= 1;
        }
        if v < 0 {
            v = v.wrapping_add(i32::MAX);
        }
        self.seed_array[n] = v;
        self.inext = n;
        self.inextp = np;
        v
    }

    /// Random.NextDouble: Sample(), InternalSample() * (1.0 / int.MaxValue).
    pub fn next_double(&mut self) -> f64 {
        self.internal_sample() as f64 * 4.656612875245797E-10
    }
}

/// Environment.TickCount: Mono's milliseconds since boot (GetTickCount on Windows, the monotonic clock elsewhere), as
/// an int. PPG_TICK_COUNT overrides it (a fixed seed for every System.Random() the game makes).
pub fn tick_count() -> i32 {
    if let Some(v) = std::env::var("PPG_TICK_COUNT").ok().and_then(|v| v.parse().ok()) {
        return v;
    }
    #[cfg(windows)]
    {
        // SAFETY: GetTickCount takes no arguments.
        unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount() as i32 }
    }
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn clock_gettime_nsec_np(clock: u32) -> u64;
        }
        // SAFETY: CLOCK_MONOTONIC (6) is a valid clock id.
        (unsafe { clock_gettime_nsec_np(6) } / 1_000_000) as i32
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helpers() {
        assert_eq!(get_formatted_byte_string(999), "999 B");
        assert_eq!(get_formatted_byte_string(1_500_000), "1 MB");
        assert_eq!(escape_rich_text("a noparse b nonoparse"), "<noparse>a  b nonoparse</noparse>");
        assert_eq!(escape_rich_text("  "), "");
        assert_eq!(normalise_angle(-90.0), 270.0);
        assert!((standarise_angle(270.0) + 90.0).abs() < 1e-4);
        assert!(is_point_inside_bounds([0.0, 0.0], [1.0, 1.0], [0.5, 0.5]) && !is_point_inside_bounds([0.0, 0.0], [1.0, 1.0], [1.0, 0.0]));
    }
    #[test]
    fn serialisable_fbsf_cross() {
        let arr = |t| ManagedType::Element(Box::new(t));
        assert!(is_serialisable_type(&ManagedType::Bool) && is_serialisable_type(&arr(ManagedType::ValueType)));
        assert!(!is_serialisable_type(&ManagedType::Class) && !is_serialisable_type(&arr(arr(ManagedType::Class))));
        assert_eq!(fbsf("bW9kdWxvMi5qcGc="), b"modulo2.jpg");
        assert_eq!(fbsf("ZW5kLXJlc3VsdA=="), b"end-result");
        assert_eq!(fbsf(""), b"");
        let s = draw_cross_segments([1.0, 2.0, 3.0], 0.5);
        assert_eq!(s[0], ([0.5, 1.5, 2.5], [1.5, 2.5, 3.5]));
        assert_eq!(s[1], ([0.5, 2.5, 3.0], [1.5, 1.5, 3.0]));
    }

    /// new System.Random(0) and (12345): the first NextDouble values Mono gives.
    #[test]
    fn net_random_matches_mscorlib() {
        let mut r = super::NetRandom::with_seed(0);
        assert_eq!(r.next_double(), 0.7262432699679598);
        assert_eq!(r.next_double(), 0.8173253595909687);
        let mut r = super::NetRandom::with_seed(12345);
        assert_eq!(r.next_double(), 0.06674693481379511);
        assert_eq!(super::NetRandom::with_seed(i32::MIN).next_double(), 0.7262432699679598);
    }
}
