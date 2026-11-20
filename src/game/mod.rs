//! The game's own scripts (Assembly-CSharp) rewritten line by line, on top of the b2 world.
//! Field names keep the original C# names so every value can be checked against the running game
//! (tools/recorder PPG_DUMP); methods keep the original statement order and float operation order.
#![allow(non_snake_case, dead_code)]

use std::collections::HashMap;
pub mod audio;
pub mod audio_spectrum;
pub mod abuse;
pub mod ballistics;
pub mod camera_control;
pub mod context_menu;
pub mod power;
pub mod utils;
pub mod jsonnet;
pub mod legacy_mods;
pub mod cannot_share;
pub mod wh;
pub mod circulation;
pub mod container;
pub mod coroutine;
pub mod menu;
pub mod signal;
pub mod use_wire;
pub mod acts;
pub mod car;
pub mod tyre;
pub mod flask;
pub mod tool_wheel;
pub mod light;
pub mod continuous;
pub mod detector;
pub mod effector;
pub mod messages;
pub mod sort;
pub mod airfoil;
pub mod stunner;
pub mod blaster;
pub mod scifi;
pub mod effect_gun;
pub mod soap;
pub mod power_hammer;
pub mod balloon;
pub mod barrel_weapons;
pub mod metronome;
pub mod determined_anomaly;
pub mod physics_gun;
pub mod smooth_line;
pub mod activator_electrode;
pub mod props;
pub mod visual;
pub mod visual2;
pub mod ui_visual;
pub mod world_text;
pub mod button;
pub mod keycode;
pub mod laser;
pub mod attachment;
pub mod reparent;
pub mod television;
pub mod wrench;
pub mod valve;
pub mod v45_new;
pub mod line;
pub mod trail;
pub mod liquid_canister;
pub mod liquid_filter;
pub mod pulse_drum;
pub mod liquidentifier;
pub mod centrifuge;
pub mod liquid_pump;
pub mod liquid_duplicator;
pub mod liquid_leak;
pub mod no_collide;
pub mod heart_monitor;
pub mod limb_sprites;
pub mod decal_renderer;
pub mod sharp_on_all_sides;
pub mod excalibur;
pub mod blood_tank;
pub mod timed_gate;
pub mod resuscitation;
pub mod blood_wire;
pub mod link_device;
pub mod serpentine_belt;
pub mod pin;
pub mod constraint;
pub mod stats;
pub mod display;
pub mod activation_toggle;
pub mod gauge;
pub mod torch;
pub mod flare;
pub mod thermometer;
pub mod syringe;
pub mod jet_engine;
pub mod chainsaw;
pub mod automatic_sentry;
pub mod ceiling_turret;
pub mod counter;
pub mod temperature_transfer;
pub mod worm_staff;
pub mod atom_explosion;
pub mod prefab_bomb;
pub mod laser_turret;
pub mod sentry_turret;
pub mod ion_bolt;
pub mod ion_thruster;
pub mod player_prefs;
pub mod random_event;
pub mod decimator;
pub mod decorations;
pub mod obstruction_emitter;
pub mod activations;
pub mod portal;
pub mod drone;
pub mod black_hole;
pub mod immobility_field;
pub mod forcefield;
pub mod javelin;
pub mod lightning_gun;
pub mod tesla_coil;
pub mod emp;
pub mod freeze;
pub mod energy_sword;
pub mod soft_connect;
pub mod signal_converter;
pub mod flamethrower;
pub mod rocket;
pub mod piston;
pub mod liquid_outlet;
pub mod map_lift;
pub mod thruster;
pub mod devices;
pub mod dump;
pub mod explosion;
pub mod fan;
pub mod lifecycle;
pub mod destroyable;
pub mod fire;
pub mod fireworks;
pub mod conveyor;
pub mod reactor;
pub mod egg;
pub mod arc;
pub mod rope;
pub mod resize_handles;
pub mod center_of_gravity;
pub mod tool_invisibility;
pub mod spooky_stuff;
pub mod map_overlay;
pub mod disk_logger;
pub mod dangle;
pub mod hover_thruster;
pub mod hoverbox;
pub mod winch;
pub mod jukebox;
pub mod antenna;
pub mod coupler;
pub mod lightning_tool;
pub mod pyro_tool;
pub mod seismic_explosion;
pub mod poison;
pub mod grid;
pub mod global;
pub mod transmitter;
pub mod firearm;
pub mod launcher;
pub mod bell;
pub mod machinegun;
pub mod minigun;
pub mod accelerator;
pub mod archelix;
pub mod archelix_trail;
pub mod pool;
pub mod value_types;
pub mod mechanism;
pub mod slight_movement;
pub mod small_machines;
pub mod custom_fixed;
pub mod disable_trigger;
pub mod old_weapons;
pub mod script_object;
pub mod bolt;
pub mod universal_bell;
pub mod mirror;
pub mod sprite_blocks;
pub mod foliage;
pub mod breakjoint;
pub mod shake;
pub mod gorse;
pub mod glow;
pub mod liquid_spread;
pub mod lightning;
pub mod translated;
pub mod limb;
pub mod machinery;
pub mod magnet;
pub mod link;
pub mod unity_event;
pub mod map;
pub mod map_editor;
pub mod layer_editor;
pub mod misc;
pub mod motors;
pub mod person;
pub mod physical;
pub mod shatter;
pub mod spawn;
pub mod scene;
pub mod scenery;
pub mod simulate;
pub mod stab;
pub mod temperature;
pub mod thermal_display;
pub mod tool;
pub mod background_loader;
pub mod mods;
pub mod spawnable_outline;
pub mod ceras;
pub mod chemistry;
pub mod contraption;
pub mod save;
mod save_members;
/// 1.28.45's changes to save_members.
mod save_members_45;
mod save_extras;
mod save_slots;
pub mod undo;
pub mod clear;
pub mod gear;
pub mod grip;
pub mod gyro;
pub mod water;
pub mod particle_clock;
pub mod particle_rand;
pub mod particle_shape;
pub mod particle_modules;
pub mod particle_noise;
pub mod particle_collision;
pub mod particle_forces;
pub mod particle_sim;
pub mod particle_shockwave;
/// Extras (not the game's): the portal gun.
pub mod extras_glue;
/// Extras (not the game's): a portal shot hitting a person misfires.
pub mod mishap_phase;
pub mod mishap_glue;
pub mod mishap_super;

use crate::b2::{
    math::Vec2 as V,
    world::{BodyId, BodyType, JointId, World, BODY_AWAKE, BODY_BULLET, BODY_FIXED_ROTATION},
};
use crate::unity::Random;

pub use circulation::Circulation;
pub use limb::Limb;
pub use map::{MapLight, PlayAudioAtRandom};
pub use misc::{BleedingParticle, FancyBloodSplat, GForce, Grip, Node, Skin};
pub use person::Person;
pub use physical::{Physical, PhysicalProperties};

/// A C# float expression as Mono evaluates it: in double precision, rounded to float by `.f()`
/// wherever the value is stored (local, field, argument, return). Single operations round the same
/// either way; chains do not.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct D(pub f64);

pub fn d(v: f32) -> D {
    D(v as f64)
}

impl D {
    pub fn f(self) -> f32 {
        self.0 as f32
    }
}

macro_rules! d_ops {
    ($($tr:ident $m:ident $op:tt),*) => {$(
        impl std::ops::$tr<D> for D { type Output = D; fn $m(self, o: D) -> D { D(self.0 $op o.0) } }
        impl std::ops::$tr<f32> for D { type Output = D; fn $m(self, o: f32) -> D { D(self.0 $op o as f64) } }
        impl std::ops::$tr<D> for f32 { type Output = D; fn $m(self, o: D) -> D { D(self as f64 $op o.0) } }
    )*};
}
d_ops!(Add add +, Sub sub -, Mul mul *, Div div /);

impl std::ops::Neg for D {
    type Output = D;
    fn neg(self) -> D {
        D(-self.0)
    }
}

/// UnityEngine.Mathf and the Vector2/Quaternion operators, in the original float operation order.
pub mod mathf {
    use super::d;
    pub const EPSILON: f32 = f32::from_bits(1);
    pub const RAD2DEG: f32 = f32::from_bits(0x42652ee1);
    pub const DEG2RAD: f32 = f32::from_bits(0x3c8efa35);

    pub fn clamp01(v: f32) -> f32 {
        if v < 0.0 {
            0.0
        } else if v > 1.0 {
            1.0
        } else {
            v
        }
    }
    pub fn clamp(mut v: f32, min: f32, max: f32) -> f32 {
        if v < min {
            v = min;
        } else if v > max {
            v = max;
        }
        v
    }
    /// Mono evaluates each expression in double and rounds at every store (locals included);
    /// CoreModule is a Debug build, so even inlined Mathf calls round through their result local.
    pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
        (d(a) + (d(b) - a) * clamp01(t)).f()
    }
    pub fn max(a: f32, b: f32) -> f32 {
        if a > b { a } else { b }
    }
    pub fn min(a: f32, b: f32) -> f32 {
        if a < b { a } else { b }
    }
    /// Mathf.Pow/Sin/Sqrt go through System.Math in double precision.
    pub fn pow(f: f32, p: f32) -> f32 {
        pow_f64(f as f64, p as f64) as f32
    }
    /// Math.Pow, Mono's icall (checked against the game's Mono, tools/dllcheck): a NaN power is returned as it is, then a NaN
    /// base, even for a power of 0; -1 to an infinite power is +NaN; a -0 base counts as +0 and a zero result
    /// is +0. Otherwise the CRT's pow; a NaN it makes from non-NaN operands (a negative base, a non-integer power) is the CRT's
    /// -nan(ind) 0xfff8000000000000, 0xffc00000 as a float, on every platform (ARM's libm gives +NaN).
    pub fn pow_f64(x: f64, y: f64) -> f64 {
        if y.is_nan() {
            return y;
        }
        if x.is_nan() {
            return x;
        }
        if x == -1.0 && y.is_infinite() {
            return f64::from_bits(0x7ff8_0000_0000_0000);
        }
        let r = (if x == 0.0 { 0.0 } else { x }).powf(y);
        if r.is_nan() { f64::from_bits(0xfff8_0000_0000_0000) } else if r == 0.0 { 0.0 } else { r }
    }
    /// Mathf.Sin: (float)Math.Sin, which Mono's JIT makes the x87 FSIN (checked against the game's Mono, tools/dllcheck): the
    /// argument reduced by the FPU's 66-bit pi, and one of 2^63 or more given back as it is (the FPU leaves it unreduced).
    pub fn sin(f: f32) -> f32 {
        match x87_reduce(f) {
            Some((odd, r)) => (if odd { -r.sin() } else { r.sin() }) as f32,
            None if f.is_finite() && f.abs() >= 9.223372e18 => f,
            None => (f as f64).sin() as f32,
        }
    }
    /// Mathf.Cos: as sin, the x87 FCOS.
    pub fn cos(f: f32) -> f32 {
        match x87_reduce(f) {
            Some((odd, r)) => (if odd { -r.cos() } else { r.cos() }) as f32,
            None if f.is_finite() && f.abs() >= 9.223372e18 => f,
            None => (f as f64).cos() as f32,
        }
    }
    /// x87 argument reduction: f = k * pi66 + r with |r| <= pi66 / 2, exactly (pi66 = PI66 / 2^64): whether k is odd, and r.
    /// None for what needs none (|f| < 1.5: r is f) or that the FPU does not reduce (non-finite, |f| >= 2^63).
    fn x87_reduce(f: f32) -> Option<(bool, f64)> {
        const PI66: u128 = 0x3_243F_6A88_85A3_08D3; // round(pi * 2^64), 66 bits
        let a = f.abs();
        if !(1.5..9.223372e18).contains(&a) {
            return None;
        }
        // a = m * 2^e (m < 2^24, e >= -23 here): a * 2^64 mod 2 * PI66, as an integer (2^(e + 64) mod 2 * PI66 by doubling).
        let bits = a.to_bits();
        let (m, e) = ((bits & 0x7f_ffff | 0x80_0000) as u128, ((bits >> 23) & 0xff) as i32 - 150);
        let two_p = 2 * PI66;
        let mut pow2 = 1u128;
        for _ in 0..e + 64 {
            pow2 = (pow2 << 1) % two_p;
        }
        let (mut rem, mut odd) = (m * pow2 % two_p, false);
        if rem >= PI66 {
            rem -= PI66;
            odd = true;
        }
        let mut r = rem as f64;
        if rem > PI66 / 2 {
            r = -((PI66 - rem) as f64);
            odd = !odd;
        }
        let r = r / 18446744073709551616.0;
        Some((odd, if f < 0.0 { -r } else { r }))
    }
    /// Mathf.Atan2: (float)Math.Atan2, Mono's icall (checked against the game's Mono, tools/dllcheck): the CRT's atan2, but +NaN
    /// for two infinities and +0 for a zero angle (Mono's `result == -0 ? 0 : result`).
    pub fn atan2(y: f32, x: f32) -> f32 {
        atan2_f64(y as f64, x as f64) as f32
    }
    pub fn atan2_f64(y: f64, x: f64) -> f64 {
        if y.is_infinite() && x.is_infinite() {
            return f64::from_bits(0x7ff8_0000_0000_0000);
        }
        let r = y.atan2(x);
        if r == 0.0 { 0.0 } else { r }
    }
    /// Mathf.Sqrt: (float)Math.Sqrt(f); a negative f gives the x86 default NaN (0xffc00000), not ARM's positive one.
    pub fn sqrt(f: f32) -> f32 {
        let r = (f as f64).sqrt() as f32;
        if r.is_nan() && !f.is_nan() { f32::from_bits(0xffc0_0000) } else { r }
    }
    /// Vector2.magnitude: (float)Math.Sqrt(x * x + y * y), in double.
    pub fn magnitude2(v: [f32; 2]) -> f32 {
        (super::d(v[0]) * v[0] + super::d(v[1]) * v[1]).0.sqrt() as f32
    }
    /// Vector2.normalized (magnitude is (float)Math.Sqrt(x * x + y * y), in double).
    pub fn normalized2(v: [f32; 2]) -> [f32; 2] {
        let num = (super::d(v[0]) * v[0] + super::d(v[1]) * v[1]).0.sqrt() as f32;
        if num > 1e-5 {
            [v[0] / num, v[1] / num]
        } else {
            [0.0, 0.0]
        }
    }
    pub fn floor(f: f32) -> f32 {
        (f as f64).floor() as f32
    }
    pub fn repeat(t: f32, length: f32) -> f32 {
        clamp((d(t) - d(floor(t / length)) * length).f(), 0.0, length)
    }
    pub fn delta_angle(current: f32, target: f32) -> f32 {
        let mut num = repeat(target - current, 360.0);
        if num > 180.0 {
            num -= 360.0;
        }
        num
    }
    pub fn approximately(a: f32, b: f32) -> bool {
        (b - a).abs() < max(1e-6 * max(a.abs(), b.abs()), EPSILON * 8.0)
    }
    /// Utils.MapRange
    pub fn map_range(lower1: f32, upper1: f32, lower2: f32, upper2: f32, value: f32) -> f32 {
        (d(lower2) + (d(value) - lower1) * (d(upper2) - lower2) / (d(upper1) - lower1)).f()
    }
    /// Utils.Snap(float, float): Mathf.Round (to even) of v / snap, times snap; v itself for a snap of about 0.
    pub fn snap(v: f32, snap: f32) -> f32 {
        if approximately(snap, 0.0) { v } else { (v / snap).round_ties_even() * snap }
    }
    /// Utils.GetLerpFactorDeltaTime
    pub fn lerp_factor_dt(lerp_factor: f32, dt: f32) -> f32 {
        1.0 - pow(1.0 - lerp_factor, dt)
    }
    pub fn perlin(x: f32, y: f32) -> f32 {
        crate::unity::perlin_noise(x, y)
    }
    /// Utils.GetPerlin2Mapped
    pub fn perlin2_mapped(x: f32, y: f32) -> [f32; 2] {
        [perlin(x, y) * 2.0 - 1.0, perlin(y, x) * 2.0 - 1.0]
    }
    pub fn v2_lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
        let t = clamp01(t) as f64;
        let l = |a: f32, b: f32| (a as f64 + (b as f64 - a as f64) * t) as f32;
        [l(a[0], b[0]), l(a[1], b[1])]
    }
    /// Vector2.sqrMagnitude (not inlined: rounded once at its return).
    pub fn sqr_magnitude(v: [f32; 2]) -> f32 {
        (d(v[0]) * v[0] + d(v[1]) * v[1]).f()
    }
    /// Quaternion * Vector3 (UnityEngine.Quaternion operator*).
    pub fn rotate(q: [f32; 4], p: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = q;
        let (n1, n2, n3) = (x * 2.0, y * 2.0, z * 2.0);
        let (n4, n5, n6) = (x * n1, y * n2, z * n3);
        let (n7, n8, n9) = (x * n2, x * n3, y * n3);
        let (n10, n11, n12) = (w * n1, w * n2, w * n3);
        // Each component is one double-evaluated chain (Mono), rounded at the field store.
        [
            ((d(1.0) - (d(n5) + n6)) * p[0] + (d(n7) - n12) * p[1] + (d(n8) + n11) * p[2]).f(),
            ((d(n7) + n12) * p[0] + (d(1.0) - (d(n4) + n6)) * p[1] + (d(n9) - n10) * p[2]).f(),
            ((d(n8) - n11) * p[0] + (d(n9) + n10) * p[1] + (d(1.0) - (d(n4) + n5)) * p[2]).f(),
        ]
    }
}

/// One value the recorder dumps: floats as hex bits, vectors as comma lists, booleans as 0/1.
pub trait Field {
    fn parse(&mut self, text: &str) -> Result<(), String>;
    fn show(&self) -> String;
    /// Loads the field's serialized value from game data (a prefab's MonoBehaviour).
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String>;
}

fn bits(text: &str) -> Result<f32, String> {
    u32::from_str_radix(text, 16).map(f32::from_bits).map_err(|_| format!("bad float bits {text}"))
}

