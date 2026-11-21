//! TransmitterBehaviour (the Radio). Use and UseContinuous pass the activation on to every other enabled transmitter
//! on the same channel (SendMessage to its GameObject), unless the activation's path already holds the root of a
//! transmitter on this channel. Start's channel sound takes an AudioClip instance id the first time a channel is seen.
//! The light and the text are presentation.
#![allow(non_snake_case)]

use super::coroutine::Yield;
use super::signal::Act;
use super::{script, Game};

script! {
    Transmitter {
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>(): its GameObject is the transmitter's.
        phys: usize = 0,
        /// Start's `physicalBehaviour = GetComponent<PhysicalBehaviour>()` (the C# reference cache: `phys` once Start ran).
        physicalBehaviour: Option<usize> = None,
        Channel: String = String::from("0"),
        /// channelSound
        sound: Option<super::audio::ClipRef> = None,
        /// Awake ran: a member of the static `transmitters` set until OnDestroy.
        awake: bool = false,
        /// TextMeshPro (world_text.rs).
        TextMeshPro: Option<usize> = None,
        /// Light (Blink's SpriteRenderer).
        Light: Option<super::visual::SpriteAt> = None,
    }
}

impl Game {
    /// TransmitterBehaviour.Awake: joins the static `transmitters` set.
    pub(crate) fn transmitter_awake(&mut self, t: usize) {
        self.transmitters[t].awake = true;
    }

    /// TransmitterBehaviour.GetChannel
    pub fn transmitter_get_channel(&self, t: usize) -> &str {
        &self.transmitters[t].Channel
    }

    /// TransmitterBehaviour.OnEnable: `TextMeshPro.enabled = true`.
    pub(crate) fn transmitter_on_enable(&mut self, t: usize) {
        let text = Self::text_ref(self.transmitters[t].TextMeshPro);
        self.text_enabled_set(text, true);
    }

    /// TransmitterBehaviour.OnDisable: StopAllCoroutines (Blink), the light (rendering), then `TextMeshPro.enabled = false`.
    pub(crate) fn transmitter_on_disable(&mut self, t: usize) {
        let id = self.transmitters[t].id;
        self.stop_all_coroutines(id);
        let light = self.transmitters[t].Light.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_sprite_enabled(light, false);
        let text = Self::text_ref(self.transmitters[t].TextMeshPro);
        self.text_enabled_set(text, false);
    }

    /// TransmitterBehaviour.OnDestroy: leaves the static `transmitters` set.
    pub(crate) fn transmitter_on_destroy(&mut self, t: usize) {
        self.transmitters[t].awake = false;
    }

    /// TransmitterBehaviour.Blink (StartCoroutine): PlayClipOnce(channelSound) when it is set, Light.enabled = true,
    /// WaitForSeconds(0.05), Light.enabled = false.
    pub(crate) fn transmitter_blink(&mut self, t: usize) {
        let id = self.transmitters[t].id;
        let r = self.transmitter_blink_routine(t);
        self.start_coroutine(id, id, r);
    }

    /// The Blink IEnumerator itself.
    pub(crate) fn transmitter_blink_routine(&self, t: usize) -> crate::game::coroutine::Routine {
        let mut lit = false;
        Box::new(move |g| {
            let light = g.transmitters[t].Light.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            if lit {
                g.set_sprite_enabled(light, false);
                return Yield::Done;
            }
            lit = true;
            if let Some(clip) = g.transmitters[t].sound.clone() {
                g.play_clip_once(g.transmitters[t].phys, Some(clip), 1.0);
            }
            g.set_sprite_enabled(light, true);
            Yield::WaitForSeconds(0.05)
        })
    }

    /// The static `transmitters` set. Its order only orders the receivers, whose effects commute (a received Use
    /// always loops back; a received UseContinuous only counts), so arena order serves.
    fn transmitter_set(&self) -> Vec<usize> {
        (0..self.transmitters.len()).filter(|&t| self.transmitters[t].awake && !self.physicals[self.transmitters[t].phys].destroyed).collect()
    }

    /// TransmitterBehaviour.Start: SetChannel(Channel), then the channel button.
    pub fn transmitter_start(&mut self, t: usize) {
        let channel = self.transmitters[t].Channel.clone();
        self.transmitter_set_channel(t, channel);
        // physicalBehaviour.ContextMenuOptions.Buttons.Add: the channel's entry box (menu.rs).
        use super::menu::{Entry, MenuAction, MenuButton};
        let p = self.transmitters[t].phys;
        self.transmitters[t].physicalBehaviour = Some(p);
        self.physicals[p].menu.push(MenuButton::new("changeRadioChannel", crate::il!(str "TransmitterBehaviour::Start" 1), crate::il!(str "TransmitterBehaviour::Start" 2), MenuAction::Entry(Entry::RadioChannel(t))));
    }

    /// TransmitterBehaviour.SetChannel and TransmitterBehaviour.LoadOrCreateFor: Channel, the text (presentation) and LoadOrCreateFor, which caches one
    /// AudioClip per channel (its first 8 UTF-16 units) in a static dictionary; AudioClip.Create takes an instance id.
    pub(crate) fn transmitter_set_channel(&mut self, t: usize, channel: String) {
        self.text_write(self.transmitters[t].TextMeshPro, &channel);
        self.transmitters[t].Channel = channel.clone();
        let clip = self.transmitter_load_or_create_for(&channel);
        self.transmitters[t].sound = Some(clip);
    }

