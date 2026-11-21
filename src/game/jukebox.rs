//! JukeboxBehaviour: the tracks play on its audioSource; SetRenderParams drives the Jaap/Jukebox shader's _MusicVolume
//! (custom_material.rs) and the glow's colour. DestroyYourEars (and the camera shake) while playing with Charge above 100 follow the perceived
//! volume of AudioSource.GetSpectrumData, which reads the audio thread's last DSP block (`audio_spectrum` models
//! FMOD's getSpectrum and the DSP clock).
#![allow(non_snake_case)]

use super::{d, mathf, script, Game};

script! {
    Jukebox {
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// physicalBehaviour (a serialised reference).
        physical: usize = 0,
        /// tracks' names (Start: the serialised tracks; custom songs from the Jukebox folder are unimplemented).
        tracks: Vec<String> = Vec::new(),
        /// tracks is null (a probe's reset): Start's `tracks.ToList()` throws.
        tracks_null: bool = false,
        trackIndex: i32 = 0,
        /// DamageRadius and DamageLayers (LayerMask bits), read by DestroyYourEars.
        DamageRadius: f32 = 15.0,
        DamageLayers: u32 = 0,
        /// perceivedVolume, previousPerceivedVolume and chaos (private fields).
        perceivedVolume: f32 = 0.0,
        previousPerceivedVolume: f32 = 0.0,
        chaos: f32 = 0.0,
        /// renderer and glow (serialised SpriteRenderers; SetRenderParams dereferences both).
        renderer: Option<super::visual::SpriteAt> = None,
        glow: Option<super::visual::SpriteAt> = None,
        glowGradient: super::visual::Gradient = super::visual::Gradient::default(),
        glowGradientSpeed: f32 = 0.1,
        /// propertyBlock (Start: renderer.GetPropertyBlock).
        propertyBlock: Option<super::props::PropBlock> = None,
    }
}

/// JukeboxSongLoader's serialised fields. The shipped scenes set Path "Jukebox" with ".wav" and ".mp3" and no OnLoadStart calls.
#[derive(Clone, Debug)]
pub struct JukeboxSongLoader {
    pub Path: String,
    pub AllowedExtensions: Vec<String>,
}

impl Default for JukeboxSongLoader {
    fn default() -> Self {
        Self {
            Path: "Jukebox".to_string(),
            AllowedExtensions: vec![".wav".into(), ".mp3".into()],
        }
    }
}

/// FileInfo.Extension: from the last '.' of the file name to its end ("" when there is none or the dot ends the name).
fn file_extension(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[i..],
        _ => "",
    }
}

impl JukeboxSongLoader {
    /// JukeboxSongLoader..ctor: Path = "Jukebox", AllowedExtensions = { ".wav", ".mp3" } (and a new, empty OnLoadStart).
    pub fn new() -> Self {
        JukeboxSongLoader { Path: "Jukebox".into(), AllowedExtensions: vec![".wav".into(), ".mp3".into()] }
    }