impl Field for f32 {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        *self = bits(text)?;
        Ok(())
    }
    fn show(&self) -> String {
        format!("{:08x}", self.to_bits())
    }
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        *self = v.f32().ok_or(format!("not a float: {v:?}"))?;
        Ok(())
    }
}
impl Field for bool {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        *self = match text {
            "1" => true,
            "0" => false,
            _ => return Err(format!("bad bool {text}")),
        };
        Ok(())
    }
    fn show(&self) -> String {
        (if *self { "1" } else { "0" }).into()
    }
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        *self = v.bool().ok_or(format!("not a bool: {v:?}"))?;
        Ok(())
    }
}
macro_rules! int_field {
    ($($t:ty),*) => {$(
        impl Field for $t {
            fn parse(&mut self, text: &str) -> Result<(), String> {
                *self = text.parse().map_err(|_| format!("bad integer {text}"))?;
                Ok(())
            }
            fn show(&self) -> String {
                self.to_string()
            }
            fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
                *self = v.i64().and_then(|i| <$t>::try_from(i).ok()).ok_or(format!("not a {}: {v:?}", stringify!($t)))?;
                Ok(())
            }
        }
    )*};
}
int_field!(i32, u32, u16, u8);
impl Field for f64 {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        *self = u64::from_str_radix(text, 16).map(f64::from_bits).map_err(|_| format!("bad double bits {text}"))?;
        Ok(())
    }
    fn show(&self) -> String {
        format!("{:016x}", self.to_bits())
    }
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        *self = match v {
            crate::assets::mono::Value::Double(d) => *d,
            _ => return Err(format!("not a double: {v:?}")),
        };
        Ok(())
    }
}
impl Field for String {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        *self = text.to_string();
        Ok(())
    }
    fn show(&self) -> String {
        self.clone()
    }
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        *self = match v {
            crate::assets::mono::Value::Str(s) => s.clone(),
            _ => return Err(format!("not a string: {v:?}")),
        };
        Ok(())
    }
}
impl<const N: usize> Field for [f32; N] {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        let parts: Vec<&str> = text.split(',').collect();
        if parts.len() != N {
            return Err(format!("expected {N} values: {text}"));
        }
        for (v, p) in self.iter_mut().zip(parts) {
            *v = bits(p)?;
        }
        Ok(())
    }
    fn show(&self) -> String {
        self.iter().map(Field::show).collect::<Vec<_>>().join(",")
    }
    /// Vector2/3/4, Quaternion and Color: the struct's float members in order.
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        let crate::assets::mono::Value::Struct(members) = v else { return Err(format!("not a struct: {v:?}")) };
        if members.len() != N {
            return Err(format!("expected {N} members: {v:?}"));
        }
        for (x, (_, m)) in self.iter_mut().zip(members) {
            x.set(m)?;
        }
        Ok(())
    }
}
impl Field for Vec<f32> {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        *self = if text.is_empty() { Vec::new() } else { text.split(',').map(bits).collect::<Result<_, _>>()? };
        Ok(())
    }
    fn show(&self) -> String {
        self.iter().map(Field::show).collect::<Vec<_>>().join(",")
    }
    /// float[]: the array's elements.
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        let crate::assets::mono::Value::Array(items) = v else { return Err(format!("not an array: {v:?}")) };
        *self = items.iter().map(|x| x.f32().ok_or(format!("not a float: {x:?}"))).collect::<Result<_, _>>()?;
        Ok(())
    }
}
/// Vector3[] as the recorder writes it: `[x,y,z;x,y,z;...]` (float bits), `null` for a null array (gapC r6: SmoothLine's vertices).
impl Field for Vec<[f32; 3]> {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        *self = if text == "null" { Vec::new() } else { text.trim_matches(['[', ']']).split(';').filter(|p| !p.is_empty()).map(|p| { let mut v = [0.0; 3]; v.parse(p).map(|_| v) }).collect::<Result<_, _>>()? };
        Ok(())
    }
    fn show(&self) -> String {
        if self.is_empty() { "null".into() } else { format!("[{}]", self.iter().map(Field::show).collect::<Vec<_>>().join(";")) }
    }
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        let crate::assets::mono::Value::Array(items) = v else { return Err(format!("not an array: {v:?}")) };
        *self = items.iter().map(|x| { let mut v = [0.0; 3]; v.set(x).map(|_| v) }).collect::<Result<_, _>>()?;
        Ok(())
    }
}
thread_local! {
    /// gapB r15: on while fncheck resets or compares an Awake/Start's reference caches (dump/ref_cache.rs); off, an
    /// `Option<usize>` link is inert (shows "null", takes only "null", changes nothing) so every other load and compare skips it.
    pub static REF_LIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
/// An arena-index link (the rewrite's reference to a component): only its null-ness, and only while REF_LIVE.
// checker limit: null-ness only (an index names no instance id without its arena); add the target id when one passes the wrong object.
impl Field for Option<usize> {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        if text != "null" {
            return Err(format!("not null: {text}"));
        }
        if REF_LIVE.with(|l| l.get()) {
            *self = None;
        }
        Ok(())
    }
    fn show(&self) -> String {
        if REF_LIVE.with(|l| l.get()) && self.is_some() { "@set".into() } else { "null".into() }
    }
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        Err(format!("Option<usize> has no serialized form: {v:?}"))
    }
}
impl Field for Option<f32> {
    fn parse(&mut self, text: &str) -> Result<(), String> {
        *self = if text == "null" { None } else { Some(bits(text)?) };
        Ok(())
    }
    fn show(&self) -> String {
        self.map_or("null".into(), |v| v.show())
    }
    fn set(&mut self, v: &crate::assets::mono::Value) -> Result<(), String> {
        Err(format!("Option<f32> has no serialized form: {v:?}"))
    }
}

