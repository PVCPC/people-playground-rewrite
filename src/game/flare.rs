//! FlareBehaviour (the Signal Flare): once lit (Use, or Start while Activated) it enables its TorchBehaviour, hides
//! FlareTopSprite and plays FlareAudioSource. The particles are presentation.
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    Flare {
        Activated: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled (Use does not read it)
        enabled: bool = true,
        /// TorchBehaviour's instance id (`Game::torches` is found by it at run time).
        torch: i32 = 0,
        /// FlareAudioSource's instance id and FlareTopSprite.
        FlareAudioSource: i32 = 0,
        FlareTopSprite: super::visual::SpriteAt = super::visual::SpriteAt::Renderer(0),
    }
}

impl Game {
    /// The TorchBehaviour this flare enables.
    fn flare_torch(&self, f: usize) -> usize {
        let id = self.flares[f].torch;
        self.torches.iter().position(|t| t.id == id).expect("FlareBehaviour.TorchBehaviour without its TorchBehaviour")
    }

    /// FlareBehaviour.Start: a flare that starts Activated enables its torch.
    pub fn flare_start(&mut self, f: usize) {
        if self.flares[f].Activated {
            let t = self.flare_torch(f);
            self.torch_set_enabled(t);
            self.flare_light(f);
            self.ps_field_call(self.flares[f].id, "FlareSystem", true, super::PsCall::Play);
        }
    }

    /// FlareTopSprite.enabled = false; FlareAudioSource.Play().
    fn flare_light(&mut self, f: usize) {
        self.set_sprite_enabled(self.flares[f].FlareTopSprite, false);
        self.audio.play(self.flares[f].FlareAudioSource);
    }

    /// FlareBehaviour.Use: a flare that is not yet lit lights its torch and becomes Activated.
    pub fn flare_use(&mut self, f: usize) {
        if !self.flares[f].Activated {
            let t = self.flare_torch(f);
            self.torch_set_enabled(t);
            self.set_sprite_enabled(self.flares[f].FlareTopSprite, false);
            let id = self.flares[f].id;
            self.ps_field_call(id, "FlareSystem", true, super::PsCall::Play);
            self.ps_field_call(id, "FlareTopSystem", false, super::PsCall::Play);
            self.audio.play(self.flares[f].FlareAudioSource);
            self.flares[f].Activated = true;
        }
    }
}