    /// JukeboxSongLoader.LoadFiles: new DirectoryInfo(Path).GetFiles() (the folder's files in the file system's order)
    /// whose Extension is one of AllowedExtensions (a case-sensitive match).
    pub fn load_files(&self, dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        let files = entries.flatten().filter(|e| e.file_type().is_ok_and(|t| !t.is_dir())).map(|e| e.path());
        files.filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| self.AllowedExtensions.iter().any(|x| x == file_extension(n)))).collect()
    }

    /// JukeboxSongLoader.ExtractData: Utils.FileToAudioClip of each file; one that throws is logged
    /// ("Could not load audio file: " and the exception) and skipped.
    pub fn extract_data(files: &[std::path::PathBuf]) -> Vec<super::audio::ClipRef> {
        files.iter().enumerate().filter_map(|(n, p)| super::audio::file_to_audio_clip(p, n as i64).map_err(|e| eprintln!("Could not load audio file: {}
{e}", p.display())).ok()).collect()
    }

    /// JukeboxSongLoader.Start: OnLoadStart.Invoke(), then, the first time of a run (hasLoaded), CustomSongs is empty
    /// without the folder (Debug.Log "<Path> not found. Can't load custom jukebox files."), else ExtractData(LoadFiles()).
    /// `base` is the folder a relative Path is read from (the game's, the player's own music; it never touches the network).
    pub fn start(&self, base: &std::path::Path) -> Vec<super::audio::ClipRef> {
        let dir = base.join(&self.Path);
        if !dir.is_dir() {
            eprintln!("{} not found. Can't load custom jukebox files.", self.Path);
            return Vec::new();
        }
        Self::extract_data(&self.load_files(&dir))
    }
}

/// JukeboxSongLoader.CustomSongs (static) after its Start ran once (hasLoaded).
pub fn jukebox_custom_songs() -> &'static [super::audio::ClipRef] {
    static SONGS: std::sync::OnceLock<Vec<super::audio::ClipRef>> = std::sync::OnceLock::new();
    SONGS.get_or_init(|| match crate::game_assets::find_game(None) {
        Ok(game) => JukeboxSongLoader::new().start(&game),
        Err(_) => Vec::new(),
    })
}

impl Game {
    /// JukeboxBehaviour.Start's tracks: CustomSongs replace them when there are any.
    pub(crate) fn jukebox_start_tracks(&mut self, j: usize) {
        if self.jukeboxes[j].tracks_null {
            crate::game::menu::throw("ArgumentNullException: Value cannot be null.
Parameter name: source");
        }
        let songs = jukebox_custom_songs();
        if !songs.is_empty() {
            let id = self.jukeboxes[j].id;
            self.jukeboxes[j].tracks = songs.iter().map(|c| c.clip.name.clone()).collect();
            self.audio.clip_fields.insert((id, "tracks".into()), songs.iter().cloned().map(Some).collect());
        }
    }

    /// JukeboxBehaviour.audioSource
    pub(super) fn jukebox_source(&self, j: usize) -> i32 {
        self.audio.source_field(self.jukeboxes[j].id, "audioSource").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: JukeboxBehaviour.audioSource"))
    }

    /// JukeboxBehaviour.Use (when enabled): NextSong plays when the source is quiet, else it stops.
    pub fn jukebox_use(&mut self, j: usize) {
        if self.jukeboxes[j].enabled {
            let s = self.jukebox_source(j);
            if !self.audio.is_playing(s) {
                self.jukebox_song(j, 1);
            } else {
                self.audio.stop(s);
            }
        }
    }

    /// audioSource.clip = tracks[trackIndex]; audioSource.Play(); ShowPlayingNotification.
    fn jukebox_play_track(&mut self, j: usize) {
        let (id, k) = (self.jukeboxes[j].id, self.jukeboxes[j].trackIndex as usize);
        let src = self.jukebox_source(j);
        let clip = self.audio.clips(id, "tracks")[k].clone();
        self.audio.set_clip(src, clip);
        self.audio.play(src);
        self.jukebox_show_playing_notification(j);
    }

    /// JukeboxBehaviour.ShowPlayingNotification: the current track's name without its extension.
    pub(crate) fn jukebox_show_playing_notification(&mut self, j: usize) {
        // audioSource.clip.name: no clip throws in the try and again in the catch.
        if self.audio.get(self.jukebox_source(j)).clip.is_none() {
            crate::game::menu::throw("NullReferenceException: JukeboxBehaviour.audioSource.clip");
        }
        // ponytail: the name is tracks[trackIndex], the clip Play set; read the clip's own name if they ever differ.
        let s = &self.jukeboxes[j];
        let name = &s.tracks[s.trackIndex as usize];
        // Path.GetFileNameWithoutExtension
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let stem = file.rfind('.').map_or(file, |i| &file[..i]);
        let text = format!("Now playing <color=#ff54f9>{stem}</color>");
        self.notify(text);
    }

    /// NextSong (offset 1) and PreviousSong (-1), when enabled: SetTrackFromOffset wraps trackIndex, the clip plays
    /// and ShowPlayingNotification shows its name without the extension.
    pub(super) fn jukebox_song(&mut self, j: usize, offset: i32) {
        let s = &mut self.jukeboxes[j];
        if !s.enabled {
            return;
        }
        self.jukebox_set_track_from_offset(j, offset);
        self.jukebox_play_track(j);
    }

    /// JukeboxBehaviour.SetTrackFromOffset: trackIndex += offset, wrapped; the track's index.
    pub(crate) fn jukebox_set_track_from_offset(&mut self, j: usize, offset: i32) -> usize {
        let s = &mut self.jukeboxes[j];
        if s.tracks.is_empty() {
            crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
        }
        s.trackIndex += offset;
        if s.trackIndex >= s.tracks.len() as i32 {
            s.trackIndex = 0;
        }
        if s.trackIndex < 0 {
            s.trackIndex = s.tracks.len() as i32 - 1;
        }
        s.trackIndex as usize
    }

    /// JukeboxBehaviour.SetTrackFromIndex: trackIndex = index, wrapped like SetTrackFromOffset; the track's index.
    pub(crate) fn jukebox_set_track_from_index(&mut self, j: usize, index: i32) -> usize {
        let s = &mut self.jukeboxes[j];
        if s.tracks.is_empty() {
            crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
        }
        s.trackIndex = index;
        if s.trackIndex >= s.tracks.len() as i32 {
            s.trackIndex = 0;
        }
        if s.trackIndex < 0 {
            s.trackIndex = s.tracks.len() as i32 - 1;
        }
        s.trackIndex as usize
    }

    /// JukeboxBehaviour.StopMusic
    pub(crate) fn jukebox_stop_music(&mut self, j: usize) {
        let s = self.jukebox_source(j);
        self.audio.stop(s);
    }

    /// JukeboxBehaviour.PlayIndex: SetTrackFromIndex wraps like SetTrackFromOffset, then the clip plays and
    /// ShowPlayingNotification (not gated on enabled).
    #[allow(dead_code)]
    pub fn jukebox_play_index(&mut self, j: usize, index: i32) {
        self.jukebox_set_track_from_index(j, index);
        self.jukebox_play_track(j);
    }

    /// JukeboxBehaviour.OnDisable: audioSource.Stop(), perceivedVolume = 0, glow.enabled = false.
    pub fn jukebox_on_disable(&mut self, j: usize) {
        let s = self.jukebox_source(j);
        self.audio.stop(s);
        self.jukeboxes[j].perceivedVolume = 0.0;
        let glow = self.jukebox_glow(j);
        self.set_sprite_enabled(glow, false);
    }

    /// JukeboxBehaviour.OnEnable: glow.enabled = true.
    pub fn jukebox_on_enable(&mut self, j: usize) {
        let glow = self.jukebox_glow(j);
        self.set_sprite_enabled(glow, true);
    }

    fn jukebox_glow(&self, j: usize) -> super::visual::SpriteAt {
        self.jukeboxes[j].glow.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: JukeboxBehaviour.glow"))
    }

    /// JukeboxBehaviour.SetRenderParams: num = perceivedVolume * Max(1, 10 chaos) as the renderer's _MusicVolume (its _MainTex
    /// is the sprite's own texture, which the renderer already draws), and glow.color = glowGradient at Time.time *
    /// glowGradientSpeed % 1, times num * 4, alpha 1.
    pub(crate) fn jukebox_set_render_params(&mut self, j: usize) {
        let x = &self.jukeboxes[j];
        let renderer = x.renderer.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: JukeboxBehaviour.renderer"));
        let num = (d(x.perceivedVolume) * mathf::max(1.0, (d(10.0f32) * x.chaos).f())).f();
        let t = (d(self.time.time) * x.glowGradientSpeed).f() % 1.0;
        let mut color = super::visual::scale(x.glowGradient.evaluate(t), (d(num) * 4.0f32).f());
        color[3] = 1.0;
        let mut block = self.jukeboxes[j].propertyBlock.take().unwrap_or_else(|| self.get_property_block(renderer));
        block.set_float("_MusicVolume", num);
        self.set_property_block(renderer, &block);
        self.jukeboxes[j].propertyBlock = Some(block);
        let glow = self.jukebox_glow(j);
        self.set_sprite_color(glow, color);
    }

    /// JukeboxBehaviour.FixedUpdate: the audioSource's distances, spread and spatialBlend follow Charge. Playing with a clip,
    /// CalculateVisualiser sets perceivedVolume; chaos = Clamp01(Charge / 1000f); above 0.1 it shakes the camera by
    /// perceivedVolume * chaos * 4f and, when the volume rose by more than 0.025 / chaos, DestroyYourEars. Else perceivedVolume
    /// is 0. previousPerceivedVolume follows, then SetRenderParams. (The distortion filter is presentation.)
    pub fn jukebox_fixed_update(&mut self, j: usize) {
        let charge = self.physicals[self.jukeboxes[j].physical].Charge();
        let s = self.jukebox_source(j);
        let a = self.audio.get_mut(s);
        a.min_distance = (d(charge) * 3.0f32 + 15.0f32).f();
        a.max_distance = (d(charge) * 10.0f32 + 1500.0f32).f();
        a.set_spread((d(charge) * 10.0f32).f());
        a.set_spatial_blend((1.0f32 - d(mathf::clamp01((d(charge) / 10.0f32).f()))).f());
        let playing = a.clip.is_some() && self.audio.is_playing(s);
        if playing {
            self.jukebox_calculate_visualiser(j, s);
            let chaos = mathf::clamp01((d(charge) / 1000.0f32).f());
            self.jukeboxes[j].chaos = chaos;
            if chaos as f64 > 0.1 {
                let p = self.physicals[self.jukeboxes[j].physical].transform.position;
                let x = &self.jukeboxes[j];
                let intensity = (d(x.perceivedVolume) * chaos * 4.0f32).f();
                let rose = (d(x.perceivedVolume) - x.previousPerceivedVolume) * chaos > super::D(0.025f32 as f64);
                self.camera_shake_at(intensity, [p[0], p[1]], 0.0);
                if rose {
                    self.jukebox_destroy_your_ears(j);
                }
            }
        } else {
            self.jukeboxes[j].perceivedVolume = 0.0;
        }
        self.jukeboxes[j].previousPerceivedVolume = self.jukeboxes[j].perceivedVolume;
        self.jukebox_set_render_params(j);
    }

    /// JukeboxBehaviour.CalculateVisualiser: GetSpectrumData(new float[256], 0, FFTWindow.Rectangular); f = array.Average() * 27f
    /// (Enumerable.Average sums floats in a double); Mathf.Pow(f, 1.5f), Clamp01.
    pub(crate) fn jukebox_calculate_visualiser(&mut self, j: usize, source: i32) {
        let array = match self.audio.get_spectrum_data(source, 256, 0, super::audio_spectrum::Window::Rectangular) {
            Ok(a) => a,
            Err(e) => crate::game::menu::throw(&e),
        };
        let sum = array.iter().fold(0.0f64, |acc, v| acc + *v as f64);
        let average = (sum / array.len() as f64) as f32;
        let f = (d(average) * 27.0f32).f();
        let f = mathf::pow(f, 1.5f32);
        self.jukeboxes[j].perceivedVolume = mathf::clamp01(f);
    }

    /// JukeboxBehaviour.DestroyYourEars: Physics2D.OverlapCircleNonAlloc(position, DamageRadius, buffer[64], DamageLayers); every
    /// person with a collider there (AliveByTransform of the collider's root) has each limb with a brain crushed, the person
    /// winces and JUKEBOX_HEAD_EXPLOSION unlocks.
    pub(crate) fn jukebox_destroy_your_ears(&mut self, j: usize) {
        let (position, radius, layers) = (self.physicals[self.jukeboxes[j].physical].transform.position, self.jukeboxes[j].DamageRadius, self.jukeboxes[j].DamageLayers);
        let mut hits = self.overlap_circle(crate::b2::math::Vec2::new(position[0], position[1]), radius, layers);
        hits.truncate(64);
        for (physical, collider) in hits {
            let Some(person) = self.alive_of_collider(collider, physical) else { continue };
            for limb in self.people[person].Limbs.clone() {
                if self.limbs[limb].HasBrain {
                    self.limb_crush(limb);
                    self.person_wince(person, 150.0);
                    super::stats::unlock_achievement("JUKEBOX_HEAD_EXPLOSION");
                }
            }
        }
    }

    // JukeboxBehaviour.Start: the Distortion and property block are presentation; the custom songs are jukebox_custom_songs and the two
    // buttons are menu.rs (Menu::Jukebox).
    // JukeboxBehaviour.StopMusic: audioSource.Stop(), which the antenna receiver's Connect calls (blood_wire.rs).
    // JukeboxBehaviour.NextSong: jukebox_song (offset 1).
    // JukeboxBehaviour.PreviousSong: jukebox_song (offset -1).
    // JukeboxBehaviour.SetTrackFromOffset: jukebox_song (trackIndex wraps).
    // JukeboxBehaviour.SetTrackFromIndex: jukebox_play_index.
    // JukeboxBehaviour.ShowPlayingNotification: jukebox_song / jukebox_play_index (self.notify).
    // JukeboxBehaviour.CalculateVisualiser: jukebox_calculate_visualiser.
}

#[cfg(test)]
mod loader_tests {
    use super::*;

    #[test]
    fn loader_defaults_and_extension_filter() {
        let l = JukeboxSongLoader::new();
        assert_eq!((l.Path.as_str(), l.AllowedExtensions.len()), ("Jukebox", 2));
        let dir = std::env::temp_dir().join(format!("jukebox_loader_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub.wav")).unwrap();
        for n in ["a.wav", "b.mp3", "c.ogg", "d.WAV", "e"] {
            std::fs::write(dir.join(n), b"").unwrap();
        }
        let mut names: Vec<String> = l.load_files(&dir).iter().map(|p| p.file_name().unwrap().to_string_lossy().into()).collect();
        names.sort();
        assert_eq!(names, ["a.wav", "b.mp3"]);
        assert!(l.start(&std::env::temp_dir().join("no_such_jukebox_base")).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod fixed_update_tests {
    use super::*;
    use crate::game::audio::{AudioSource, Clip};
    use crate::assets::audio::Pcm;
    use std::sync::Arc;

    /// A charged (Charge above 100) jukebox with a looping clip playing runs FixedUpdate: the spectrum is read (zeros in the first block,
    /// then the clip's), perceivedVolume follows and the camera shakes; no stop.
    #[test]
    fn a_charged_jukebox_runs_fixed_update() {
        let mut g = Game::default();
        let mut p = crate::game::Physical::default();
        p.charge = 500.0;
        g.physicals.push(p);
        let mut x = Jukebox::default();
        x.id = 7;
        g.physicals[0].sprite = Some((crate::assets::db::ObjRef { file: usize::MAX, path_id: 2 }, [1.0; 4], 0));
        (x.renderer, x.glow) = (Some(crate::game::visual::SpriteAt::Physical(0)), Some(crate::game::visual::SpriteAt::Physical(0)));
        g.jukeboxes.push(x);
        let data: Vec<i16> = (0..88200).map(|i| (12000.0 * (std::f64::consts::TAU * 440.0 * i as f64 / 44100.0).sin()) as i16).collect();
        let pcm = Pcm { channels: 1, frequency: 44100, data };
        let clip = crate::assets::classes::AudioClip { name: "sine".into(), load_type: 0, channels: 1, frequency: 44100, length: 2.0, resource: Default::default(), compression_format: 0 };
        let clip = Arc::new(Clip { asset: crate::assets::db::ObjRef { file: usize::MAX, path_id: 1 }, clip, pcm: Some(Arc::new(pcm)), stream: None });
        let mut src = AudioSource::default();
        src.looping = true;
        g.audio.add(70, src);
        g.audio.set_clip(70, Some(clip));
        g.audio.play(70);
        g.audio.source_fields.insert((7, "audioSource".into()), vec![Some(70)]);
        let mut seen = Vec::new();
        for _ in 0..50 {
            g.audio.update(0.02);
            g.jukebox_fixed_update(0);
            seen.push(g.jukeboxes[0].perceivedVolume);
        }
        assert_eq!(g.jukeboxes[0].chaos, 0.5);
        // The first read starts the history: silence, whose 1e-8 imaginary parts give the tiny DC bin.
        assert!(seen[0] < 1e-6, "{}", seen[0]);
        assert!(seen[10..].iter().all(|v| *v > 0.0 && *v <= 1.0), "{seen:?}");
        assert!(g.camera.shakeIntensity > 0.0);
    }

    /// A real asset clip of the user's install (decoded from its FSB5 bank, as the mixer would) through the same path.
    #[test]
    fn a_real_clip_has_a_spectrum() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let db = crate::assets::db::AssetDb::open(&game).unwrap();
        let Some((asset, clip)) = db.files.iter().enumerate().flat_map(|(fi, f)| f.objects.iter().filter(|o| o.class_id == 83).map(move |o| (crate::assets::db::ObjRef { file: fi, path_id: o.path_id }, o))).filter_map(|(r, _o)| {
            let c = crate::assets::classes::AudioClip::read(db.data(r)).ok()?;
            (c.length > 3.0).then_some((r, c))
        }).next().map(|(r, c)| (r, c)) else { return };
        let mut g = Game::default();
        let mut p = crate::game::Physical::default();
        p.charge = 500.0;
        g.physicals.push(p);
        let mut x = Jukebox::default();
        x.id = 7;
        g.physicals[0].sprite = Some((crate::assets::db::ObjRef { file: usize::MAX, path_id: 2 }, [1.0; 4], 0));
        (x.renderer, x.glow) = (Some(crate::game::visual::SpriteAt::Physical(0)), Some(crate::game::visual::SpriteAt::Physical(0)));
        g.jukeboxes.push(x);
        let mut src = AudioSource::default();
        src.looping = true;
        g.audio.add(70, src);
        g.audio.set_clip(70, Some(Arc::new(Clip { asset, clip, pcm: None, stream: None })));
        g.audio.play(70);
        g.audio.source_fields.insert((7, "audioSource".into()), vec![Some(70)]);
        let mut loudest = 0.0f32;
        for _ in 0..100 {
            g.audio.update(0.02);
            g.jukebox_fixed_update(0);
            loudest = loudest.max(g.jukeboxes[0].perceivedVolume);
        }
        assert!(loudest > 0.0, "silent");
    }
}