/// A rewritten MonoBehaviour: `dumped` fields are the C# fields the oracle loads and compares,
/// `links` are this rewrite's references to other components and engine objects.
macro_rules! script {
    ($(#[$m:meta])* $name:ident { $($(#[$fm:meta])* $field:ident : $ty:ty = $default:expr),* $(,)? }
     links { $($(#[$lm:meta])* $lfield:ident : $lty:ty = $ldefault:expr),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug)]
        pub struct $name {
            $($(#[$fm])* pub $field: $ty,)*
            $($(#[$lm])* pub $lfield: $lty,)*
        }
        impl Default for $name {
            fn default() -> Self {
                Self { $($field: $default,)* $($lfield: $ldefault,)* }
            }
        }
        impl $name {
            pub fn fields(&mut self) -> Vec<(&'static str, &mut dyn $crate::game::Field)> {
                vec![$((stringify!($field), &mut self.$field as &mut dyn $crate::game::Field)),*]
            }
            /// The dumped fields, then the links whose type is a Field (the per-function checker loads and compares those
            /// a recording has under the same name: a private C# field the rewrite keeps as a link).
            #[allow(unused_imports)]
            pub fn fields_and_links(&mut self) -> (Vec<(&'static str, &mut dyn $crate::game::Field)>, Vec<(&'static str, &mut dyn $crate::game::Field)>) {
                use $crate::game::{LinkField, LinkOther};
                let links: Vec<(&'static str, Option<&mut dyn $crate::game::Field>)> = vec![$((stringify!($lfield), (&&$crate::game::LinkProbe::new(&mut self.$lfield)).field())),*];
                (vec![$((stringify!($field), &mut self.$field as &mut dyn $crate::game::Field)),*], links.into_iter().filter_map(|(n, f)| Some((n, f?))).collect())
            }
        }
    };
    // A subclass of a rewritten base class: `base` holds the base's fields, dumped after the subclass's own.
    ($(#[$m:meta])* $name:ident { $($(#[$fm:meta])* $field:ident : $ty:ty = $default:expr),* $(,)? }
     base { $bfield:ident : $bty:ty }
     links { $($(#[$lm:meta])* $lfield:ident : $lty:ty = $ldefault:expr),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug)]
        pub struct $name {
            $($(#[$fm])* pub $field: $ty,)*
            pub $bfield: $bty,
            $($(#[$lm])* pub $lfield: $lty,)*
        }
        impl Default for $name {
            fn default() -> Self {
                Self { $($field: $default,)* $bfield: Default::default(), $($lfield: $ldefault,)* }
            }
        }
        impl $name {
            pub fn fields(&mut self) -> Vec<(&'static str, &mut dyn $crate::game::Field)> {
                let mut v = vec![$((stringify!($field), &mut self.$field as &mut dyn $crate::game::Field)),*];
                v.extend(self.$bfield.fields());
                v
            }
            /// As the base-less form's: the base's dumped fields with the own ones.
            #[allow(unused_imports)]
            pub fn fields_and_links(&mut self) -> (Vec<(&'static str, &mut dyn $crate::game::Field)>, Vec<(&'static str, &mut dyn $crate::game::Field)>) {
                use $crate::game::{LinkField, LinkOther};
                let links: Vec<(&'static str, Option<&mut dyn $crate::game::Field>)> = vec![$((stringify!($lfield), (&&$crate::game::LinkProbe::new(&mut self.$lfield)).field())),*];
                let mut v = vec![$((stringify!($field), &mut self.$field as &mut dyn $crate::game::Field)),*];
                v.extend(self.$bfield.fields());
                (v, links.into_iter().filter_map(|(n, f)| Some((n, f?))).collect())
            }
        }
    };
}
pub(crate) use script;

/// A `script!` link seen as a Field when its type is one (autoref specialisation: `LinkField` for a Field type, `LinkOther`
/// for the rest), so `fields_and_links` lists the links a recording can hold.
pub struct LinkProbe<'a, T>(std::cell::Cell<Option<&'a mut T>>);
impl<'a, T> LinkProbe<'a, T> {
    pub fn new(v: &'a mut T) -> Self {
        LinkProbe(std::cell::Cell::new(Some(v)))
    }
}
pub trait LinkField<'a> {
    fn field(&self) -> Option<&'a mut dyn Field>;
}
impl<'a, T: Field + 'a> LinkField<'a> for &LinkProbe<'a, T> {
    fn field(&self) -> Option<&'a mut dyn Field> {
        self.0.take().map(|v| v as &mut dyn Field)
    }
}
pub trait LinkOther<'a> {
    fn field(&self) -> Option<&'a mut dyn Field>;
}
impl<'a, T> LinkOther<'a> for LinkProbe<'a, T> {
    fn field(&self) -> Option<&'a mut dyn Field> {
        None
    }
}

/// UnityEngine.Time as the scripts see it.
#[derive(Clone, Debug, Default)]
pub struct Time {
    pub time: f32,
    pub timeAsDouble: f64,
    pub fixedTime: f32,
    pub fixedTimeAsDouble: f64,
    pub deltaTime: f32,
    pub fixedDeltaTime: f32,
    pub maximumDeltaTime: f32,
    pub timeScale: f32,
    pub frameCount: i32,
    pub unscaledDeltaTime: f32,
    /// Time.smoothDeltaTime and TimeManager's weight counter (+0x80 and +0x84): the recordings lack them, so a loaded
    /// one starts at the steady state (the counter at its fixed point 1, the average at the frame's dt).
    pub smoothDeltaTime: f32,
    pub smoothCount: f32,
    /// TimeManager +0x60: the frame's time (`timeAsDouble` is the fixed step's during FixedUpdate).
    pub dynamicTimeAsDouble: f64,
    /// Time.realtimeSinceStartup
    pub realtimeSinceStartup: f64,
    /// TimeManager +0x70 and +0x40: the frame's and the fixed step's unscaled time (zero at the scene's load).
    pub unscaledTimeAsDouble: f64,
    pub fixedUnscaledTimeAsDouble: f64,
    /// TimeManager +0x4c: the fixed step's unscaledDeltaTime.
    pub fixedUnscaledDeltaTime: f32,
}

/// UnityEngine.Bounds in the xy plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub center: [f32; 2],
    pub extents: [f32; 2],
}

impl Default for Bounds {
    /// The Default map prefab's BoundingBoxBehaviour.
    fn default() -> Self {
        Bounds { center: [572.5, 496.4], extents: [578.4, 496.6] }
    }
}

impl Bounds {
    /// Utils.ContainsExpanded
    pub fn contains_expanded(&self, p: [f32; 2], expansion: f32) -> bool {
        let min = [self.center[0] - self.extents[0], self.center[1] - self.extents[1]];
        let max = [self.center[0] + self.extents[0], self.center[1] + self.extents[1]];
        p[0] > min[0] - expansion && p[0] < max[0] + expansion && p[1] > min[1] - expansion && p[1] < max[1] + expansion
    }
}

/// An f32 Game field whose Default is its C# field initialiser (given as bits; Game derives Default).
#[derive(Clone, Copy, PartialEq)]
pub struct InitF32<const BITS: u32>(pub f32);

impl<const BITS: u32> Default for InitF32<BITS> {
    fn default() -> Self {
        InitF32(f32::from_bits(BITS))
    }
}

impl<const BITS: u32> std::fmt::Debug for InitF32<BITS> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// UserPreferenceManager.Current: the values the simulation reads (config.json in the game folder).
#[derive(Clone, Debug)]
pub struct Prefs {
    pub CollisionQuality: i32,
    pub AmbientTemperatureTransfer: bool,
    pub BrainDamage: bool,
    pub AutoHealWounds: bool,
    pub StopAnimationOnDamage: bool,
    pub FragilityMultiplier: f32,
    pub LimbCrushing: bool,
    pub GorelessMode: bool,
    /// GoreShaderMode: Default 0, Legacy 1.
    pub GoreMode: i32,
    pub DropOnDeath: bool,
    pub DismembermentLooseTissue: bool,
    pub CrushForceMultiplier: f32,
    pub ProceduralFragments: bool,
    pub FancyEffects: bool,
    pub Decals: bool,
    pub ShowOutlines: bool,
    pub ZoomSensitivity: f32,
    pub DeleteWireByContextMenu: bool,
    pub LogDebugMessages: bool,
    pub ToolSelectionMode: i32,
    pub GridSnap: f32,
    pub AngleSnap: i32,
    pub SlowMotionSpeed: f32,
    pub PhysicsIterations: i32,
    pub ShowTutorial: bool,
    pub DistantSoundEffects: bool,
    pub ClampVolume: bool,
    pub TracerBullets: bool,
    pub BulletCrackWhiz: bool,
    pub ChunkyShotParticles: bool,
    pub ShakeIntensity: f32,
    /// TemperatureUnit: Celsius 0, Fahrenheit 1, Kelvin 2.
    pub TemperatureUnit: i32,
    /// ThermalVisionUpdateRate, in Hz.
    pub ThermalVisionUpdateRate: i32,
    /// Rewrite extra: the portal gun's opening length, times the default (the Mods settings tab).
    pub PortalSize: f32,
    /// Rewrite extra: the portal lines' and the gun's glow, times the default (the Mods settings tab).
    pub PortalGlow: f32,
    /// Rewrite extras: the portal mishaps a shot into a person can cause (the Mods settings tab).
    pub PortalMishapSuperposition: bool,
    pub PortalMishapFold: bool,
    pub PortalMishapPhase: bool,
}

/// TemperatureUnit's names.
pub const TEMPERATURE_UNITS: [&str; 3] = ["Celsius", "Fahrenheit", "Kelvin"];

/// Utils.CelsiusToFahrenheit: i * 9f / 5f + 32f.
pub fn celsius_to_fahrenheit(i: f32) -> f32 {
    (d(i) * 9.0 / 5.0 + 32.0).f()
}

/// Utils.CelsiusToKelvin: i + 273.15f.
pub fn celsius_to_kelvin(i: f32) -> f32 {
    i + 273.15
}

/// Utils.FahrenheitToCelsius: (i - 32f) * (5f / 9f), the constant folded to a float.
pub fn fahrenheit_to_celsius(i: f32) -> f32 {
    ((d(i) - 32.0) * (5.0f32 / 9.0f32)).f()
}

/// Utils.KelvinToCelsius: i - 273.15f.
pub fn kelvin_to_celsius(i: f32) -> f32 {
    i - 273.15
}

impl Prefs {
    /// Utils.CelsiusToPreference
    pub fn celsius_to_preference(&self, i: f32) -> f32 {
        match self.TemperatureUnit {
            1 => celsius_to_fahrenheit(i),
            2 => celsius_to_kelvin(i),
            _ => i,
        }
    }

    /// Utils.PreferenceToCelsius
    pub fn preference_to_celsius(&self, i: f32) -> f32 {
        match self.TemperatureUnit {
            1 => fahrenheit_to_celsius(i),
            2 => kelvin_to_celsius(i),
            _ => i,
        }
    }

    /// TemperatureUnit.ToString()
    pub fn temperature_unit_name(&self) -> String {
        TEMPERATURE_UNITS.get(self.TemperatureUnit as usize).map_or(self.TemperatureUnit.to_string(), |s| s.to_string())
    }
}

/// CollisionQuality.Dynamic
pub const COLLISION_DYNAMIC: i32 = 2;

impl Default for Prefs {
    fn default() -> Self {
        Self {
            CollisionQuality: COLLISION_DYNAMIC,
            AmbientTemperatureTransfer: false,
            BrainDamage: true,
            AutoHealWounds: true,
            StopAnimationOnDamage: false,
            FragilityMultiplier: 1.0,
            LimbCrushing: true,
            GorelessMode: false,
            GoreMode: 0,
            DropOnDeath: true,
            DismembermentLooseTissue: true,
            CrushForceMultiplier: 1.0,
            ProceduralFragments: false,
            FancyEffects: true,
            Decals: true,
            ShowOutlines: true,
            ZoomSensitivity: 1.0,
            DeleteWireByContextMenu: true,
            LogDebugMessages: false,
            ToolSelectionMode: tool::SELECTION_AREA,
            GridSnap: 0.25,
            AngleSnap: 15,
            SlowMotionSpeed: 20.0,
            PhysicsIterations: 32,
            ShowTutorial: true,
            DistantSoundEffects: true,
            ClampVolume: true,
            TracerBullets: true,
            BulletCrackWhiz: true,
            ChunkyShotParticles: true,
            ShakeIntensity: 1.0,
            TemperatureUnit: 0,
            ThermalVisionUpdateRate: 25,
            PortalSize: 1.0,
            PortalGlow: 1.5,
            PortalMishapSuperposition: true,
            PortalMishapFold: true,
            PortalMishapPhase: true,
        }
    }
}

impl Prefs {
    /// UserPreferenceManager.Load: config.json in the game folder (the working directory), deserialized by
    /// Newtonsoft over the defaults with unknown members ignored. Without the file, the defaults; a file that fails
    /// to read or deserialize goes to LoadFromLegacyStorage (PlayerPrefs, player_prefs.rs).
    pub fn load(game: &std::path::Path) -> Prefs {
        let path = game.join("config.json");
        if !path.exists() {
            return Prefs::default();
        }
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| Prefs::from_json(&text)) {
            Ok(p) => p,
            Err(_) => player_prefs::legacy_preferences(&player_prefs::store()),
        }
    }

    /// The members the simulation reads. An enum takes its name or its number (Newtonsoft's StringEnumConverter
    /// writes names); a value of the wrong type fails the whole file.
    pub fn from_json(text: &str) -> Result<Prefs, String> {
        let json: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let obj = json.as_object().ok_or("not an object")?;
        let mut p = Prefs::default();
        for (k, v) in obj {
            let bool = || v.as_bool().ok_or_else(|| format!("{k}: not a bool"));
            let float = || v.as_f64().map(|x| x as f32).ok_or_else(|| format!("{k}: not a number"));
            let int = || v.as_i64().and_then(|x| i32::try_from(x).ok()).ok_or_else(|| format!("{k}: not an int"));
            let enumeration = |names: &[&str]| match v {
                serde_json::Value::String(s) => names.iter().position(|n| n.eq_ignore_ascii_case(s)).map(|i| i as i32).ok_or_else(|| format!("{k}: no member {s}")),
                _ => int(),
            };
            match k.as_str() {
                "CollisionQuality" => p.CollisionQuality = enumeration(&["Discrete", "Continuous", "Dynamic"])?,
                "AmbientTemperatureTransfer" => p.AmbientTemperatureTransfer = bool()?,
                "BrainDamage" => p.BrainDamage = bool()?,
                "AutoHealWounds" => p.AutoHealWounds = bool()?,
                "StopAnimationOnDamage" => p.StopAnimationOnDamage = bool()?,
                "FragilityMultiplier" => p.FragilityMultiplier = float()?,
                "LimbCrushing" => p.LimbCrushing = bool()?,
                "GorelessMode" => p.GorelessMode = bool()?,
                "GoreMode" => p.GoreMode = enumeration(&["Default", "Legacy"])?,
                "DropOnDeath" => p.DropOnDeath = bool()?,
                "DismembermentLooseTissue" => p.DismembermentLooseTissue = bool()?,
                "CrushForceMultiplier" => p.CrushForceMultiplier = float()?,
                "ProceduralFragments" => p.ProceduralFragments = bool()?,
                "FancyEffects" => p.FancyEffects = bool()?,
                "Decals" => p.Decals = bool()?,
                "ShowOutlines" => p.ShowOutlines = bool()?,
                "ZoomSensitivity" => p.ZoomSensitivity = float()?,
                "DeleteWireByContextMenu" => p.DeleteWireByContextMenu = bool()?,
                "LogDebugMessages" => p.LogDebugMessages = bool()?,
                "ToolSelectionMode" => p.ToolSelectionMode = enumeration(&["Area", "Mass", "Depth"])?,
                "GridSnap" => p.GridSnap = float()?,
                "AngleSnap" => p.AngleSnap = int()?,
                "SlowMotionSpeed" => p.SlowMotionSpeed = float()?,
                "PhysicsIterations" => p.PhysicsIterations = int()?,
                "ShowTutorial" => p.ShowTutorial = bool()?,
                "DistantSoundEffects" => p.DistantSoundEffects = bool()?,
                "ClampVolume" => p.ClampVolume = bool()?,
                "TracerBullets" => p.TracerBullets = bool()?,
                "BulletCrackWhiz" => p.BulletCrackWhiz = bool()?,
                "ChunkyShotParticles" => p.ChunkyShotParticles = bool()?,
                "ShakeIntensity" => p.ShakeIntensity = float()?,
                "TemperatureUnit" => p.TemperatureUnit = enumeration(&TEMPERATURE_UNITS)?,
                _ => {}
            }
        }
        Ok(p)
    }
}

/// System.Collections.Generic.Dictionary enumeration order: insertion order, with removed slots
/// reused by later insertions (most recently freed first).
#[derive(Clone, Debug)]
pub struct CsDict<K, V> {
    entries: Vec<Option<(K, V)>>,
    free: Vec<usize>,
}

impl<K, V> Default for CsDict<K, V> {
    fn default() -> Self {
        Self { entries: Vec::new(), free: Vec::new() }
    }
}

impl<K: PartialEq + Copy, V> CsDict<K, V> {
    pub fn get(&self, key: K) -> Option<&V> {
        self.entries.iter().flatten().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        self.entries.iter_mut().flatten().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
    pub fn insert(&mut self, key: K, value: V) {
        match self.free.pop() {
            Some(slot) => self.entries[slot] = Some((key, value)),
            None => self.entries.push(Some((key, value))),
        }
    }
    pub fn remove(&mut self, key: K) -> bool {
        match self.entries.iter().position(|e| matches!(e, Some((k, _)) if *k == key)) {
            Some(slot) => {
                self.entries[slot] = None;
                self.free.push(slot);
                true
            }
            None => false,
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len() - self.free.len()
    }
    pub fn keys(&self) -> Vec<K> {
        self.entries.iter().flatten().map(|(k, _)| *k).collect()
    }
    pub fn iter(&self) -> impl Iterator<Item = &(K, V)> {
        self.entries.iter().flatten()
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut (K, V)> {
        self.entries.iter_mut().flatten()
    }
}

/// UnityEngine.Rigidbody2D over a b2 body (RigidbodyType2D: 0 dynamic, 1 kinematic, 2 static).
#[derive(Clone, Debug)]
pub struct Rigidbody {
    pub body: BodyId,
    pub bodyType: i32,
    pub collisionDetectionMode: i32,
    pub interpolation: i32,
    pub movement: MovementState,
    /// Set once Rigidbody2D::Cleanup has destroyed the b2 body (m_Body null): the component's own mass fields.
    pub detached: Option<crate::b2::world::RigidbodyMass>,
    /// The Rigidbody2D component's instance id.
    pub component: i32,
    /// Rigidbody2D.simulated (+0x48): PhysicsManager2D::Simulate's write-back and Update's interpolation skip a body
    /// without it.
    pub simulated: bool,
    /// While detached, the component's linear drag, angular drag and gravity scale (the next Create reads them).
    pub detached_props: [f32; 3],
    /// Rigidbody2D.constraints (+0x4c): RigidbodyConstraints2D bits (1 freeze x, 2 freeze y, 4 freeze rotation).
    pub constraints: i32,
}

/// RigidbodyMovementState2D (Rigidbody2D+0x98): the body pose before and after the last physics step, and the
/// pose PhysicsManager2D::Update interpolates between them for the Transform.
#[derive(Clone, Copy, Debug, Default)]
pub struct MovementState {
    pub previous: ([f32; 3], [f32; 4]),
    pub current: ([f32; 3], [f32; 4]),
    pub body: ([f32; 3], [f32; 4]),
    pub interpolating: bool,
    /// In PhysicsScene2D.m_RigidbodyMovementStates (m_MovementIndex != -1).
    // Membership only: the index is the scripts' Start order and no result reads it.
    pub listed: bool,
    /// Rigidbody2D.MovePosition's target (+0x70) and its pending byte (+0x7d).
    pub linear_target: [f32; 2],
    pub linear_pending: bool,
    /// Rigidbody2D.MoveRotation's target in radians (+0x78) and its pending byte (+0x7e).
    pub angular_target: f32,
    pub angular_pending: bool,
    /// The body velocities SetupMovementStates replaced (+0x64, +0x6c); ResetLinear/AngularMoveState put them back.
    pub saved_velocity: [f32; 2],
    pub saved_angular_velocity: f32,
    /// The component's linear and angular drag while SetupMovementStates has the body's damping at 0 (natively
    /// Rigidbody2D+0x3c/+0x40, which the reset writes back; otherwise the body's damping is the component's).
    pub zeroed_damping: [Option<f32>; 2],
}

impl Rigidbody {
    /// The movement state's interpolation flag with a non-None mode (both checked by the interpolation pass).
    pub fn interpolating(&self) -> bool {
        self.movement.listed && self.movement.interpolating && self.interpolation != 0
    }

    pub fn velocity(&self, w: &World) -> [f32; 2] {
        if self.detached.is_some() || self.bodyType == 2 {
            return [0.0, 0.0];
        }
        let b = w.body(self.body);
        [b.linear_velocity.x, b.linear_velocity.y]
    }
    /// Rigidbody2D::SetVelocity
    pub fn set_velocity(&self, w: &mut World, v: [f32; 2]) {
        if self.bodyType == 2 || self.detached.is_some() {
            return;
        }
        let b = w.body_mut(self.body);
        if b.kind != BodyType::Static {
            if 0.0 < v[1] * v[1] + v[0] * v[0] {
                b.flags |= BODY_AWAKE;
                b.sleep_time = 0.0;
            }
            b.linear_velocity = V::new(v[0], v[1]);
        }
    }
    /// Rigidbody2D::GetAngularVelocity, degrees per second.
    pub fn angular_velocity(&self, w: &World) -> f32 {
        if self.detached.is_some() || self.bodyType == 2 { 0.0 } else { w.body(self.body).angular_velocity * mathf::RAD2DEG }
    }
    /// Rigidbody2D::SetAngularVelocity
    pub fn set_angular_velocity(&self, w: &mut World, degrees: f32) {
        if self.bodyType == 2 || self.detached.is_some() {
            return;
        }
        let b = w.body_mut(self.body);
        let v = degrees * mathf::DEG2RAD;
        if b.kind != BodyType::Static && b.flags & BODY_FIXED_ROTATION == 0 {
            if 0.0 < v * v {
                b.flags |= BODY_AWAKE;
                b.sleep_time = 0.0;
            }
            b.angular_velocity = v;
        }
    }
    /// Rigidbody2D::AddTorque, ForceMode2D.Force
    pub fn add_torque(&self, w: &mut World, torque: f32) {
        if self.detached.is_some() || self.bodyType != 0 {
            return;
        }
        let b = w.body_mut(self.body);
        if b.kind == BodyType::Dynamic {
            if b.flags & BODY_AWAKE == 0 {
                b.sleep_time = 0.0;
                b.flags |= BODY_AWAKE;
            }
            b.torque += torque;
        }
    }
    /// Rigidbody2D::SetConstraints (180c23360): stored, then b2Body SetFixedPositionX/Y and SetFixedRotation
    /// (180bba380: on a change, angular velocity 0 and ResetMassData) on a live body.
    pub fn set_constraints(&mut self, w: &mut World, v: i32) {
        self.constraints = v;
        if self.detached.is_some() {
            return;
        }
        w.set_fixed_position_x(self.body, v & 1 != 0);
        w.set_fixed_position_y(self.body, v & 2 != 0);
        let fixed = v & 4 != 0;
        let b = w.body_mut(self.body);
        if (b.flags & BODY_FIXED_ROTATION != 0) == fixed {
            return;
        }
        b.angular_velocity = 0.0;
        if fixed { b.flags |= BODY_FIXED_ROTATION } else { b.flags &= !BODY_FIXED_ROTATION }
        w.reset_mass_data(self.body);
    }
    /// Rigidbody2D.gravityScale: the b2 body's, or the component's own while detached.
    pub fn gravity_scale(&self, w: &World) -> f32 {
        if self.detached.is_some() { self.detached_props[2] } else { w.body(self.body).gravity_scale }
    }
    /// Rigidbody2D.gravityScale = v (Rigidbody2D::SetGravityScale 180c23620): clamped to ±1e6; a change of more than
    /// 1e-6 reaches the body, and a nonzero scale then wakes it. After Cleanup destroyed the body only m_GravityScale changes.
    pub fn set_gravity_scale(&mut self, w: &mut World, v: f32) {
        let v = if -1_000_000.0 > v { -1_000_000.0 } else if 1_000_000.0 < v { 1_000_000.0 } else { v };
        if self.detached.is_some() {
            self.detached_props[2] = v;
            return;
        }
        let old = w.body(self.body).gravity_scale;
        if (v - old).abs() <= 1e-6 {
            if v != old {
                unimplemented!("mod-only: Rigidbody2D.gravityScale: m_GravityScale apart from the body's (a change within 1e-6)");
            }
            return;
        }
        w.body_mut(self.body).gravity_scale = v;
        if v != 0.0 {
            wake_for_joint(w, self.body);
        }
    }
    pub fn mass(&self, w: &World) -> f32 {
        self.detached.unwrap_or_else(|| w.body(self.body).rigidbody).mass
    }
    /// Rigidbody2D::AddForce (180c1bf80) with ForceMode2D.Impulse: a Dynamic Rigidbody2D's body wakes and its
    /// velocity gains impulse times the inverse mass.
    pub fn add_force_impulse(&self, w: &mut World, impulse: [f32; 2]) {
        if self.detached.is_some() || self.bodyType != 0 {
            return;
        }
        let b = w.body_mut(self.body);
        if b.kind != BodyType::Dynamic {
            return;
        }
        if b.flags & BODY_AWAKE == 0 {
            b.flags |= BODY_AWAKE;
            b.sleep_time = 0.0;
        }
        b.linear_velocity = V::new(impulse[0] * b.inv_mass + b.linear_velocity.x, impulse[1] * b.inv_mass + b.linear_velocity.y);
    }

    /// Rigidbody2D::AddTorque (180c1c320) with ForceMode2D.Impulse: a Dynamic Rigidbody2D's body wakes and its
    /// angular velocity gains the impulse times the inverse inertia.
    pub fn add_torque_impulse(&self, w: &mut World, impulse: f32) {
        if self.detached.is_some() || self.bodyType != 0 {
            return;
        }
        let b = w.body_mut(self.body);
        if b.kind != BodyType::Dynamic {
            return;
        }
        if b.flags & BODY_AWAKE == 0 {
            b.flags |= BODY_AWAKE;
            b.sleep_time = 0.0;
        }
        b.angular_velocity = impulse * b.inv_i + b.angular_velocity;
    }
    /// Rigidbody2D::AddForce (180c1bf80) with ForceMode2D.Force: a dynamic Rigidbody2D that has a body.
    pub fn add_force(&self, w: &mut World, force: [f32; 2]) {
        if self.detached.is_some() || self.bodyType != 0 {
            return;
        }
        w.body_mut(self.body).apply_force_to_center(V::new(force[0], force[1]));
    }
    /// Rigidbody2D::AddRelativeForce (180c1c1b0) with ForceMode2D.Force: the force turned by the body's rotation.
    pub fn add_relative_force(&self, w: &mut World, force: [f32; 2]) {
        if self.detached.is_some() || self.bodyType != 0 {
            return;
        }
        let q = w.body(self.body).xf.q;
        w.body_mut(self.body).apply_force_to_center(V::new(q.c * force[0] - q.s * force[1], q.s * force[0] + q.c * force[1]));
    }
    /// Rigidbody2D::AddRelativeForce (180c1c1b0) with ForceMode2D.Impulse: the impulse turned by the body's rotation,
    /// then a Dynamic body wakes and its velocity gains it times the inverse mass.
    pub fn add_relative_force_impulse(&self, w: &mut World, impulse: [f32; 2]) {
        if self.detached.is_some() || self.bodyType != 0 {
            return;
        }
        let b = w.body_mut(self.body);
        let (x, y) = (b.xf.q.c * impulse[0] - b.xf.q.s * impulse[1], b.xf.q.s * impulse[0] + b.xf.q.c * impulse[1]);
        if b.kind != BodyType::Dynamic {
            return;
        }
        if b.flags & BODY_AWAKE == 0 {
            b.flags |= BODY_AWAKE;
            b.sleep_time = 0.0;
        }
        b.linear_velocity = V::new(x * b.inv_mass + b.linear_velocity.x, y * b.inv_mass + b.linear_velocity.y);
    }
    /// Rigidbody2D::AddForceAtPosition (180c1c030) with ForceMode2D.Impulse.
    pub fn add_impulse_at_position(&self, w: &mut World, impulse: [f32; 2], point: [f32; 2]) {
        if self.detached.is_some() || self.bodyType != 0 {
            return;
        }
        w.body_mut(self.body).apply_impulse_at(V::new(impulse[0], impulse[1]), V::new(point[0], point[1]));
    }
    /// Rigidbody2D::AddForceAtPosition (180c1c030) with ForceMode2D.Force: a dynamic Rigidbody2D that has a body.
    pub fn add_force_at_position(&self, w: &mut World, force: [f32; 2], point: [f32; 2]) {
        if self.detached.is_some() || self.bodyType != 0 {
            return;
        }
        w.body_mut(self.body).apply_force(V::new(force[0], force[1]), V::new(point[0], point[1]));
    }
    /// Rigidbody2D::GetPointVelocity (180c1fa10), after the AutoSyncTransforms the caller runs: zero without a body
    /// or for a static Rigidbody2D.
    pub fn point_velocity(&self, w: &World, point: [f32; 2]) -> [f32; 2] {
        if self.detached.is_some() || self.bodyType == 2 {
            return [0.0, 0.0];
        }
        let v = w.body(self.body).velocity_at(V::new(point[0], point[1]));
        [v.x, v.y]
    }
    /// Rigidbody2D::SetCollisionDetectionMode (CollisionDetectionMode2D: 0 discrete, 1 continuous)
    pub fn set_collision_detection_mode(&mut self, w: &mut World, mode: i32) {
        self.collisionDetectionMode = mode;
        if self.detached.is_some() {
            return;
        }
        let b = w.body_mut(self.body);
        if mode == 1 {
            b.flags |= BODY_BULLET;
        } else {
            b.flags &= !BODY_BULLET;
        }
    }
    /// Rigidbody2D::GetRotation, degrees.
    pub fn rotation(&self, w: &World) -> f32 {
        w.body(self.body).sweep.a * mathf::RAD2DEG
    }
}

/// UnityEngine.HingeJoint2D: the managed-side motor and limits plus its b2 revolute joint.
#[derive(Clone, Debug)]
pub struct Hinge {
    pub joint: Option<JointId>,
    pub useMotor: bool,
    /// JointMotor2D (motorSpeed degrees/s, maxMotorTorque)
    pub motor: [f32; 2],
    pub useLimits: bool,
    /// JointAngleLimits2D (min, max) degrees
    pub limits: [f32; 2],
    pub breakForce: f32,
    pub breakTorque: f32,
    /// connectedBody: the Rigidbody2D's b2 body (None: the scene's ground body).
    pub connected: Option<BodyId>,
    pub anchor: [f32; 2],
    pub connectedAnchor: [f32; 2],
    pub enabled: bool,
    /// The component was destroyed (a broken joint).
    pub destroyed: bool,
    /// A pending Object.Destroy(Joint): the DelayedCallManager time it runs at.
    pub destroy_at: Option<f64>,
}

/// b2Body::SetAwake(true) as the joint setters inline it: the sleep timer always restarts.
fn wake_for_joint(w: &mut World, id: BodyId) {
    let b = w.body_mut(id);
    b.sleep_time = 0.0;
    if b.kind == BodyType::Static {
        b.flags &= !BODY_AWAKE;
        b.linear_velocity = V::new(0.0, 0.0);
        b.angular_velocity = 0.0;
        b.force = V::new(0.0, 0.0);
        b.torque = 0.0;
    } else {
        b.flags |= BODY_AWAKE;
    }
}

/// The clamps of HingeJoint2D::SetMotor: speed to [-1e6, 1e6] and torque to [0, 1e6] (a NaN becomes the lower bound).
pub(crate) fn clamp_motor(speed: f32, torque: f32) -> (f32, f32) {
    let mut s = -1_000_000.0;
    if -1_000_000.0 <= speed && { s = 1_000_000.0; speed <= 1_000_000.0 } {
        s = speed;
    }
    let mut t = 0.0;
    if 0.0 <= torque && { t = 1_000_000.0; torque <= 1_000_000.0 } {
        t = torque;
    }
    (s, t)
}

/// b2RevoluteJoint::EnableMotor (180bc1740) on a live joint: both bodies wake when the flag changes.
pub(crate) fn revolute_enable_motor(w: &mut World, joint: Option<crate::b2::world::JointId>, v: bool) {
    let Some(id) = joint else { return };
    let j = w.joints[id].as_ref().unwrap();
    if j.revolute().def.enable_motor == v {
        return;
    }
    let (a, b) = (j.body_a, j.body_b);
    wake_for_joint(w, a);
    wake_for_joint(w, b);
    w.joints[id].as_mut().unwrap().revolute_mut().def.enable_motor = v;
}

/// The live half of HingeJoint2D::SetMotor: EnableMotor(true), then the speed (degrees to radians) and the maximum torque
/// when they differ, each waking both bodies.
pub(crate) fn revolute_write_motor(w: &mut World, joint: Option<crate::b2::world::JointId>, s: f32, t: f32) {
    let Some(id) = joint else { return };
    let j = w.joints[id].as_ref().unwrap();
    let (a, b) = (j.body_a, j.body_b);
    let def = &j.revolute().def;
    let (enable, old_speed, old_torque) = (def.enable_motor, def.motor_speed, def.max_motor_torque);
    let speed = s * mathf::DEG2RAD;
    if !enable {
        wake_for_joint(w, a);
        wake_for_joint(w, b);
        w.joints[id].as_mut().unwrap().revolute_mut().def.enable_motor = true;
    }
    if speed != old_speed {
        wake_for_joint(w, a);
        wake_for_joint(w, b);
        w.joints[id].as_mut().unwrap().revolute_mut().def.motor_speed = speed;
    }
    if t != old_torque {
        wake_for_joint(w, a);
        wake_for_joint(w, b);
        w.joints[id].as_mut().unwrap().revolute_mut().def.max_motor_torque = t;
    }
}

impl Hinge {
    /// HingeJoint2D::GetJointAngle, degrees.
    pub fn joint_angle(&self, w: &World) -> f32 {
        let Some(id) = self.joint else { return 0.0 };
        let j = w.joints[id].as_ref().unwrap();
        (w.body(j.body_b).sweep.a - w.body(j.body_a).sweep.a - j.revolute().def.reference_angle) * mathf::RAD2DEG
    }
    /// HingeJoint2D::GetReferenceAngle, degrees.
    pub fn reference_angle(&self, w: &World) -> f32 {
        self.joint.map_or(0.0, |id| w.joints[id].as_ref().unwrap().revolute().def.reference_angle * mathf::RAD2DEG)
    }
    /// Joint2D.jointSpeed of a HingeJoint2D (180c45ea0 -> 180bef740): b2RevoluteJoint::GetJointSpeed (bodyB's angular
    /// velocity minus bodyA's) times Rad2Deg, 0 without a b2 joint.
    pub fn joint_speed(&self, w: &World) -> f32 {
        let Some(id) = self.joint else { return 0.0 };
        let j = w.joints[id].as_ref().unwrap();
        (w.body(j.body_b).angular_velocity - w.body(j.body_a).angular_velocity) * mathf::RAD2DEG
    }
    /// HingeJoint2D::SetUseMotor (180bfdda0): stored, then b2RevoluteJoint::EnableMotor (180bc1740) on a live joint,
    /// which wakes both bodies when the flag changes.
    pub fn set_use_motor(&mut self, w: &mut World, v: bool) {
        self.useMotor = v;
        revolute_enable_motor(w, self.joint, v);
    }
    /// HingeJoint2D::SetMotor (the `Joint.motor = motor` setter).
    pub fn set_motor(&mut self, w: &mut World, speed: f32, torque: f32) {
        let (s, t) = clamp_motor(speed, torque);
        self.motor = [s, t];
        self.useMotor = true;
        revolute_write_motor(w, self.joint, s, t);
    }
    /// HingeJoint2D::SetLimits (the `Joint.limits = limits` setter).
    pub fn set_limits(&mut self, w: &mut World, min: f32, max: f32) {
        let clamp = |v: f32| {
            let mut r = -1_000_000.0;
            if -1_000_000.0 <= v && { r = 1_000_000.0; v <= 1_000_000.0 } {
                r = v;
            }
            r
        };
        self.limits = [clamp(min), clamp(max)];
        self.useLimits = true;
        let Some(id) = self.joint else { return };
        let j = w.joints[id].as_ref().unwrap();
        let (a, b) = (j.body_a, j.body_b);
        if !j.revolute().def.enable_limit {
            wake_for_joint(w, a);
            wake_for_joint(w, b);
            let r = w.joints[id].as_mut().unwrap().revolute_mut();
            r.def.enable_limit = true;
            r.impulse[2] = 0.0;
        }
        let (mut lower, mut upper) = (self.limits[0] * mathf::DEG2RAD, self.limits[1] * mathf::DEG2RAD);
        if upper < lower {
            std::mem::swap(&mut lower, &mut upper);
        }
        let def = &w.joints[id].as_ref().unwrap().revolute().def;
        if lower != def.lower_angle || upper != def.upper_angle {
            wake_for_joint(w, a);
            wake_for_joint(w, b);
            let r = w.joints[id].as_mut().unwrap().revolute_mut();
            r.def.lower_angle = lower;
            r.def.upper_angle = upper;
            r.impulse[2] = 0.0;
        }
    }
    /// Joint2D::SetBreakForce / SetBreakTorque clamp to [0, inf].
    pub fn set_break_force(&mut self, v: f32) {
        self.breakForce = if 0.0 <= v && v <= f32::INFINITY { v } else if 0.0 <= v { f32::INFINITY } else { 0.0 };
    }
    pub fn set_break_torque(&mut self, v: f32) {
        self.breakTorque = if 0.0 <= v && v <= f32::INFINITY { v } else if 0.0 <= v { f32::INFINITY } else { 0.0 };
    }
}

/// What a bare collider's Transform hangs from.
#[derive(Clone, Copy, Debug, Default)]
pub enum BareAnchor {
    Physical(usize),
    Node(usize),
    #[default]
    Root,
}

/// A Collider2D's GameObject without a PhysicalBehaviour: its local TRS and those of its ancestors up to the nearest
/// physical or transform node (object first). Every Rigidbody2D's object is one of those, so nothing in the chain
/// moves.
#[derive(Clone, Debug)]
pub struct BareCollider {
    pub anchor: BareAnchor,
    pub chain: Vec<crate::unity::transform::Trs>,
    /// The GameObject's MonoBehaviour classes (SendMessage receivers).
    pub scripts: Vec<String>,
    /// WaterImpactBehaviour.Splash.
    pub water_impact: Option<crate::assets::db::ObjRef>,
    /// GameObject.activeSelf.
    pub active_self: bool,
    /// `transform.root` of the GameObject as (GameObject id, Transform id): its clone's root.
    pub root: (i32, i32),
}

impl BareCollider {
    /// Whether SendMessage(message) finds a receiver among its scripts (the classes declaring the method).
    pub fn receives(&self, receivers: &[&str]) -> bool {
        self.scripts.iter().any(|s| receivers.contains(&s.as_str()))
    }
}

/// A Rigidbody2D on a GameObject without a PhysicalBehaviour (FoliageBehaviour leaves, grenade pins, the Tank's
/// hinge): its Transform is `transform_nodes[node]`, whose local pose the body writes.
#[derive(Clone, Debug)]
pub struct NodeBody {
    pub node: usize,
    pub rigidbody: Rigidbody,
    /// CheckForDrivenByParentRigidbody: the nearest ancestor Rigidbody2D's body.
    pub driven_by: Option<usize>,
}

/// What a SpriteRenderer draws its sprite with: flips, drawMode, size, adaptiveModeThreshold, tileMode == Adaptive.
pub type SpriteDraw = ([bool; 2], i32, [f32; 2], f32, bool);

/// A SpriteRenderer on a GameObject without a PhysicalBehaviour: where its Transform hangs, and what it draws.
#[derive(Clone, Debug)]
pub struct Renderer {
    pub anchor: BareAnchor,
    pub chain: Vec<crate::unity::transform::Trs>,
    pub sprite: (crate::assets::db::ObjRef, [f32; 4], i16),
    /// The SpriteRenderer's sortingLayerID.
    pub layer: i32,
    /// Its GameObject's layer and its first material.
    pub game_layer: i32,
    pub material: Option<crate::assets::db::ObjRef>,
    /// The SpriteRenderer component's instance id.
    pub id: i32,
    /// flipX/flipY, drawMode, size.
    pub draw: SpriteDraw,
    /// GameObject.activeInHierarchy (SetActive keeps it current).
    pub visible: bool,
    /// Renderer.enabled
    pub enabled: bool,
    /// SpriteRenderer::GetSpriteBounds: the local bounds (center, extents).
    pub bounds: ([f32; 3], [f32; 3]),
}

/// The object owning a Rigidbody2D: a physical, or a `node_rigidbodies` entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyOwner {
    Physical(usize),
    Node(usize),
}

/// A Joint2D in PhysicsScene2D.m_JointBreakLimits: a hinge, a limb's gore-string SpringJoint2D, a FixedJoint2D, or
/// a Fixed/Slider/SpringJoint2D cloned from a prefab (`Anchored`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BreakJoint {
    Hinge(usize),
    Spring(usize),
    Fixed(usize),
    Anchored(usize),
}

/// What PhysicsScene2D's break-limit walk reads from a prefab-cloned Fixed/Slider/SpringJoint2D.
#[derive(Clone, Copy, Debug)]
pub struct AnchoredJoint {
    pub break_force: f32,
    pub break_torque: f32,
    /// m_Joint (+0x58)
    pub joint: Option<crate::b2::world::JointId>,
    /// The `Game::hinges` entry that took a spawned hinge's slot over: its break limit is `BreakJoint::Hinge` from then on.
    pub hinge: Option<usize>,
    /// The joint component's instance id.
    pub component: i32,
}

/// A Transform as the scripts read it: world position and rotation, local scale, and the hierarchy's stored
/// local position and rotation (None: derive them from the world pose).
#[derive(Clone, Copy, Debug, Default)]
pub struct Pose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub localScale: [f32; 3],
    pub local: Option<([f32; 3], [f32; 4])>,
}

impl Pose {
    pub fn up(&self) -> [f32; 3] {
        mathf::rotate(self.rotation, [0.0, 1.0, 0.0])
    }
    pub fn right(&self) -> [f32; 3] {
        mathf::rotate(self.rotation, [1.0, 0.0, 0.0])
    }
}

/// A spawnable MonoBehaviour with Unity Update, FixedUpdate or LateUpdate messages, by its arena index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Behaviour {
    Person(usize),
    /// CirculationBehaviour (its BloodContainer.Update) by limb.
    Circulation(usize),
    Skin(usize),
    Grip(usize),
    GoreString(usize),
    GForce(usize),
    Splat(usize),
    Bleeding(usize),
    Machinery(usize),
    Lava(usize),
    Destroyable(usize),
    Spawner(usize),
    HeatRect(usize),
    Explosive(usize),
    DeleteAfter(usize),
    LineRendererTargets(usize),
    InfiniteLiquid(usize),
    ReturnToPool(usize),
    SlightMovement(usize),
    /// ScherbewerferBehaviour.Use, NeedlerBehaviour.Use, NeedleBehaviour, MachineGunProjectileBehaviour, PlaneBehaviour, WingBehaviour and
    /// GlassShardBehaviour: indexes into old_weapons.rs's vectors.
    Scherbewerfer(usize),
    Needler(usize),
    Needle(usize),
    Raygun(usize),
    WhOpening(usize),
    Wh(usize),
    IgnoreParentSize(usize),
    CustomFixedConstrained(usize),
    MachineGunProjectile(usize),
    Plane(usize),
    Wing(usize),
    GlassShard(usize),
    Shockwave(usize),
    /// PressureGridController (`Game::pressure_grids`).
    PressureGrid(usize),
    /// RadiationGridController (`Game::radiation_grids`).
    RadiationGrid(usize),
    /// DisableWhenControlTrigger (`Game::disable_triggers`).
    DisableTrigger(usize),
    FireMap(usize),
    Firearm(usize),
    Launcher(usize),
    Bell(usize),
    MachineGun(usize),
    Minigun(usize),
    Accelerator(usize),
    AcceleratorBolt(usize),
    ArchelixCaster(usize),
    ArchelixBolt(usize),
    /// A PoolGenerator pool's ObjectPoolBehaviour.
    ObjectPool(pool::PoolRef),
    UniversalBell(usize),
    Counter(usize),
    Pointer(usize),
    Valve(usize),
    LiquidCanister(usize),
    LiquidFilter(usize),
    PulseDrum(usize),
    Liquidentifier(usize),
    Centrifuge(usize),
    Vibration(usize),
    OnlyExistWith(usize),
    OffsetGrid(usize),
    WindController(usize),
    ParticleMachine(usize),
    Egg(usize),
    LiquidPump(usize),
    LiquidDuplicator(usize),
    LiquidHole(usize),
    TimedGate(usize),
    Lagbox(usize),
    Gate(usize),
    LiquidLevelGate(usize),
    ActivationTransformer(usize),
    HoverThruster(usize),
    Hovercar(usize),
    Winch(usize),
    Conveyor(usize),
    ReactorCore(usize),
    ReactorA5Display(usize),
    ReactorConduit(usize),
    ReactorMeltdown(usize),
    DoAtTime(usize),
    IndustrialLift(usize),
    AcidPool(usize),
    Vacuum(usize),
    PointAt(usize),
    Ep(usize),
    Jukebox(usize),
    AntennaReceiver(usize),
    Coupler(usize),
    /// The ToolController's LightningTool.
    LightningTool,
    LightningEntity(usize),
    /// The ToolController's PyroPowerTool.
    PyroTool,
    PyroEntity(usize),
    SeismicExplosion(usize),
    EnergyBolt(usize),
    Transmitter(usize),
    Defibrillator(usize),
    CardiopulmonaryBypass(usize),
    HeartMonitor(usize),
    PowerGauge(usize),
    Excalibur(usize),
    SharpOnAllSides(usize),
    BloodTank(usize),
    LiquidOutlet(usize),
    ToggleableMirror(usize),
    PointToVelocity(usize),
    /// InverterBehaviour: the `Game::inverters` index.
    Inverter(usize),
    /// ObstructionEmitterBehaviour: the `Game::obstruction_emitters` index.
    ObstructionEmitter(usize),
    AndroidSpark(usize),
    Car(usize),
    Tyre(usize),
    Flask(usize),
    LiquidContainer(usize),
    TruckDoor(usize),
    Glowtube(usize),
    CathodeLight(usize),
    Led(usize),
    Detector(usize),
    Sticky(usize),
    Generator(usize),
    Floodlight(usize),
    SpikeGrenade(usize),
    HeatingElement(usize),
    RayGun(usize),
    Bulb(usize),
    PointLiquid(usize),
    Thrusterbed(usize),
    Piston(usize),
    MapLift(usize),
    JointMotorSound(usize),
    Flamethrower(usize),
    Rocket(usize),
    BreakJointOnBurn(usize),
    SoundOnShake(usize),
    EmitOnShake(usize),
    SerialisationHelper(usize),
    GorseDrooling(usize),
    GorseEye(usize),
    Airfoil(usize),
    Stunner(usize),
    Balloon(usize),
    Metronome(usize),
    DeterminedAnomaly(usize),
    PhysicsGun(usize),
    SmoothLine(usize),
    ActivatorElectrode(usize),
    /// A `visual::Visual` by its class.
    Visual(&'static str, usize),
    Thermometer(usize),
    Torch(usize),
    Flare(usize),
    Button(usize),
    Display(usize),
    Laser(usize),
    /// Scope, Explosive, Flashlight and Incendiary AttachmentBehaviours (class FirearmAttachmentBehaviour's Use).
    FirearmAttachment(usize),
    BatteryAttachment(usize),
    Television(usize),
    InfraredThermometer(usize),
    Syringe(usize),
    JetEngine(usize),
    Chainsaw(usize),
    /// CeilingTurretBehaviour.Start: the `Game::ceiling_turrets` index.
    CeilingTurret(usize),
    /// TemperatureTransferBehaviour.FixedUpdate: the `Game::temperature_transfers` index.
    TemperatureTransfer(usize),
    /// WormStaffBehaviour.Update: the `Game::worm_staffs` index.
    WormStaff(usize),
    /// NewAtomBombExplosionBehaviour.Update: the `Game::atom_explosions` index.
    AtomExplosion(usize),
    /// AtomBombBehaviour.Use: the `Game::prefab_bombs` index.
    AtomBomb(usize),
    /// SeismicChargeBehaviour.Use: the `Game::prefab_bombs` index.
    SeismicCharge(usize),
    /// LaserTurretBehaviour.FixedUpdate: the `Game::laser_turrets` index.
    LaserTurret(usize),
    /// SentryTurretBehaviour.Start/Update/FixedUpdate: the `Game::sentry_turrets` index.
    SentryTurret(usize),
    /// SentryTurretGunBehaviour.Update/FixedUpdate: the `Game::sentry_guns` index.
    SentryTurretGun(usize),
    /// IonBoltBehaviour.Update: the `Game::ion_bolts` index.
    IonBolt(usize),
    /// RandomEventBehaviour.Update: the `Game::random_events` index.
    RandomEvent(usize),
    Decimator(usize),
    Drone(usize),
    BlackHole(usize),
    ImmobilityField(usize),
    Forcefield(usize),
    /// LightningGunBehaviour.Update/FixedUpdate: the `Game::lightning_guns` index.
    LightningGun(usize),
    /// JavelinMissileBehaviour.FixedUpdate: the `Game::javelin_missiles` index.
    JavelinMissile(usize),
    /// JavelinBehaviour.FixedUpdate: the `Game::javelins` index.
    Javelin(usize),
    TeslaCoil(usize),
    TeslaBolt(usize),
    /// EMPBehaviour.Use: the `Game::emps` index.
    Emp(usize),
    EnergySword(usize),
    /// EnergySwordMotionBlur: the `Game::energy_blurs` index.
    EnergySwordBlur(usize),
    SignalConverter(usize),
    Blaster(usize),
    BeamCannon(usize),
    BeamRepeater(usize),
    AndroidLaser(usize),
    RayCannon(usize),
    Railgun(usize),
    WeatherLightning(usize),
    LaserReceiver(usize),
    KeyTrigger(usize),
    Fireworks(usize),
    Blasterbolt(usize),
    Beamformer(usize),
    Scifi40(usize),
    HealthGun(usize),
    FreezeGun(usize),
    /// BloodWireBehaviour (a component added at run time by the BloodWireTool).
    BloodWire(usize),
    /// A FixedCableBehaviour on a root without a PhysicalBehaviour (blood_wire.rs `RootWire`).
    RootWire(usize),
    PhaseLink(usize),
    /// A phase link's VisualDeletableDetachedToolBehaviour: the link and its end (0 source, 1 target).
    LinkEnd(usize, usize),
    Belt(usize),
    Pin(usize),
    /// PoisonSpreadBehaviour and its subclasses (`Game::poisons`, poison.rs).
    Poison(usize),
    Translated(translated::TBehaviour),
    /// 1.28.45's new classes (v45_new.rs).
    V45(v45_new::V),
    /// The Main scene's ThermalVisionTempDisplayBehaviour (thermal_display.rs).
    ThermalDisplay,
    /// The Main scene's ResizeHandles once SetActive(true) put it back into the manager (resize_handles.rs); before
    /// that it updates at its scene place.
    ResizeHandles,
}

impl Behaviour {
    fn class(self) -> &'static str {
        match self {
            Behaviour::Person(_) => "PersonBehaviour",
            Behaviour::Circulation(_) => "CirculationBehaviour",
            Behaviour::Skin(_) => "SkinMaterialHandler",
            Behaviour::Grip(_) => "GripBehaviour",
            Behaviour::GoreString(_) => "GoreStringBehaviour",
            Behaviour::GForce(_) => "GForceMeasureBehaviour",
            Behaviour::Splat(_) => "FancyBloodSplatController",
            Behaviour::Bleeding(_) => "BleedingParticleBehaviour",
            Behaviour::Machinery(_) => "DamagableMachineryBehaviour",
            Behaviour::Lava(_) => "LavaBehaviour",
            Behaviour::Destroyable(_) => "DestroyableBehaviour",
            Behaviour::Spawner(_) => "RandomPrefabSpawnerBehaviour",
            Behaviour::HeatRect(_) => "TemperatureTargetRectangleBehaviour",
            Behaviour::Explosive(_) => "ExplosiveBehaviour",
            Behaviour::DeleteAfter(_) => "DeleteAfterTime",
            Behaviour::LineRendererTargets(_) => "LineRendererTargetsBehaviour",
            Behaviour::InfiniteLiquid(_) => "InfiniteLiquidSourceBehaviour",
            Behaviour::ReturnToPool(_) => "ReturnToPoolAfterTime",
            Behaviour::SlightMovement(_) => "SlightMovement",
            Behaviour::Scherbewerfer(_) => "ScherbewerferBehaviour",
            Behaviour::Needler(_) => "NeedlerBehaviour",
            Behaviour::Needle(_) => "NeedleBehaviour",
            Behaviour::Raygun(_) => "RaygunBehaviour",
            Behaviour::WhOpening(_) => "WhOpening",
            Behaviour::Wh(_) => "wh",
            Behaviour::IgnoreParentSize(_) => "IgnoreParentSize",
            Behaviour::CustomFixedConstrained(_) => "CustomFixedConstrainedBehaviour",
            Behaviour::MachineGunProjectile(_) => "MachineGunProjectileBehaviour",
            Behaviour::Plane(_) => "PlaneBehaviour",
            Behaviour::Wing(_) => "WingBehaviour",
            Behaviour::GlassShard(_) => "GlassShardBehaviour",
            Behaviour::Shockwave(_) => "ShockwaveBehaviour",
            Behaviour::PressureGrid(_) => "PressureGridController",
            Behaviour::RadiationGrid(_) => "RadiationGridController",
            Behaviour::DisableTrigger(_) => "DisableWhenControlTrigger",
            Behaviour::PointToVelocity(_) => "PointToVelocityBehaviour",
            Behaviour::Inverter(_) => "InverterBehaviour",
            Behaviour::ObstructionEmitter(_) => "ObstructionEmitterBehaviour",
            Behaviour::AndroidSpark(_) => "AndroidSparkCreator",
            Behaviour::Car(_) => "CarBehaviour",
            Behaviour::Tyre(_) => "TyreDeflationBehaviour",
            Behaviour::Flask(_) => "GasTankBehaviour",
            Behaviour::LiquidContainer(_) => "LiquidContainerController",
            Behaviour::TruckDoor(_) => "TruckDoorBehaviour",
            Behaviour::Glowtube(_) => "GlowtubeBehaviour",
            Behaviour::CathodeLight(_) => "CathodeLightBehaviour",
            Behaviour::Led(_) => "LEDBulbBehaviour",
            Behaviour::Detector(_) => "DetectorBehaviour",
            Behaviour::Generator(_) => "GeneratorBehaviour",
            Behaviour::Floodlight(_) => "SingleFloodlightBehaviour",
            Behaviour::SpikeGrenade(_) => "SpikeGrenadeBehaviour",
            Behaviour::HeatingElement(_) => "HeatingElementBehaviour",
            Behaviour::RayGun(_) => "TemperatureRayGunBehaviour",
            Behaviour::Bulb(_) => "BulbBehaviour",
            Behaviour::PointLiquid(_) => "PointLiquidTransferBehaviour",
            Behaviour::Thrusterbed(_) => "ThrusterbedBehaviour",
            Behaviour::Piston(_) => "PistonBehaviour",
            Behaviour::MapLift(_) => "MapLiftBehaviour",
            Behaviour::ReactorA5Display(_) => "ReactorA5DisplayBehaviour",
            Behaviour::JointMotorSound(_) => "JointMotorSoundBehaviour",
            Behaviour::Flamethrower(_) => "FlamethrowerBehaviour",
            Behaviour::Rocket(_) => "LaunchedRocketBehaviour",
            Behaviour::BreakJointOnBurn(_) => "BreakJointOnBurnBehaviour",
            Behaviour::SoundOnShake(_) => "SoundOnShakeBehaviour",
            Behaviour::EmitOnShake(_) => "EmitParticlesOnShakeBehaviour",
            Behaviour::SerialisationHelper(_) => "ParticleSystemSerialisationHelper",
            Behaviour::GorseDrooling(_) => "GorseDroolingBehaviour",
            Behaviour::GorseEye(_) => "GorseEyeBehaviour",
            Behaviour::Airfoil(_) => "AirfoilBehaviour",
            Behaviour::Stunner(_) => "StunnerBehaviour",
            Behaviour::Balloon(_) => "BalloonBehaviour",
            Behaviour::Metronome(_) => "MetronomeBehaviour",
            Behaviour::DeterminedAnomaly(_) => "DeterminedAnomaly",
            Behaviour::PhysicsGun(_) => "PhysicsGunBehaviour",
            Behaviour::SmoothLine(_) => "SmoothLineControllerBehaviour",
            Behaviour::ActivatorElectrode(_) => "ActivatorElectrodeBehaviour",
            Behaviour::Visual(class, _) => class,
            Behaviour::Thermometer(_) => "ThermometerBehaviour",
            Behaviour::InfraredThermometer(_) => "InfraredThermometer",
            Behaviour::Torch(_) => "TorchBehaviour",
            Behaviour::Flare(_) => "FlareBehaviour",
            Behaviour::Button(_) => "ButtonBehaviour",
            Behaviour::Laser(_) => "LaserBehaviour",
            Behaviour::FirearmAttachment(_) => "FirearmAttachmentBehaviour",
            Behaviour::BatteryAttachment(_) => "BatteryAttachmentBehaviour",
            Behaviour::Television(_) => "TelevisionBehaviour",
            Behaviour::Display(_) => "DisplayBehaviour",
            Behaviour::JetEngine(_) => "JetEngineBehaviour",
            Behaviour::Chainsaw(_) => "ChainsawBehaviour",
            Behaviour::CeilingTurret(_) => "CeilingTurretBehaviour",
            Behaviour::TemperatureTransfer(_) => "TemperatureTransferBehaviour",
            Behaviour::WormStaff(_) => "WormStaffBehaviour",
            Behaviour::AtomExplosion(_) => "NewAtomBombExplosionBehaviour",
            Behaviour::AtomBomb(_) => "AtomBombBehaviour",
            Behaviour::SeismicCharge(_) => "SeismicChargeBehaviour",
            Behaviour::LaserTurret(_) => "LaserTurretBehaviour",
            Behaviour::SentryTurret(_) => "SentryTurretBehaviour",
            Behaviour::SentryTurretGun(_) => "SentryTurretGunBehaviour",
            Behaviour::IonBolt(_) => "IonBoltBehaviour",
            Behaviour::RandomEvent(_) => "RandomEventBehaviour",
            Behaviour::Decimator(_) => "DecimatorBehaviour",
            Behaviour::Drone(_) => "DroneBehaviour",
            Behaviour::BlackHole(_) => "BlackHoleBehaviour",
            Behaviour::ImmobilityField(_) => "ImmobilityFieldBehaviour",
            Behaviour::Forcefield(_) => "ForcefieldGeneratorBehaviour",
            Behaviour::LightningGun(_) => "LightningGunBehaviour",
            Behaviour::JavelinMissile(_) => "JavelinMissileBehaviour",
            Behaviour::Javelin(_) => "JavelinBehaviour",
            Behaviour::TeslaCoil(_) => "TeslaCoilBehaviour",
            Behaviour::TeslaBolt(_) => "TeslaCoilLightningBoltBehaviour",
            Behaviour::Emp(_) => "EMPBehaviour",
            Behaviour::EnergySword(_) => "EnergySwordBehaviour",
            Behaviour::EnergySwordBlur(_) => "EnergySwordMotionBlur",
            Behaviour::Blaster(_) => "BlasterBehaviour",
            Behaviour::BeamCannon(_) => "BeamCannonBehaviour",
            Behaviour::BeamRepeater(_) => "BeamRepeaterBehaviour",
            Behaviour::AndroidLaser(_) => "AndroidLaserBehaviour",
            Behaviour::RayCannon(_) => "RayCannonBehaviour",
            Behaviour::Railgun(_) => "RailgunBehaviour",
            Behaviour::WeatherLightning(_) => "WeatherLightningBehaviour",
            Behaviour::LaserReceiver(_) => "LaserReceiverBehaviour",
            Behaviour::KeyTrigger(_) => "KeyTriggerBehaviour",
            Behaviour::Fireworks(_) => "FireworksBehaviour",
            Behaviour::Blasterbolt(_) => "BlasterboltBehaviour",
            Behaviour::Beamformer(_) => "BeamformerBehaviour",
            Behaviour::Scifi40(_) => "GenericScifiWeapon40Behaviour",
            Behaviour::HealthGun(_) => "HealthGunBehaviour",
            Behaviour::FreezeGun(_) => "FreezeGunBehaviour",
            Behaviour::BloodWire(_) => "BloodWireBehaviour",
            Behaviour::RootWire(_) => "FixedCableBehaviour",
            Behaviour::PhaseLink(_) => "PhaseLinkBehaviour",
            Behaviour::LinkEnd(..) => "VisualDeletableDetachedToolBehaviour",
            Behaviour::Belt(_) => "SerpentineBeltBehaviour",
            Behaviour::Pin(_) => "PinBehaviour",
            Behaviour::Poison(_) => "PoisonSpreadBehaviour",
            Behaviour::SignalConverter(_) => "SignalConverterBehaviour",
            // Every subclass has the lifecycle of SyringeBehaviour.
            Behaviour::Syringe(_) => "SyringeBehaviour",
            Behaviour::Sticky(_) => "StickyGrenadeBehaviour",
            Behaviour::FireMap(_) => "LocalFirePropagationMap",
            Behaviour::Firearm(_) => "FirearmBehaviour",
            Behaviour::Launcher(_) => "ProjectileLauncherBehaviour",
            Behaviour::Bell(_) => "BellBehaviour",
            Behaviour::MachineGun(_) => "MachineGunBehaviour",
            Behaviour::Minigun(_) => "MinigunBehaviour",
            Behaviour::Accelerator(_) => "AcceleratorGunBehaviour",
            Behaviour::AcceleratorBolt(_) => "AcceleratorBoltBehaviour",
            Behaviour::ArchelixCaster(_) => "ArchelixCasterBehaviour",
            Behaviour::ArchelixBolt(_) => "ArchelixCasterBoltBehaviour",
            Behaviour::UniversalBell(_) => "UniversalBellBehaviour",
            Behaviour::Counter(_) => "CounterBehaviour",
            Behaviour::Pointer(_) => "PointerBehaviour",
            Behaviour::Valve(_) => "ValveBehaviour",
            Behaviour::LiquidCanister(_) => "LiquidCanisterBehaviour",
            Behaviour::LiquidFilter(_) => "LiquidFilterBehaviour",
            Behaviour::PulseDrum(_) => "PulseDrumBehaviour",
            Behaviour::Liquidentifier(_) => "LiquidentifierBehaviour",
            Behaviour::Centrifuge(_) => "CentrifugeBehaviour",
            Behaviour::Vibration(_) => "VibrationBehaviour",
            Behaviour::OnlyExistWith(_) => "OnlyExistWith",
            Behaviour::OffsetGrid(_) => "OffsetGridBehaviour",
            Behaviour::WindController(_) => "WindControllerBehaviour",
            Behaviour::ParticleMachine(_) => "ParticleMachineBehaviour",
            Behaviour::Egg(_) => "EggBehaviour",
            Behaviour::LiquidPump(_) => "LiquidPumpBehaviour",
            Behaviour::LiquidDuplicator(_) => "LiquidDuplicatorBehaviour",
            Behaviour::LiquidHole(_) => "LiquidContainerHoleBehaviour",
            Behaviour::TimedGate(_) => "TimedGateBehaviour",
            Behaviour::Lagbox(_) => "LagboxBehaviour",
            Behaviour::Gate(_) => "GateBehaviour",
            Behaviour::LiquidLevelGate(_) => "LiquidLevelGateBehaviour",
            Behaviour::ActivationTransformer(_) => "ActivationTransformerBehaviour",
            Behaviour::HoverThruster(_) => "HoverThrusterBehaviour",
            Behaviour::Hovercar(_) => "HovercarBehaviour",
            Behaviour::Winch(_) => "WinchBehaviour",
            Behaviour::Conveyor(_) => "ConveyorBeltBehaviour",
            Behaviour::ReactorCore(_) => "ReactorCoreBehaviour",
            Behaviour::ReactorA5Display(_) => "ReactorA5DisplayBehaviour",
            Behaviour::ReactorConduit(_) => "ReactorConduitBehaviour",
            Behaviour::ReactorMeltdown(_) => "ReactorMeltdownBehaviour",
            Behaviour::DoAtTime(_) => "DoAtTime",
            Behaviour::IndustrialLift(_) => "IndustrialLiftBehaviour",
            Behaviour::AcidPool(_) => "AcidPoolBehaviour",
            Behaviour::Vacuum(_) => "VacuumComponent",
            Behaviour::PointAt(_) => "PointAtBehaviour",
            Behaviour::Ep(_) => "ep",
            Behaviour::Jukebox(_) => "JukeboxBehaviour",
            Behaviour::AntennaReceiver(_) => "AntennaReceiverBehaviour",
            Behaviour::Coupler(_) => "AdhesiveCouplerBehaviour",
            Behaviour::LightningTool => "LightningTool",
            Behaviour::LightningEntity(_) => "LightningToolEntityBehaviour",
            Behaviour::PyroTool => "PyroPowerTool",
            Behaviour::PyroEntity(_) => "PyroPowerToolEntity",
            Behaviour::SeismicExplosion(_) => "SeismicChargeExplosionBehaviour",
            Behaviour::EnergyBolt(_) => "EnergyVesselLightningBehaviour",
            Behaviour::Transmitter(_) => "TransmitterBehaviour",
            Behaviour::Defibrillator(_) => "DefibrillatorBehaviour",
            Behaviour::CardiopulmonaryBypass(_) => "CardiopulmonaryBypassMachineBehaviour",
            Behaviour::HeartMonitor(_) => "HeartMonitorBehaviour",
            Behaviour::PowerGauge(_) => "PowerGaugeBehaviour",
            Behaviour::Excalibur(_) => "ExcaliburBehaviour",
            Behaviour::SharpOnAllSides(_) => "SharpOnAllSidesBehaviour",
            Behaviour::BloodTank(_) => "BloodTankBehaviour",
            Behaviour::LiquidOutlet(_) => "LiquidOutletBehaviour",
            Behaviour::ToggleableMirror(_) => "ToggleableMirrorBehaviour",
            Behaviour::Translated(t) => t.class(),
            Behaviour::V45(v) => v.class(),
            Behaviour::ThermalDisplay => "ThermalVisionTempDisplayBehaviour",
            Behaviour::ResizeHandles => "ResizeHandles",
            Behaviour::ObjectPool(_) => "ObjectPoolBehaviour",
        }
    }
}

/// Whether a MonoBehaviour class declares or inherits the Unity message (lifecycle.rs).
pub fn declares(class: &str, message: &str) -> bool {
    lifecycle::LIFECYCLE.iter().chain(v45_new::LIFECYCLE).find(|(k, _)| *k == class).is_some_and(|(_, m)| m.contains(&message))
}

/// The passes of Unity's FixedBehaviourManager, BehaviourManager and LateBehaviourManager.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Fixed,
    Update,
    Late,
}

/// The spawnable behaviours in Unity's three behaviour managers: by execution order, then AddBehaviour order
/// (BaseBehaviourManager::AddBehaviour 180632040 appends to the order's pending list, IntegrateLists 180634580
/// appends that to the list at the next pass). A behaviour is added once Awake ran on an active, enabled one.
#[derive(Default)]
pub struct Behaviours {
    pub fixed: Vec<(i32, Behaviour)>,
    pub update: Vec<(i32, Behaviour)>,
    pub late: Vec<(i32, Behaviour)>,
}

impl Behaviours {
    pub fn list(&self, phase: Phase) -> &Vec<(i32, Behaviour)> {
        match phase {
            Phase::Fixed => &self.fixed,
            Phase::Update => &self.update,
            Phase::Late => &self.late,
        }
    }

    /// MonoBehaviour::AddToManager: into each manager whose message the class declares.
    pub fn add(&mut self, order: i32, b: Behaviour) {
        for (list, m) in [(&mut self.fixed, "FixedUpdate"), (&mut self.update, "Update"), (&mut self.late, "LateUpdate")] {
            if declares(b.class(), m) {
                let at = list.partition_point(|&(o, _)| o <= order);
                list.insert(at, (order, b));
            }
        }
    }

    /// MonoBehaviour::RemoveFromManager
    pub fn remove(&mut self, b: Behaviour) {
        for list in [&mut self.fixed, &mut self.update, &mut self.late] {
            list.retain(|&(_, k)| k != b);
        }
    }
}

/// A ParticleSystem of a spawned prefab, played by a script: where its Transform hangs, whose it is and what drives it.
#[derive(Clone, Debug)]
pub struct EmbeddedPs {
    /// The ParticleSystem's GameObject in its prefab.
    pub go: crate::assets::db::ObjRef,
    pub anchor: BareAnchor,
    pub chain: Vec<crate::unity::transform::Trs>,
    /// The owning behaviour's component id: the system is destroyed with it.
    pub owner: i32,
    pub drive: EmbeddedDrive,
    /// ShapeModule.texture set by a script (VaseDebrisBehaviour.SetSprite): the sampled texture in place of m_Texture.
    pub shape_texture: Option<crate::assets::db::ObjRef>,
    /// Its object has been active: a Free system on an inactive object waits for the activation's Awake.
    pub awake: bool,
    /// gapC r11: a script's `emission.enabled = x` (ps_set_emission); None: the asset's m_Enabled.
    pub emission: Option<bool>,
}

/// A script's call on an embedded ParticleSystem.
#[derive(Clone, Copy, Debug)]
pub enum PsCall {
    Play,
    /// Stop, with m_CurFrameTime at the call (stopTime).
    Stop(f64),
    /// Emit(n), with the system's world position and rotation at the call.
    Emit(i32, [f32; 3], [f32; 4]),
    /// Emit(EmitParams, 1), with the same pose.
    EmitParams(crate::particles::EmitParams, [f32; 3], [f32; 4]),
    /// A script's module writes (rate and speed multipliers, startColor).
    Writes(crate::particles::PsWrites),
}

#[derive(Clone, Copy, Debug)]
pub enum EmbeddedDrive {
    /// FireworksBehaviour: Play(false) once Activated.
    Firework(usize),
    /// GorseDroolingBehaviour: playing from the start, emission.enabled = isAlive once its FixedUpdate has set it.
    Gorse(usize),
    /// AndroidLaserBehaviour.Fire: `particles.Play()`.
    AndroidLaser(usize),
    /// ParticleMachineBehaviour.UpdateActivation on Smoke/Fire/ElectricityParticles (`k`), with the serialised Emit
    /// flags its OnEnable reads before Start.
    ParticleMachine { k: usize, init: [bool; 3], enabled: bool },
    /// IonThrusterBehaviour.UpdateEffectState on FX.
    IonThruster,
    /// MagnetBehaviour.UpdateActivated/OnDisable on its particleSystem.
    Magnet,
    /// EmitParticlesOnShakeBehaviour: Emit(n) on its own system, which plays as its playOnAwake says.
    EmitOnShake,
    /// ParticleSystemSerialisationHelper.Start: Play or Stop by IsPlaying (`serialisation_helpers`).
    SerialisationHelper(usize),
    /// PyroPowerToolEntity `e`'s `pyro_tool::PYRO_SYSTEMS[k]`: Play/Stop and Emit(EmitParams) (`ps_calls`).
    Pyro { e: usize, k: u8 },
    /// FirearmBehaviour `f`'s MuzzleSmoke clone: flash.Play(withChildren) in Shoot.
    Muzzle(usize),
    /// wh: the system plays from its Awake; only its renderer's visibility is read.
    Wh,
    /// A system no script drives: it plays as its playOnAwake says (owner = its own component id).
    Free,
}

/// An Instantiate or pooled request of a prefab that may hold particle systems, for the presentation.
#[derive(Clone, Debug)]
pub struct EffectSpawn {
    pub prefab: crate::assets::db::ObjRef,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: f32,
    /// BloodImpactBehaviour.SetColor's colour.
    pub color: Option<[f32; 4]>,
    /// The clone's root Transform: its particles stop with it.
    pub root: Option<i32>,
    /// ParticleSystem.Play(withChildren: true) of this object of the prefab, not the clone's own start.
    pub subtree: Option<crate::assets::db::ObjRef>,
    /// WaterImpactBehaviour.Shot's writes to the clone's ParticleSystems.
    pub water: Option<WaterSplash>,
    /// The pooled clone this is (its root's stop returns it).
    pub pooled: Option<Pooled>,
    /// The clone's root is a child of this physical's Transform (SetParent with the world pose kept).
    pub parent: Option<EffectParent>,
    /// Its children (by name) that are inactive (activeSelf false): their systems do not play.
    pub off: Vec<&'static str>,
    /// Rewrite extra (extras_glue): `color` tints every system of the prefab, not only a BloodImpactBehaviour's.
    pub tint_all: bool,
}

/// The clone root's local pose under a physical's Transform.
#[derive(Clone, Copy, Debug)]
pub struct EffectParent {
    pub physical: usize,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    /// Its ShapeModule.spriteRenderer is this physical's SpriteRenderer (PhysicalBehaviour.SetupSpawnPrefab).
    pub sprite_shape: bool,
}

/// A pooled particle clone: a `splash_pools` item, a `blood_puffs` entry or a `generic_pools` item with a
/// PoolableParticleSystemHelper.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pooled {
    Splash(usize, usize),
    BloodPuff(usize),
    Generic(usize, usize),
}

/// shape.radius, main.startSpeedMultiplier, and main.startSizeMultiplier of each ParticleSystem's GameObject.
#[derive(Clone, Debug)]
pub struct WaterSplash {
    pub radius: f32,
    pub speed: f32,
    pub sizes: Vec<(crate::assets::db::ObjRef, f32)>,
}

/// Everything the rewritten scripts touch: the b2 world, engine state and the component arenas.
/// Limb-owned components (circulation, node, skin, gforce) share the limb's index.
#[derive(Default)]
pub struct Game {
    pub time: Time,
    pub random: Random,
    pub prefs: Prefs,
    /// PhysicalBehaviour.AmbientTemperature (the map's EnvironmentalSettings.Ambient_temperature).
    pub AmbientTemperature: f32,
    pub world: World,
    pub properties: Vec<PhysicalProperties>,
    /// Global.PhysicalObjectsInWorld order.
    pub physicals: Vec<Physical>,
    /// Extras (not the game's): the portal gun's guns, portals and the bodies in their doorway.
    pub extras: extras_glue::Extras,
    /// Extras: the portal mishaps under way.
    pub mishap: mishap_glue::Mishaps,
    /// AmbientTemperatureGridBehaviour.Instance (the Main scene's grid; nothing in the dumps records it).
    pub ambient_grid: temperature::SceneGrid,
    pub limbs: Vec<Limb>,
    pub circulations: Vec<Circulation>,
    pub nodes: Vec<Node>,
    pub skins: Vec<Skin>,
    pub gforces: Vec<GForce>,
    pub grips: Vec<Grip>,
    /// A recording's FixedJoint2Ds on a hand that its grip no longer references (an earlier CreateJoint's: Attach adds
    /// a new joint without destroying the old one): `fixed_joints` entries, compared with the grips' joints.
    pub loose_fixed_joints: Vec<usize>,
    pub people: Vec<Person>,
    pub bleeding: Vec<BleedingParticle>,
    pub splats: Vec<FancyBloodSplat>,
    /// Behaviours instantiated since the last delayed-Start pass, in Awake order.
    pub pending_starts: Vec<misc::PendingStart>,
    /// A Start whose GameObject was inactive at its turn (an earlier Start on it SetActive(false)), by its component's id:
    /// Unity does not call it then, but once the object is active again (`set_active_scripts`).
    pub deferred_starts: Vec<(i32, misc::PendingStart)>,
    /// The joints below each pending HingeJointLimitAutofixBehaviour host (`PendingStart::HingeJointLimitAutofixItem`'s
    /// index), kept current by `refresh_autofix_joints` until its Start.
    pub autofix_joints: Vec<misc::AutofixJoints>,
    /// The joints of each HingeJointLimitAutofixBehaviour (not a person's) after its Start, by component id, for OnFlip.
    pub autofix_started: HashMap<i32, misc::AutofixJoints>,
    pub joint_break_sounds: Vec<misc::JointBreakSound>,
    pub behaviours: Behaviours,
    pub machinery: Vec<machinery::Machinery>,
    pub lavas: Vec<scenery::Lava>,
    pub spawners: Vec<scenery::Spawner>,
    pub heat_rects: Vec<scenery::HeatRect>,
    pub random_sprites: Vec<misc::RandomSprite>,
    pub return_to_pools: Vec<pool::ReturnToPoolAfterTime>,
    pub slight_movements: Vec<slight_movement::SlightMovement>,
    pub custom_fixeds: Vec<custom_fixed::CustomFixedConstrained>,
    pub scherbewerfers: Vec<old_weapons::Scherbewerfer>,
    pub needlers: Vec<old_weapons::Needler>,
    pub needles: Vec<old_weapons::Needle>,
    pub static_needles: Vec<old_weapons::StaticNeedle>,
    pub raygun_beams: Vec<old_weapons::Raygun>,
    pub wh_openings: Vec<old_weapons::WhOpening>,
    pub whs: Vec<wh::Wh>,
    pub cannot_shares: Vec<cannot_share::CannotShare>,
    /// ParentConstraint components, in creation order (ConstraintManager's order).
    pub parent_constraints: Vec<constraint::ParentConstraint>,
    pub ignore_parent_sizes: Vec<explosion::IgnoreParentSize>,
    pub machine_gun_projectiles: Vec<old_weapons::MachineGunProjectile>,
    pub planes: Vec<old_weapons::Plane>,
    pub wings: Vec<old_weapons::Wing>,
    pub glass_shards: Vec<old_weapons::GlassShard>,
    pub shockwaves: Vec<old_weapons::Shockwave>,
    pub pressure_grids: Vec<grid::PressureGrid>,
    pub radiation_grids: Vec<grid::RadiationGrid>,
    pub disable_triggers: Vec<disable_trigger::DisableWhenControlTrigger>,
    /// PlayerPrefs.SetInt calls of this run, by registry value name.
    pub player_prefs_set: std::collections::HashMap<String, i32>,
    /// PoolableSizeHelper: (component id, localScaleAtStart).
    pub poolable_size_helpers: Vec<(i32, Option<[f32; 3]>)>,
    /// Global.ActiveUiBlockers
    pub ActiveUiBlockers: i32,
    /// InfiniteLiquidSourceBehaviour: (LiquidID, the BloodContainer it found in Awake).
    pub infinite_liquids: Vec<(String, container::Owner)>,
    pub weightless_helpers: Vec<physical::WeightlessHelper>,
    /// MuteBehaviour.Muted.
    pub mutes: Vec<bool>,
    /// Each MuteBehaviour's component id (its Sources are `Audio::source_fields`).
    pub mute_ids: Vec<i32>,
    pub destroyables: Vec<destroyable::Destroyable>,
    pub fire_maps: Vec<fire::FireMap>,
    pub firearms: Vec<firearm::Firearm>,
    pub launchers: Vec<launcher::Launcher>,
    pub bells: Vec<bell::Bell>,
    pub machine_guns: Vec<machinegun::MachineGun>,
    pub miniguns: Vec<minigun::Minigun>,
    pub accelerators: Vec<accelerator::Accelerator>,
    pub accelerator_bolts: Vec<accelerator::AcceleratorBolt>,
    pub archelix_casters: Vec<archelix::ArchelixCaster>,
    pub archelix_bolts: Vec<archelix::ArchelixBolt>,
    /// PoolGenerator's pools of script-bearing prefabs (pool.rs).
    pub generic_pools: Vec<pool::PrefabPool>,
    /// A replay's pools an earlier scenario of the recording's launch made (ClearEverything empties them; they stay), by
    /// GameObject name, until PoolGenerator.Ensure of their prefab finds them made.
    pub recorded_pools: Vec<(String, misc::ObjectPool)>,
    /// A recorded pool holding clones (all returned; its ObjectPoolBehaviour.Update runs from the replay's start): the
    /// GameObject name, the clones' positions in createdObjects order (Ensure Instantiates them) and the generic pool it
    /// became.
    pub recorded_pool_items: Vec<(String, Vec<([f32; 3], bool)>, Option<usize>)>,
    pub universal_bells: Vec<universal_bell::UniversalBell>,
    pub counters: Vec<counter::Counter>,
    pub pointers: Vec<counter::Pointer>,
    pub toggleable_mirrors: Vec<mirror::ToggleableMirror>,
    pub glowing_metals: Vec<sprite_blocks::GlowingHotMetal>,
    pub resizable_slices: Vec<sprite_blocks::ResizableSlice>,
    pub burnable_sprites: Vec<sprite_blocks::BurnableSprite>,
    /// RendererScene nodes in node order: the SpriteRenderer ids (`renderer_scene`).
    pub renderer_nodes: Vec<i32>,
    /// RendererScene +0xf8 (SetPreventAddRemoveRenderer, set around CullScene's OnWillRenderObject messages) and the
    /// pending lists it fills: removals (+0x68, node ids) and additions (+0x48), applied by `renderer_cull_end`.
    pub renderer_prevent: bool,
    pub renderer_pending_remove: Vec<i32>,
    pub renderer_pending_add: Vec<i32>,
    pub foliages: Vec<foliage::Foliage>,
    pub break_joint_on_burns: Vec<breakjoint::BreakJointOnBurn>,
    pub break_joint_on_shots: Vec<breakjoint::BreakJointOnShot>,
    pub sound_on_shakes: Vec<shake::SoundOnShake>,
    pub emit_on_shakes: Vec<shake::EmitOnShake>,
    pub serialisation_helpers: Vec<shake::SerialisationHelper>,
    /// The scripts' Play, Stop and Emit calls on `embedded_ps` since the particles' last update, in call order.
    pub ps_calls: Vec<(usize, PsCall)>,
    pub gorse_droolings: Vec<gorse::GorseDrooling>,
    /// ParticleSystems inside spawned prefabs that a script plays or switches (particles.rs draws them).
    pub embedded_ps: Vec<EmbeddedPs>,
    /// A spawn whose clone `effect_log` plays (Instantiate of an effect prefab): its systems are the effect's.
    pub effect_spawn: bool,
    pub gorse_eyes: Vec<gorse::GorseEye>,
    /// ToolLibrary.Tools and Powers, read from the scene the first time a tool is picked.
    pub tool_library: Option<(Vec<tool::ToolEntry>, Vec<tool::ToolEntry>)>,
    pub gorse_projectiles: Vec<gorse::GorseProjectile>,
    pub fireballs: Vec<excalibur::Fireball>,
    pub glows: Vec<glow::GlowInTheDark>,
    /// LightSprites (a glow's ModLightPrefab, or a recording's).
    pub light_sprites: Vec<glow::LightSprite>,
    pub liquid_spreads: Vec<liquid_spread::LiquidSpreadAoE>,
    /// Every AudioSource and the AudioListener (audio.rs).
    pub audio: audio::Audio,
    /// The translated classes' components (tools/translator).
    pub tr: translated::Translated,
    pub explosives: Vec<explosion::Explosive>,
    pub act_on_collides: Vec<acts::ActOnCollide>,
    pub no_collides: Vec<acts::NoCollide>,
    pub disintegration_counters: Vec<acts::DisintegrationCounter>,
    pub sticky_grenades: Vec<acts::StickyGrenade>,
    pub point_to_velocities: Vec<acts::PointToVelocity>,
    pub inverters: Vec<acts::Inverter>,
    pub obstruction_emitters: Vec<obstruction_emitter::ObstructionEmitter>,
    pub android_sparks: Vec<acts::AndroidSparkCreator>,
    pub cars: Vec<car::CarBehaviour>,
    pub tyres: Vec<tyre::TyreDeflationBehaviour>,
    pub flasks: Vec<flask::Flask>,
    pub point_liquids: Vec<flask::PointLiquidTransfer>,
    pub liquid_containers: Vec<flask::LiquidContainer>,
    pub glowtubes: Vec<light::Glowtube>,
    pub cathode_lights: Vec<light::CathodeLight>,
    pub leds: Vec<light::LedBulb>,
    pub detectors: Vec<detector::Detector>,
    pub generators: Vec<devices::Generator>,
    pub floodlights: Vec<devices::Floodlight>,
    pub spike_grenades: Vec<devices::SpikeGrenade>,
    /// Components whose Behaviour.enabled was set to false at run time (SetAsBroken) and that keep no flag of their own.
    pub disabled_ids: std::collections::HashSet<i32>,
    /// Prefab-disabled ExplosionSoundBehviours that set_enabled(true) turned on.
    pub explosion_sounds_enabled: std::collections::HashSet<i32>,
    /// ExplosionSoundBehviours whose Start has run (or is queued).
    pub explosion_sounds_started: std::collections::HashSet<i32>,
    /// ExplosionSoundBehviours enabled on an inactive object before their Start: (prefab m_Enabled, Start's Play(Delay) when
    /// StartWhenAwake). Their Start comes with the object's SetActive(true).
    pub explosion_sounds_waiting: std::collections::HashMap<i32, (bool, Option<(Option<usize>, i64, f32)>)>,
    /// Each renderer's MaterialPropertyBlock and material overrides (props.rs).
    pub props: std::collections::HashMap<visual::SpriteAt, props::RenderProps>,
    pub heating_elements: Vec<devices::HeatingElement>,
    pub ray_guns: Vec<devices::RayGun>,
    pub bulbs: Vec<devices::Bulb>,
    pub thrusterbeds: Vec<thruster::Thrusterbed>,
    pub pistons: Vec<piston::Piston>,
    pub map_lifts: Vec<map_lift::MapLift>,
    pub flamethrowers: Vec<flamethrower::Flamethrower>,
    pub rockets: Vec<rocket::LaunchedRocket>,
    pub airfoils: Vec<airfoil::Airfoil>,
    pub stunners: Vec<stunner::Stunner>,
    pub soaps: Vec<soap::Soap>,
    pub power_hammers: Vec<power_hammer::PowerHammer>,
    pub balloons: Vec<balloon::Balloon>,
    pub metronomes: Vec<metronome::Metronome>,
    /// 1.28.45's new classes (v45_new.rs).
    pub v45: v45_new::V45,
    /// The PseudoBloodImpactHelpers (component id, Color) that LimbBehaviour.PrepareGoreBits added to its shards.
    pub pseudo_blood_helpers: Vec<(i32, [f32; 4])>,
    pub determined_anomalies: Vec<determined_anomaly::DeterminedAnomaly>,
    pub random_sprite_children: Vec<misc::RandomSpriteChild>,
    pub physics_guns: Vec<physics_gun::PhysicsGun>,
    pub smooth_lines: Vec<smooth_line::SmoothLine>,
    pub activator_electrodes: Vec<activator_electrode::ActivatorElectrode>,
    /// The visual scripts (visual.rs) and their index by component instance id.
    pub visuals: Vec<visual::Visual>,
    pub visual_ids: std::collections::HashMap<i32, usize>,
    pub thermometers: Vec<thermometer::Thermometer>,
    pub infrared_thermometers: Vec<thermometer::InfraredThermometer>,
    pub activation_toggles: Vec<activation_toggle::ActivationToggle>,
    pub power_gauges: Vec<gauge::PowerGauge>,
    pub torches: Vec<torch::Torch>,
    pub flares: Vec<flare::Flare>,
    pub buttons: Vec<button::Button>,
    pub lasers: Vec<laser::Laser>,
    pub firearm_attachments: Vec<attachment::FirearmAttachment>,
    /// FirearmAttachmentPointBehaviour components.
    pub attachment_points: Vec<attachment::AttachmentPoint>,
    pub televisions: Vec<television::Television>,
    pub wrenches: Vec<wrench::Wrench>,
    pub valves: Vec<valve::Valve>,
    pub dangles: Vec<dangle::Dangle>,
    pub liquid_canisters: Vec<liquid_canister::LiquidCanister>,
    pub liquid_filters: Vec<liquid_filter::LiquidFilter>,
    pub pulse_drums: Vec<pulse_drum::PulseDrum>,
    pub act_on_disintegrates: Vec<acts::ActOnDisintegrate>,
    pub only_exist_withs: Vec<acts::OnlyExistWith>,
    pub offset_grids: Vec<acts::OffsetGrid>,
    pub wind_controllers: Vec<particle_forces::WindController>,
    /// Menus-and-UI drawing state (ui_visual.rs).
    pub ui_visual: ui_visual::UiVisual,
    /// ParticleMachineBehaviour.Activated (the particles it plays are presentation).
    pub particle_machines: Vec<bool>,
    /// ParticleMachineBehaviour's EmitSmoke, EmitFire and EmitElectricity, from its Start (its buttons read them).
    pub particle_machine_emit: Vec<[bool; 3]>,
    /// EditableTintBehaviour.Tint, by the Menu::EditableTint index.
    pub editable_tints: Vec<[f32; 4]>,
    /// EditableTintBehaviour.phys (Awake's GetComponent<PhysicalBehaviour>()), by the same index.
    pub editable_tint_phys: Vec<usize>,
    /// The open colour picker.
    pub colour: Option<menu::ColourBox>,
    /// The open key entry boxes, the last on top.
    pub key_boxes: Vec<menu::KeyBox>,
    /// LimbStatusViewBehaviour.Main.
    pub limb_view: menu::LimbView,
    /// PianoDialogBox while shown.
    pub piano: Option<menu::Piano>,
    pub eggs: Vec<acts::Egg>,
    /// EggBehaviour.canEnter (static).
    pub egg_can_enter: bool,
    /// EggBehaviour.enteredCode (static).
    pub egg_entered: String,
    pub liquidentifiers: Vec<liquidentifier::Liquidentifier>,
    pub centrifuges: Vec<centrifuge::Centrifuge>,
    pub vibrations: Vec<centrifuge::Vibration>,
    pub liquid_pumps: Vec<liquid_pump::LiquidPump>,
    pub liquid_duplicators: Vec<liquid_duplicator::LiquidDuplicator>,
    pub liquid_leaks: Vec<liquid_leak::LiquidLeak>,
    pub liquid_holes: Vec<liquid_leak::LiquidHole>,
    pub no_collide_body_parts: Vec<no_collide::NoCollideWithBodyPart>,
    pub timed_gates: Vec<timed_gate::TimedGate>,
    pub lagboxes: Vec<timed_gate::Lagbox>,
    pub gates: Vec<timed_gate::Gate>,
    pub activation_transformers: Vec<timed_gate::ActivationTransformer>,
    pub hover_thrusters: Vec<hover_thruster::HoverThruster>,
    pub hovercars: Vec<hover_thruster::Hovercar>,
    pub winches: Vec<winch::Winch>,
    pub conveyors: Vec<conveyor::Conveyor>,
    pub reactor_cores: Vec<reactor::ReactorCore>,
    pub reactor_conduits: Vec<reactor::ReactorConduit>,
    pub reactor_conduit_sets: Vec<reactor::ReactorConduitSet>,
    pub reactor_a5_displays: Vec<reactor::ReactorA5Display>,
    pub reactor_meltdowns: Vec<reactor::ReactorMeltdown>,
    pub do_at_times: Vec<reactor::DoAtTime>,
    pub industrial_lifts: Vec<reactor::IndustrialLift>,
    pub acid_pools: Vec<reactor::AcidPool>,
    pub vacuums: Vec<reactor::Vacuum>,
    pub point_ats: Vec<reactor::PointAt>,
    pub eps: Vec<egg::Ep>,
    pub reactor_a5_arcs: Vec<arc::ReactorA5Arc>,
    pub rope_creators: Vec<rope::PhysicalRopeCreator>,
    pub line_renderer_targets: Vec<rope::LineRendererTargets>,
    pub random_sounds: Vec<reactor::RandomSound>,
    pub loop_sounds: Vec<audio::LoopSound>,
    pub joint_motor_sounds: Vec<audio::JointMotorSound>,
    pub play_random_clips: Vec<reactor::PlayRandomClip>,
    pub jukeboxes: Vec<jukebox::Jukebox>,
    pub antenna_receivers: Vec<antenna::AntennaReceiver>,
    pub couplers: Vec<coupler::Coupler>,
    pub lightning_entities: Vec<lightning_tool::LightningEntity>,
    /// The LightningTool once added to the ToolController.
    pub lightning_tool: Option<lightning_tool::LightningTool>,
    pub pyro_entities: Vec<pyro_tool::PyroEntity>,
    /// The PyroPowerTool once added to the ToolController.
    pub pyro_tool: Option<pyro_tool::PyroTool>,
    pub seismic_explosions: Vec<seismic_explosion::SeismicExplosion>,
    pub energy_bolts: Vec<seismic_explosion::EnergyBolt>,
    pub transmitters: Vec<transmitter::Transmitter>,
    /// TransmitterBehaviour.uniqueSounds (static): the channels with a clip.
    pub transmitter_sounds: std::collections::HashMap<String, audio::ClipRef>,
    pub defibrillators: Vec<resuscitation::Defibrillator>,
    pub cardiopulmonary_bypasses: Vec<resuscitation::CardiopulmonaryBypass>,
    pub heart_monitors: Vec<heart_monitor::HeartMonitor>,
    /// LimbSpriteCache.Instance
    pub limb_sprite_cache: limb_sprites::LimbSpriteCache,
    /// The scene singletons' static Instance/Main properties (BulletTracerPool.Instance, ClipboardControllerBehaviour.Main, ...):
    /// class name to the value as the recorder writes a reference (`@name#id`, `null`). Each Awake's `Instance = this` is the
    /// scene's own object; fn-check loads them from the case's em=.
    pub statics: std::collections::BTreeMap<String, String>,
    /// The BloodWireBehaviours the BloodWireTool added, by creation order, and that tool's state.
    pub blood_wires: Vec<blood_wire::BloodWire>,
    /// The FixedCableBehaviours loaded onto a root without a PhysicalBehaviour.
    pub root_wires: Vec<blood_wire::RootWire>,
    pub pins: Vec<pin::Pin>,
    /// ToolWheelBehaviour.OnToolVisibilityChanged's ToolInvisibilityTracker listeners, in subscription order.
    pub trackers: Vec<tool_invisibility::Owner>,
    /// ToolWheelBehaviour.EyeAnimation
    pub eye: tool_wheel::EyeAnimation,
    /// CurrentTool is PinTool (false) or WoodPinTool (true).
    pub pin_tool: Option<bool>,
    /// Whether each was added to the ToolController.
    pub pin_tool_created: [bool; 2],
    /// FireLoopSoundControllerBehaviour.FireChunks: the cells with a chunk.
    pub fire_loop_chunks: std::collections::BTreeMap<(i32, i32), physical::FireLoopChunk>,
    pub wire_tool: blood_wire::WireTool,
    pub phase_links: Vec<link_device::PhaseLink>,
    /// The PhaseLinkTool on the ToolController.
    pub link_tool: link_device::LinkTool,
    pub belts: Vec<serpentine_belt::Belt>,
    /// The SerpentineBeltTool on the ToolController.
    pub belt_tool: serpentine_belt::BeltTool,
    /// The AOEPowerTools added to the ToolController, and the one that is CurrentTool.
    pub powers: Vec<power::Power>,
    pub power: Option<usize>,
    /// Global.ShowLimbStatus
    pub ShowLimbStatus: bool,
    /// The LimbStatus prefab's sprites (healthbar frame, dead, bar fill), read when the first limb links.
    pub limb_status_look: Option<limb::LimbStatusLook>,
    /// ExistInDetailView components that have started: LimbStatusToggled sets their GameObjects active.
    pub detail_views: Vec<i32>,
    /// ThermalVisionBehaviour.Instance's ThermalVisionEnabled (None: no instance).
    pub thermal_vision: Option<bool>,
    /// That instance's heat texture and its Transform.
    pub thermal_texture: Option<explosion::ThermalTexture>,
    /// ExistInThermalVision components that have started: OnThermalVisionToggle sets their GameObjects active.
    pub thermal_views: Vec<i32>,
    /// gapA r9: a level1 (Main.unity) component's instance id -> the id the rewrite keeps that scene component's singleton
    /// under (ExistInThermalVision: thermal_display::CONTAINER), for the ones it does not keep by their own id.
    pub scene_alias: std::collections::HashMap<i32, i32>,
    /// gapA r8: subscriptions a later Awake/OnDestroy added or took, against the model's own (continuous::Ev).
    pub listen_delta: HashMap<(i32, continuous::Ev), i32>,
    /// The Main scene's thermal vision temperature display.
    pub thermal_display: Option<thermal_display::ThermalDisplay>,
    pub invisibles: Vec<misc::Invisible>,
    pub excaliburs: Vec<excalibur::Excalibur>,
    pub sharp_on_all_sides: Vec<sharp_on_all_sides::SharpOnAllSides>,
    pub blood_tanks: Vec<blood_tank::BloodTank>,
    pub liquid_outlets: Vec<liquid_outlet::LiquidOutlet>,
    pub displays: Vec<display::Display>,
    pub syringes: Vec<syringe::Syringe>,
    pub jet_engines: Vec<jet_engine::JetEngine>,
    pub chainsaws: Vec<chainsaw::Chainsaw>,
    pub decimators: Vec<decimator::Decimator>,
    pub drones: Vec<drone::Drone>,
    pub activation_nodes: Vec<activations::ActivationNode>,
    pub portals: Vec<portal::Portal>,
    pub trigger_events: Vec<portal::TriggerEvent>,
    pub portal_traversings: Vec<portal::PortalTraversing>,
    pub propellers: Vec<drone::Propeller>,
    pub black_holes: Vec<black_hole::BlackHole>,
    pub immobility_fields: Vec<immobility_field::ImmobilityField>,
    pub forcefields: Vec<forcefield::Forcefield>,
    pub lightning_guns: Vec<lightning_gun::LightningGun>,
    pub javelins: Vec<javelin::Javelin>,
    pub javelin_missiles: Vec<javelin::JavelinMissile>,
    pub tesla_coils: Vec<tesla_coil::TeslaCoil>,
    pub tesla_bolts: Vec<tesla_coil::TeslaBolt>,
    /// DecimatorBehaviour.lastDestroyedRootId (static, -1 until set).
    pub decimator_last_root: Option<i32>,
    pub emps: Vec<emp::Emp>,
    pub freezes: Vec<freeze::Freeze>,
    pub freeze_serialisations: Vec<freeze::FreezeSerialisation>,
    pub ceiling_turrets: Vec<ceiling_turret::CeilingTurret>,
    pub temperature_transfers: Vec<temperature_transfer::TemperatureTransfer>,
    pub worm_staffs: Vec<worm_staff::WormStaff>,
    pub atom_explosions: Vec<atom_explosion::AtomExplosion>,
    pub prefab_bombs: Vec<prefab_bomb::PrefabBomb>,
    pub laser_turrets: Vec<laser_turret::LaserTurret>,
    pub sentry_turrets: Vec<sentry_turret::SentryTurret>,
    pub sentry_guns: Vec<sentry_turret::SentryTurretGun>,
    pub ion_bolts: Vec<ion_bolt::IonBolt>,
    pub random_events: Vec<random_event::RandomEvent>,
    pub energy_swords: Vec<energy_sword::EnergySword>,
    pub energy_blurs: Vec<energy_sword::EnergySwordBlur>,
    /// EnergySwordBladeClashBehaviour: its id, its GameObject's colliders and CollideWithOtherSwordVFX.
    pub blade_clashes: Vec<(i32, Vec<i32>, crate::assets::db::ObjRef)>,
    /// A MapConfig was woken (FindObjectOfType<MapConfig>() is not null).
    pub map_config_present: bool,
    /// MapLoaderBehaviour.CurrentMap's Prefab and the Transform id of the map clone under WORLD (set by the main
    /// menu's load; a recording has neither).
    pub current_map: Option<crate::assets::db::Prefab>,
    /// MapLoaderBehaviour.CurrentMap itself: the Map asset (TelevisionBehaviour compares it with B2Map).
    pub current_map_asset: Option<crate::assets::db::ObjRef>,
    pub map_root: Option<i32>,
    /// MapLightBehaviour.StartEnabled (Global.ClearMap sets it from the old MapConfig's Settings).
    pub map_light_start_enabled: bool,
    /// MapConfig.Instance.Settings (map_config_apply_settings edits it).
    pub map_settings: Option<map::EnvironmentalSettings>,
    /// MapConfig's heatEffect.weight and coldEffect.weight (the post volumes, lights.rs).
    pub post_weights: [f32; 2],
    /// PostProcessManager's volumes registered after the Main scene's three (the maps', the fog's), in OnEnable order.
    pub post_volumes: Vec<map::PostVolume>,
    /// The map editor screen's MapEditor* behaviours (None outside the editor).
    pub map_editor: Option<map_editor::MapEditor>,
    /// LayerEditorController (its Container, Targets and displays).
    pub layer_editor: layer_editor::LayerEditor,
    /// The LayerSerialisationBehaviours the context menu's No Collide added, by physical object.
    pub layer_serialisations: std::collections::HashMap<usize, context_menu::LayerSerialisation>,
    /// A loaded LayerSerialisationBehaviour on an object with no body (an old save's No Collide root): component id -> Layer.
    pub layer_serialisations_bodyless: std::collections::HashMap<i32, i32>,
    /// The `lightnings` entry of MapConfig's WeatherLightningBehaviour clone.
    pub map_lightning: Option<usize>,
    /// A recording's map weather clones' GameObject ids by clone name: the linked map's MapConfig.Start makes them with these.
    pub recorded_weather: HashMap<String, i32>,
    /// MapConfig.overlayTarget's values in insertion order (each Reinitialise adds its weather instances; None only while taken).
    pub map_overlays: Vec<Option<map_overlay::Overlay>>,
    /// MapConfig.SnowInstance, RainInstance, FogInstance: their `map_overlays` entries.
    pub map_weather: [Option<usize>; 3],
    /// The weather overlay materials' _Density (Material.SetFloat on a shared material asset: it outlives the map).
    pub overlay_densities: HashMap<crate::assets::db::ObjRef, f32>,
    /// A WaterImpactBehaviour's component id -> its GameObject's bare collider (the key of `bare_colliders`).
    pub water_impact_colliders: HashMap<i32, i32>,
    /// The live MapConfig's Start data (its prefabs, bounds and ambience), for Reinitialise and its Prepare steps.
    pub map_config: Option<map::MapConfigStart>,
    /// Main's DiskLogger (play.rs new_game; None headless).
    pub disk_logger: Option<disk_logger::DiskLogger>,
    pub blasters: Vec<blaster::Blaster>,
    pub beam_cannons: Vec<blaster::BeamCannon>,
    pub beam_repeaters: Vec<blaster::BeamRepeater>,
    pub android_lasers: Vec<blaster::AndroidLaser>,
    pub ray_cannons: Vec<blaster::RayCannon>,
    pub railguns: Vec<blaster::Railgun>,
    pub lightnings: Vec<map::WeatherLightning>,
    pub laser_receivers: Vec<laser::LaserReceiver>,
    pub key_triggers: Vec<laser::KeyTrigger>,
    pub liquid_level_gates: Vec<laser::LiquidLevelGate>,
    pub fireworks: Vec<fireworks::Fireworks>,
    pub blasterbolts: Vec<blaster::Blasterbolt>,
    pub beamformers: Vec<scifi::Beamformer>,
    pub scifi40s: Vec<scifi::Scifi40>,
    pub effect_guns: Vec<effect_gun::EffectGun>,
    pub act_on_shots: Vec<acts::ActOnShot>,
    pub shot_propagators: Vec<ballistics::ShotMessagePropagator>,
    pub signal_converters: Vec<signal_converter::SignalConverter>,
    pub syringe_explosions: Vec<syringe::SyringeExplosion>,
    /// `anchored_joints` slot -> `fixed_joints` entry, for the prefab joints adopted (a script that reads the component,
    /// or HingeJointLimitAutofixBehaviour.Start of a mirrored host): the first adoption owns the component's state.
    pub adopted_slots: HashMap<usize, usize>,
    /// The fixed_joints entry of each adopted prefab Joint2D, by component id.
    pub adopted_joints: HashMap<i32, usize>,
    /// gapB r21: the link-time context (prefab, instance, links; object and component index) of each script whose Awake
    /// reads it (IndustrialLift/ReactorConduit/TeslaCoil; CopperWire has no Awake of its own: WireBehaviour.Awake), by component id: fn-check calls that Awake again.
    #[allow(clippy::type_complexity)]
    pub awake_ctx: HashMap<i32, (std::sync::Arc<(crate::assets::db::Prefab, spawn::Instance, link::Linked)>, usize, usize)>,
    /// A translated script's presentation link (LinkKind::Present) to a component of its prefab: (the script's
    /// instance id, the field) -> that component's instance id (an AudioSource, a SpriteRenderer).
    pub present_ids: HashMap<(i32, &'static str), i32>,
    pub continuous: continuous::ContinuousActivation,
    /// The ActivationPropagation objects and their path set pool.
    pub signals: signal::Signals,
    /// ContextMenuBehaviour: open, with its dynamic buttons.
    pub context_menu: Option<Vec<menu::ShownButton>>,
    /// The open number entry box (menu.rs).
    pub entry: Option<menu::EntryBox>,
    /// Inside Game::context_menu_click, and whether an action threw (Game::menu_throw).
    pub menu_clicking: bool,
    pub menu_threw: bool,
    /// The seven ConditionalTextBehaviours' timers and last read property (context_menu.rs).
    pub context_text_t: [f32; 7],
    pub context_text: [Option<bool>; 7],
    /// The Follow Status item: its object's active flag (the scene starts it active) and its Text.
    pub following_view_active: bool,
    pub following_view_text: String,
    /// Each TruckDoorBehaviour with its component's instance id.
    pub truck_doors: Vec<(i32, car::TruckDoor)>,
    pub joint_helpers: Vec<car::JointHelper>,
    pub no_collide_gms: Vec<car::NoCollideGm>,
    pub random_car_textures: Vec<car::RandomCarTexture>,
    pub delete_afters: Vec<explosion::DeleteAfter>,
    pub poisons: Vec<poison::Poison>,
    pub add_forces: Vec<misc::AddForce>,
    /// Coroutines in WaitForEndOfFrame, in the order they got there.
    pub end_of_frame: Vec<destroyable::Coroutine>,
    /// GlobalShrapnelEmitter.Instance
    pub shrapnel: Option<ballistics::ShrapnelEmitter>,
    /// Splats that destroyed themselves this fixed step: their Object.Destroy runs after the WaitForFixedUpdate coroutines.
    pub splats_destroying: Vec<usize>,
    /// Coroutines in WaitForFixedUpdate, in the order they got there.
    pub fixed_update_waits: Vec<destroyable::Coroutine>,
    /// A replay's recorder coroutine, itself in WaitForFixedUpdate: the coroutine_seq when it last got there. The
    /// waits queued after it resume after its FE snapshot (`after_recorder_waits`); None outside a replay.
    pub recorder_wait_seq: Option<u64>,
    pub after_recorder_waits: Vec<destroyable::Coroutine>,
    /// BallisticsEmitter.BallisticIteration coroutines in a wait (`Step::Bullet`); None once resumed or stopped.
    pub bullets: Vec<Option<(ballistics::Bullet, Option<ballistics::BulletHit>)>>,
    pub tracer_pool: ballistics::TracerPool,
    pub camera: ballistics::CameraState,
    /// Global.main.CameraControlBehaviour
    pub camera_control: camera_control::CameraControl,
    /// Hover.CurrentlyHovering (a component instance id).
    pub hovering: Option<i32>,
    /// fnweap: level1's SimpleLightningEmitterBehaviour.PoolableLightningPrefab (read on the first Emit).
    pub simple_lightning_prefab: Option<crate::assets::db::ObjRef>,
    /// fn-check (biga:emit): the hit the probe's onHit hook kept.
    pub fn_lightning_hit: Option<(usize, [f32; 2])>,
    /// fn-check (gapA r5): an OnWillRenderObject case's target; the will-render loops run it alone (will_render_dispatch).
    pub fn_render_only: Option<i32>,
    /// ContextMenuBehaviour.priorSelectedToolBehaviour
    pub context_prior: Option<i32>,
    /// Global.BulletCrackClips length.
    pub bullet_crack_clips: usize,
    pub coroutine_seq: u64,
    /// The translated routines (`coroutine::Routine`) the queues above resume.
    pub co: coroutine::Coroutines,
    /// The components of destroyed GameObjects (they read null).
    pub destroyed_components: std::collections::HashSet<i32>,
    /// The grips and FixedJointWireBehaviours whose OnChangePause listener ModdingApi.Events.ClearListeners removed.
    pub pause_listeners_cleared: std::collections::HashSet<i32>,
    // ModAPI.get_Draw, ModAPI.set_Draw: ModdingApi.Draw (Global.Awake: a new ModDebugDrawer over an empty stack).
    /// ModdingApi.Draw
    pub mod_debug_draw: mods::ModDebugDrawer,
    /// Scripts Object.Destroy has disabled, whose delayed deactivation skips the OnDisable.
    pub destroy_disabled: std::collections::HashSet<i32>,
    /// The fn-check load's object references (`@name#id` in a script link): recorded instance id -> the rewrite's value it loaded as.
    pub ref_pairs: std::collections::HashMap<i32, String>,
    /// gapB r15: the self's reference caches fncheck reset before an Awake/Start (class, id, field), compared after it.
    pub ref_reset: Vec<(String, i32, &'static str)>,
    /// Object.Destroy calls whose clone was checked out, as (id, a lone component), run when it returns.
    pub destroy_disable_pending: Vec<(i32, bool)>,
    /// text_set_rotation_identity's local rotations for a clone being awoken, (text component id, q), written by awake_behaviours.
    pub text_rotation_pending: Vec<(i32, [f32; 4])>,
    /// Clones checked out by with_component_instance / with_water_instance (their calls drain the pending lists).
    pub clones_checked_out: u32,
    /// OnDisable calls of translated scripts that came while their clone was checked out (an OnDisable that SetActives an object of its own clone), run when it returns.
    pub tr_on_disable_pending: Vec<translated::TBehaviour>,
    pub act_on_destroy: HashMap<i32, destroyable::ActOnDestroy>,
    /// The next instance id when a SerialisableIdentity was added at run time: (the object's Transform id, false) for one
    /// ObjectState.Reconstruct added, (the root's, true) for SerialiseInstructions.Start's. A component added at run time
    /// with an id at or below it came after the identity (GetComponents order, which the pack follows).
    pub identity_added_at: HashMap<(i32, bool), i32>,
    /// UseEventTrigger components by instance id.
    pub use_events: HashMap<i32, link::UseEvent>,
    /// UseBubbler.OnlyDirect by component instance id, where it was set over the prefab's (a recorded ItemScript value).
    pub use_bubblers: HashMap<i32, bool>,
    /// What `spawn` linked of each clone with an object that was inactive when it spawned (by the id of the root
    /// Transform): the first GameObject.SetActive(true) of such an object runs its scripts' Awake with it.
    pub first_activation: HashMap<i32, link::Linked>,
    /// fn-check: what link_recording_item linked of each recorded root, by its root GameObject id (a VaseBehaviour case's
    /// debris clone is handed to OnDebrisCreated as Break hands a fresh spawn's).
    pub recorded_linked: HashMap<i32, link::Linked>,
    /// The GameObjects (by the id of their Transform) that GetOrAddComponent<DebrisComponent> added one to.
    pub debris_added: std::collections::HashSet<i32>,
    /// FuseBoxBehaviour.audioProvider (Awake's AudioClip.Create clip), its instance id by the FuseBoxBehaviour's.
    pub fuse_box_clips: HashMap<i32, i32>,
    /// Clones Instantiate(.., parent) put under an existing GameObject, by the parent's Transform id. Destroying the
    /// parent destroys them; `update_attached_clones` keeps their poses under the parent's.
    pub attached_clones: HashMap<i32, Vec<destroyable::AttachedClone>>,
    /// The clones scripts Instantiate under a spawn's Transforms in Awake or Start (save.rs RuntimeChild), by the parent's Transform id.
    pub runtime_children: HashMap<i32, Vec<save::RuntimeChild>>,
    /// The Transforms of runtime clones Instantiate(prefab, parent) put under a Transform the rewrite keeps no clone object for
    /// (MapConfig's weather and lightning clones under the map root, PhysicalBehaviour's MetalScrape and BubbleVFX clones under
    /// its own): by the parent's Transform id, in sibling order, each clone's Transforms depth first with the index after each
    /// one's subtree (tool.rs add_runtime_clone).
    pub runtime_clone_transforms: HashMap<i32, Vec<Vec<(i32, usize)>>>,
    pub dwacds: Vec<destroyable::DestroyWhenAllChildrenDestroyed>,
    /// level1's DistantSoundBehaviour clip counts, by SoundType - 1 (SmallExplosion .. LargeFirearm).
    pub distant_sounds: [usize; 6],
    /// DistantSoundBehaviour's clips (same order as `distant_sounds`).
    pub distant_sound_clips: [Vec<Option<audio::ClipRef>>; 6],
    /// Each BellClip's Clip (same order as `bell_clips`).
    pub bell_clip_refs: Vec<Option<audio::ClipRef>>,
    /// UISoundBehaviour.Main (None in a recording, which has no scene).
    pub ui_sound: Option<audio::UiSound>,
    /// WaterSoundBehaviours and RectangleAudioSourceBehaviours (`audio::AudioScript`).
    pub audio_scripts: Vec<audio::AudioScript>,
    /// Each `ambient_audio`'s Clips (MapConfig.Reinitialise may replace them with the map's Ambience.Clips).
    pub ambient_audio_clips: Vec<Vec<Option<audio::ClipRef>>>,
    /// Each piano dialog key's UiPianoKeyBehaviour.Source and BellClip.Clip (`menu::piano_keys` order).
    pub piano_key_audio: Vec<(Option<i32>, Option<audio::ClipRef>)>,
    /// Global.CameraAudioSource and DistantSoundBehaviour.AudioSource (None in a recording, which has no scene).
    pub camera_audio_source: Option<i32>,
    /// The persistent calls of each UnityEvent field, by (component id, field name) (`invoke_event`).
    pub unity_events: std::collections::HashMap<(i32, String), Vec<link::EventCall>>,
    pub distant_audio_source: Option<i32>,
    /// Global.main.BellClips.BellClips (level1's BellClipContainer): each clip's Name and Key (PianoKey).
    pub bell_clips: Vec<(String, i32)>,
    /// ProceduralShatterMaterialPool / ShatteredColliderShapeCache statics.
    pub shatter: shatter::ShatterState,
    /// The ShatteredObjectGenerators of the loaded Persons' limbs.
    pub shatter_generators: Vec<shatter::GeneratorSource>,
    /// Coroutines waiting in WaitForSeconds, resumed by the dynamic delayed-call pass.
    pub coroutines: Vec<destroyable::Coroutine>,
    pub do_at_time_onces: Vec<destroyable::DoAtTimeOnce>,
    pub underwater_explosions: Vec<explosion::UnderwaterExplosion>,
    /// InstantiateObjectBehaviour by component id: Prefab and fields.
    pub instantiate_objects: HashMap<i32, (Option<crate::assets::db::ObjRef>, destroyable::InstantiateObject)>,
    /// Object.Destroy calls queued for the delayed-call pass: (time, a component id on the object).
    pub object_destroys: Vec<(f64, i32)>,
    /// Object.Destroy(component) calls queued for the delayed-call pass: (time, the component's id).
    pub component_destroys: Vec<(f64, i32)>,
    /// Object.Destroy(FixedJoint2D) calls queued for the delayed-call pass: (time, `fixed_joints` index).
    pub joint_destroys: Vec<(f64, usize)>,
    /// The queued Object.Destroy calls of a FirearmAttachmentPointBehaviour's HingeJoint2D: (time, b2 joint).
    pub attachment_joint_destroys: Vec<(f64, crate::b2::world::JointId)>,
    /// The manager entry of each registered script component (by instance id), for SetActive.
    pub behaviour_of: HashMap<i32, (i32, Behaviour)>,
    /// Every BloodPuff the pool created and its ObjectPoolBehaviour (None until PoolGenerator.Ensure).
    pub blood_puffs: Vec<misc::BloodPuff>,
    pub blood_puff_pool: Option<misc::ObjectPool>,
    pub hinges: Vec<Hinge>,
    /// Every PointEffector2D component, and the indices of those on the scene's UpdateEffectors list (AddToManager order).
    pub point_effectors: Vec<effector::PointEffector>,
    pub effector_updates: Vec<usize>,
    /// The translated components that take OnTrigger*2D / OnJointBreak2D (messages.rs).
    pub message_scripts: Vec<messages::MessageScript>,
    /// The map's scripts that draw UnityEngine.Random (scene order).
    pub ambient_audio: Vec<PlayAudioAtRandom>,
    pub map_lights: Vec<MapLight>,
    /// WaterBehaviour.waters (in Awake order, which is the arena order) with their state.
    pub waters: Vec<water::Water>,
    /// WaterBehaviour.waters: indices into `waters`, in Awake order.
    pub water_list: Vec<usize>,
    /// FreezeStackController.stack: the physical objects' Rigidbody2Ds with their request counts.
    pub freeze_stack: std::collections::HashMap<usize, i32>,
    pub frozen_in_ice: Vec<water::FrozenInIce>,
    /// The pools of the Splash prefabs the waters name.
    pub splash_pools: Vec<water::SplashPool>,
    /// Global.main.DynamicSqrdVelocityThreshold (2500 at construction, the Main scene's value)
    pub DynamicSqrdVelocityThreshold: InitF32<0x451c4000>,
    /// Global.main.CameraControlBehaviour.BoundingBox
    pub BoundingBox: Bounds,
    /// Global.physicalBehaviourChunks (each chunk holds at most 128) and chunksUpToDate.
    pub physical_chunks: std::collections::HashMap<(i32, i32), Vec<usize>>,
    pub chunksUpToDate: bool,
    /// Global.paused: the toolbar's Paused, as the game last heard it (Game::change_pause).
    pub paused: bool,
    /// Global.GetPausedMenu: the pause menu is open (the UI sets it each frame).
    pub paused_menu: bool,
    /// InputSystem.Actions' Key and SecondaryKey by `ui::tables::ACTIONS` index; None: the defaults.
    pub control_binds: Option<Vec<(String, String)>>,
    /// TextMeshProControlReplacer: its Text and (ToReplace, ControlCodeName) pairs.
    pub control_replacers: Vec<(Option<usize>, Vec<(String, String)>)>,
    /// PhysicalBehaviour.contactBuffer / LimbBehaviour.contactBuffer: static ContactPoint2D[8], stale between calls.
    pub physical_contact_buffer: [crate::b2::contacts2d::ContactPoint2D; PHYSICAL_CONTACT_BUFFER],
    pub limb_contact_buffer: [crate::b2::contacts2d::ContactPoint2D; LIMB_CONTACT_BUFFER],
    /// Physics2D.velocityIterations / positionIterations (UserPreferences.PhysicsIterations).
    pub velocityIterations: i32,
    pub positionIterations: i32,
    /// PhysicsScene2D.m_JointBreakLimits: hinges with a finite break force or torque, in list order.
    pub joint_break_limits: Vec<BreakJoint>,
    /// The Fixed/Slider/SpringJoint2D components cloned from prefabs, by creation order (BreakJoint::Anchored).
    pub anchored_joints: Vec<AnchoredJoint>,
    /// FixedJoint2D components (the shattered pieces' welds), by creation order.
    pub fixed_joints: Vec<simulate::FixedJoint>,
    /// PhysicalBehaviour.Penetration objects, by creation order (Physical.penetrations/victimPenetrations index it).
    pub penetration_arena: Vec<stab::Penetration>,
    // IgnoreCollisionStackController.Clear: Global.Awake empties the stack, as a fresh Game starts with it empty.
    /// IgnoreCollisionStackController.stack (IgnoreCollisionStackController.Clear is a fresh Game's empty map): the request count of each collider pair (lower instance id first).
    pub ignore_stack: std::collections::HashMap<(i32, i32), i32>,
    /// LimbBehaviour.VitalParts by limb: Bounds as center then extents. The dump loader fills it.
    pub vital_parts: std::collections::HashMap<usize, Vec<[f32; 6]>>,
    /// The Transforms that parent physical objects: Rigidbody-less ones and static bodies' (fixed TRS), and
    /// non-static bodies' (`node_bodies`).
    pub transform_nodes: Vec<crate::unity::transform::Node>,
    /// The `transform_nodes` that are a physical's own Transform, by node: their TRS is the physical's pose.
    pub node_bodies: std::collections::HashMap<usize, usize>,
    /// Colliders on GameObjects without a PhysicalBehaviour, by Collider2D id: their Transforms for the queries.
    pub bare_colliders: std::collections::HashMap<i32, BareCollider>,
    /// Collider2D ids whose GameObject or an ancestor without a PhysicalBehaviour carries a DecalControllerBehaviour.
    pub decal_chain_colliders: std::collections::HashSet<i32>,
    /// Rigidbody2Ds on GameObjects without a PhysicalBehaviour.
    pub node_rigidbodies: Vec<NodeBody>,
    /// The LinkedBody components Linkage.CreateLinkages added, by Rigidbody2D.
    pub linked_bodies: Vec<(BodyOwner, mechanism::LinkedBody)>,
    /// SpriteRenderers on GameObjects without a PhysicalBehaviour (drawing only).
    pub renderers: Vec<Renderer>,
    /// LineRenderers a script drives (line.rs).
    pub lines: Vec<line::LineVisual>,
    /// TrailRenderers drawn as world-space lines of `lines` (trail.rs; rendering only).
    pub trails: Vec<trail::Trail>,
    /// World-space TextMeshPro texts (world_text.rs) and the font assets they use, read once.
    pub world_texts: Vec<world_text::WorldText>,
    pub tmp_fonts: HashMap<crate::assets::db::ObjRef, std::sync::Arc<crate::tmp::FontAsset>>,
    /// The DecalDescriptor assets in use (DecalController.descriptor indexes it).
    pub decal_descriptors: Vec<misc::DecalDescriptor>,
    /// Each descriptor's Sprites and DripParticle, when it came from the game's data.
    pub decal_sprites: std::collections::HashMap<usize, (Vec<crate::assets::db::ObjRef>, Option<crate::assets::db::ObjRef>)>,
    /// The DecalDripStreakBehaviours that still Update: physical, controller, decal, streak.
    pub active_streaks: Vec<[usize; 4]>,
    /// Instances whose Transform changes (an Awake's or the catalog's scale) wait for Physics2D's next
    /// SyncTransforms (`sync_pending`); after it nothing moves their Transforms but the bodies.
    pub unsynced: Vec<spawn::Instance>,
    /// A recording's physicals whose Transform.position (or, with the flag, Transform.rotation) was set since the last sync.
    pub recorded_moves: Vec<(usize, bool)>,
    /// Prefabs instantiated since the particles' last BeginUpdateAll (none while `effects_off`).
    pub effect_log: Vec<EffectSpawn>,
    pub effects_off: bool,
    /// The ParticleSystems (`particles::begin_update_all` / `end_update_all`).
    pub particle_world: Option<Box<crate::particles::World>>,
    /// WindManager's WindZones and ParticleSystemForceFieldManager's fields by component id (particle_forces).
    pub wind_zones: Vec<i32>,
    pub force_fields: Vec<i32>,
    /// WindZone.windMain as WindControllerBehaviour.Update set it, by component id.
    pub wind_main: HashMap<i32, f32>,
    /// Pooled clones returned by a script (SetActive(false)) since the particles last looked: their systems clear.
    pub pooled_deactivated: Vec<Pooled>,
    /// SetActive(false) of a pooled clone's child, by name, for the particle systems.
    pub pooled_child_off: Vec<(Pooled, &'static str)>,
    /// Synced instances that hold a WaterBehaviour (its SetActive calls change their objects).
    pub retained: Vec<spawn::Instance>,
    /// Kept clones whose Transform a Start moved after their first sync (a component id of each): the next SyncTransforms takes them.
    pub retained_sync: Vec<i32>,
    /// A translated Awake/OnEnable of an instance being instantiated runs (its clone is the last in `unsynced`).
    pub awaking: bool,
    /// The GameObjects such an Awake/OnEnable SetActive(false) (HoverboxBehaviour.OnEnable's Effects): the Awake pass deactivates them after it.
    pub awake_deactivate: Vec<i32>,
    /// While a recording's clone runs its pending Starts: the component ids its Clone records list (the rest are gone by the snapshot).
    pub recorded_present: std::collections::HashSet<i32>,
    /// The game's data, for what scripts instantiate at run time (the catalog); None when loaded from a recording.
    pub assets: Option<Box<crate::assets::db::AssetDb>>,
    /// CatalogBehaviour.SelectedItem: the SpawnableAsset's name (the catalog UI is not rewritten).
    pub catalog_selected: Option<String>,
    /// CatalogBehaviour.SelectedContraption's ObjectStates and SelectedContraptionMetaData.DisplayName.
    pub catalog_contraption: Option<(Vec<save::ObjectState>, String)>,
    /// UndoControllerBehaviour's history and position.
    pub undo: undo::UndoController,
    /// GearManagerBehaviour.Gears
    pub gears: gear::GearList,
    /// NotificationControllerBehaviour.Show's texts since the UI last took them.
    pub notifications: Vec<String>,
    /// ContraptionSavePoint, the contraption dialogs waiting for the UI and the catalogue refresh flag (contraption.rs).
    pub contraption: contraption::ContraptionState,
    /// CatalogBehaviour.SpawnableOutline (spawnable_outline.rs).
    pub spawnable_outline: spawnable_outline::SpawnableOutline,
    /// ClipboardControllerBehaviour.ObjectStateInClipboard
    pub clipboard: Option<save::Clipboard>,
    /// SerialisableIdentity.UniqueIdentity by the Transform id of its object (set by Regenerate or a loaded ObjectState).
    pub identities: HashMap<i32, u128>,
    pub guid_counter: u64,
    /// The stab arrays of pasted PhysicalBehaviours (physical index) between ObjectState injection and OnAfterDeserialise.
    pub saved_stabs: HashMap<usize, save::SavedStabs>,
    /// The storedReferenceFields of injected prototypes, with their class and the component ids of their object, between
    /// ObjectState.Reconstruct and ObjectState.LinkReferences.
    pub pending_reference_links: Vec<(String, Vec<i32>, Vec<(String, save::ComponentReferenceLink)>)>,
    /// The arena index of the struct of each script component (by component id), recorded when a clone is linked.
    script_slots: HashMap<i32, save_slots::Slot>,
    /// The stored members of a pasted component that its struct does not hold (by component id), kept for the next copy.
    script_loose: HashMap<i32, Vec<(String, save::SavedValue)>>,
    /// Stored members ObjectState could not read for a copy (class.member and why), listed by the catalog copy and paste test.
    save_gaps: std::collections::BTreeSet<String>,
    /// Behaviour.enabled of pasted components that differ from their prefab's (class, component id, Enabled), applied once the clone is kept.
    pending_enabled: Vec<(String, i32, bool)>,
    /// DrawableComponent.cachedWorldMatrix (by component id): the raw Matrix4x4 the last LateUpdate of an active, enabled
    /// LinefyCircularPolyline cached (`linefy_late_update`).
    linefy_cached: HashMap<i32, Vec<u8>>,
    /// gameObject.name of the catalog's clones (by the id of the root Transform).
    pub object_names: HashMap<i32, String>,
    /// The roots (by Transform id) that CatalogBehaviour.Spawn gave a DeregisterBehaviour.
    pub deregister_behaviours: std::collections::HashSet<i32>,
    /// The `transform_nodes` roots carrying an AudioSourceTimeScaleBehaviour (the catalog adds one).
    pub audio_time_scale_roots: Vec<usize>,
    /// PhysicsScene2D m_LastSimulationTime / m_LastSimulationDelta.
    pub last_simulation: (f64, f32),
    /// PhysicsScene2D m_RigidbodyHierarchyChanged: the movement-state list is rebuilt after the next step.
    pub rigidbody_hierarchy_changed: bool,
    /// The ToolController (recordings before the drag tool lack it).
    pub tool: Option<tool::Tool>,
    /// The scene's "Resizing handles" clone and the object its ResizeHandles sits on, while it is active.
    pub resize_handles: Option<(spawn::Instance, usize)>,
    /// level1's CenterOfGravity and CenterOfBounds markers.
    pub centers: Vec<center_of_gravity::Center>,
    /// The ResizeHandles script's state.
    pub resize: resize_handles::ResizeHandles,
    pub input: tool::Input,
    /// Global.main.MousePosition
    pub MousePosition: [f32; 3],
    /// Global.oldMousePos: MousePosition before this frame's UpdateMousePosition.
    pub oldMousePos: [f32; 3],
    /// Global.MouseDelta (set in LateUpdate, so a tool's Update reads the previous frame's).
    pub MouseDelta: [f32; 2],
    /// Global.inactivity: seconds without mouse movement (LateUpdate).
    pub inactivity: f32,
    /// Global.alreadyUnlockedPacifist
    pub alreadyUnlockedPacifist: bool,
    /// SteamworksInitialiser.IsInitialised false: a recording made without a Steam client (the rewrite's local store stands in otherwise).
    pub steam_off: bool,
    /// Global.SlowMotion
    pub SlowMotion: bool,
    /// Global.timeScaleTarget (1 at construction)
    pub timeScaleTarget: InitF32<0x3f800000>,
    /// TriggerEditorBehaviour.IsBeingEdited
    pub trigger_being_edited: bool,
    /// MenuController.main.CurrentPage's name (Global.SetPausedMenu's page).
    pub menu_page: String,
    /// DialogBox.IsAnyDialogboxOpen (OpenDialogboxCount > 0).
    pub dialog_open: bool,
    /// The world position ContextMenuBehaviour.Show(Input.mousePosition) puts the menu at (the UI clamps it to the screen).
    pub context_at: [f32; 2],
    /// A Global.Update hotkey ran ContextMenuBehaviour.Hide since the UI last looked.
    pub context_hidden: bool,
    /// The restore's GameObject ids by name.
    pub objects: HashMap<String, i32>,
    /// A recorded item's DetachFromParentOnStart components whose Start had detached their object (by component id):
    /// the object's former parent in its clone, for the DestroyIfParentDestroyed listener.
    pub recorded_detached: HashMap<i32, usize>,
    /// gapB r27: a loaded ExplosiveBehaviour's recorded Killzone link (its id to the Killzone(Clone)'s GameObject id): the load's
    /// Start makes its Killzone under those ids, so it is the recorded clone (other Awakes took the ids before it in the game).
    pub recorded_killzones: HashMap<i32, i32>,
    /// The next link's parent (ArrayCloner.Start's SetParent, worldPositionStays): the anchor and the parent chain
    /// (object first) the clone root hangs from; the clone root's TRS is its local one then.
    pub link_under: Option<(BareAnchor, Vec<crate::unity::transform::Trs>)>,
    /// The id Object.Instantiate gives its next clone (ids go down by 2).
    pub next_instance_id: i32,
    /// A clone of Global.MetalScrapePrefab: the instance ids it takes (objects and components) and the
    /// Random.value draws its ActivateIfFancyEffects Awakes make; None when the scene was not loaded from data.
    /// Its objects in prefab order: (the ids each takes, its parent object).
    pub metal_scrape: Option<(usize, usize, Vec<(usize, Option<usize>)>)>,
    /// Global.HolleManPrefab: the zombie easter egg instantiated when a zombie head dies (person.rs).
    pub holle_man_prefab: Option<crate::assets::db::ObjRef>,
    /// FireParticleSystem.Emit calls for the drawing: Physical, position, startSize, startColor.
    pub fire_emits: Vec<(usize, [f32; 3], f32, [f32; 4])>,
    /// SmokeParticleSystem.Emit(n) calls (Sizzle) for the drawing: Physical, n.
    pub smoke_emits: Vec<(usize, i32)>,
    /// The modelled managed heap's base for this session (PhysicalBehaviour hash codes); None: the fixed test base.
    pub heap_base: Option<u64>,
    /// A replay's recorded PhysicalBehaviour hash codes by instance id (dump.rs): they replace the modelled ones.
    pub recorded_hashes: dump::RecordedHashes,
    /// The frame whose EarlyUpdate.ScriptRunDelayedStartupFrame pass ran (`fixed_update_scripts`).
    pub startup_pass_frame: Option<i32>,
    /// Physics2DSettings.m_DefaultMaterial's (friction, bounciness); None without one.
    pub default_material2d: Option<(f32, f32)>,
}

impl Default for Random {
    fn default() -> Self {
        Random::new(0)
    }
}

#[cfg(test)]
mod prefs_tests {
    use super::*;

    #[test]
    fn prefs_from_json() {
        let p = Prefs::from_json(r#"{"CollisionQuality": "Continuous", "ToolSelectionMode": 1, "PhysicsIterations": 16, "ShowTutorial": false, "GridSnap": 0.5, "Resolution": {"x": 1}}"#).unwrap();
        assert_eq!((p.CollisionQuality, p.ToolSelectionMode, p.PhysicsIterations, p.ShowTutorial, p.GridSnap), (1, tool::SELECTION_MASS, 16, false, 0.5));
        assert_eq!(p.FragilityMultiplier, 1.0);
        assert!(Prefs::from_json(r#"{"LimbCrushing": 1}"#).is_err());
    }
}

/// PhysicalBehaviour.contactBuffer: static ContactPoint2D[8] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const PHYSICAL_CONTACT_BUFFER: usize = 8;

/// LimbBehaviour.contactBuffer: static ContactPoint2D[8] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const LIMB_CONTACT_BUFFER: usize = 8;