    /// TransmitterBehaviour.LoadOrCreateFor: the cached clip for the channel's first 8 UTF-16 units, else a new one
    /// (AudioClip.Create of 0 samples throws ArgumentException).
    pub(crate) fn transmitter_load_or_create_for(&mut self, channel: &str) -> super::audio::ClipRef {
        let units: Vec<u16> = channel.encode_utf16().take(8).collect();
        let key = String::from_utf16_lossy(&units);
        if let Some(c) = self.transmitter_sounds.get(&key) {
            return c.clone();
        }
        if units.is_empty() {
            crate::game::menu::throw("ArgumentException: Length of created clip must be larger than 0");
        }
        let id = self.allocate_instance_id();
        let c = channel_clip(&units, id);
        self.transmitter_sounds.insert(key, c.clone());
        c
    }

    /// TransmitterBehaviour.IsLooping
    pub(crate) fn transmitter_is_looping(&self, t: usize, p: &Act) -> bool {
        let channel = &self.transmitters[t].Channel;
        self.transmitter_set().into_iter().filter(|&x| &self.transmitters[x].Channel == channel).any(|x| self.signal_contains(p, self.root_id(self.transmitters[x].phys)))
    }

    /// TransmitterBehaviour.Transmit: the other enabled transmitters on this channel.
    pub(crate) fn transmitter_targets(&self, t: usize) -> Vec<usize> {
        let channel = &self.transmitters[t].Channel;
        self.transmitter_set().into_iter().filter(|&x| x != t && self.transmitters[x].enabled && &self.transmitters[x].Channel == channel).collect()
    }

    /// TransmitterBehaviour.Use: Blink (sound and light) and Transmit("Use", new ActivationPropagation(root, p.Channel, gameObject)).
    pub fn transmitter_use(&mut self, t: usize, p: &Act) {
        if !self.transmitters[t].enabled || self.transmitter_is_looping(t, p) {
            return;
        }
        self.transmitter_blink(t);
        let root = self.root_id(self.transmitters[t].phys);
        let next = self.signal_from(root, self.sig(p).Channel);
        self.transmitter_transmit(t, "Use", &next);
    }

    /// TransmitterBehaviour.UseContinuous: Transmit("UseContinuous", new ActivationPropagation(root, p.Channel)).
    pub fn transmitter_use_continuous(&mut self, t: usize, p: &Act) {
        if !self.transmitters[t].enabled || self.transmitter_is_looping(t, p) {
            return;
        }
        let root = self.root_id(self.transmitters[t].phys);
        let next = self.signal_from(root, self.sig(p).Channel);
        self.transmitter_transmit(t, "UseContinuous", &next);
    }

    /// TransmitterBehaviour.Transmit(name, content): SendMessage(name, content) to every other enabled transmitter on this
    /// channel. "Use": the target's TransmitterBehaviour and PhysicalBehaviour (the modding event, checked at link) take it,
    /// then the use wires added there. "UseContinuous": the target's PhysicalBehaviour counts it (it comes first on the
    /// object), then its TransmitterBehaviour and use wires. No script on a transmitter takes any other name.
    pub(crate) fn transmitter_transmit(&mut self, t: usize, name: &str, content: &Act) {
        for x in self.transmitter_targets(t) {
            let q = self.transmitters[x].phys;
            match name {
                "Use" => {
                    self.transmitter_use(x, content);
                    for w in self.use_wires_on(q) {
                        self.use_wire_receive(w, content, false);
                    }
                }
                "UseContinuous" => {
                    let channel = self.sig(content).Channel as usize;
                    self.physicals[q].ContinuousActivationTracker[channel].Value += 1;
                    self.transmitter_use_continuous(x, content);
                    for w in self.use_wires_on(q) {
                        self.use_wire_receive(w, content, true);
                    }
                }
                _ => {}
            }
        }
    }
}

/// TransmitterBehaviour.LoadOrCreateFor's AudioClip: 320 samples per character at 8000 Hz (mono), a square wave of
/// 200 + 8 * (byte)c Hz fading in over each character (the engine keeps 16-bit samples).
fn channel_clip(units: &[u16], id: i32) -> super::audio::ClipRef {
    let mut data = Vec::with_capacity(320 * units.len());
    for (k, &u) in units.iter().enumerate() {
        let b = (u as u8) as f32;
        for j in 0..320usize {
            let n = (j + 320 * k) as f32;
            let freq = (200.0f64 + 8.0 * b as f64) as f32;
            let num5 = (j as f32 / 320.0) as f64;
            // Mathf.Sin(PI * 2 * n * freq / 8000), Mathf.Round half to even
            let x = ((std::f32::consts::PI as f64) * 2.0 * n as f64 * freq as f64 / 8000.0) as f32;
            let r = (super::mathf::sin(x) as f64).round_ties_even();
            let v = (r * num5 * 0.25) as f32;
            data.push((v * 32767.0).round() as i16);
        }
    }
    let name = format!("{} clip", String::from_utf16_lossy(units));
    let length = data.len() as f32 / 8000.0;
    let clip = crate::assets::classes::AudioClip { name, load_type: 0, preload_audio_data: true, load_in_background: false, channels: 1, frequency: 8000, length, resource: Default::default(), compression_format: 0 };
    let pcm = crate::assets::audio::Pcm { channels: 1, frequency: 8000, data };
    std::sync::Arc::new(super::audio::Clip { asset: crate::assets::db::ObjRef { file: usize::MAX, path_id: id as i64 }, clip, pcm: Some(std::sync::Arc::new(pcm)), stream: None })
}
