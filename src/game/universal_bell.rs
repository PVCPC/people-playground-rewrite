//! UniversalBellBehaviour (the piano-note bell). Its clapper swings on the bell's own acceleration (FixedUpdate) and on
//! gravity (Update); a clapper that reaches +-24 degrees bounces, which changes the body's angular velocity. Use (Play)
//! kicks the clapper and torques the body. Start's SetBellIndex sets the physical's impact and shot sound arrays to the
//! note's one clip, whose lengths the draws count. The DetailViewLabel is a world text (the note's short name, its rotation
//! kept at identity). Colours and context menus are presentation.
#![allow(non_snake_case)]

use super::{mathf, script, BareAnchor, Game};
use crate::unity::transform::Trs;

script! {
    UniversalBell {
        BellClipIndex: i32 = 0,
        ClapperVelocityInfluence: f32 = 100.0,
        ClapperIntensity: f32 = 0.0,
    }
    links {
        /// The component's instance id and enabled (Update and FixedUpdate run only while it is).
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour (GetComponent in Start).
        physical: usize = 0,
        /// The clapper's angle (degrees) and its angular velocity; the private fields of the same names.
        clapperRot: f32 = 0.0,
        clapperRotVel: f32 = 0.0,
        /// previousVelocity: the body's velocity at the last FixedUpdate.
        previousVelocity: [f32; 2] = [0.0; 2],
        /// DetailViewLabel (world_text.rs).
        DetailViewLabel: Option<usize> = None,
        /// clapperTransform (clapperSpriteRenderer.transform in Start): its object's Transform id.
        clapper: Option<i32> = None,
    }
}

/// PianoKey's names, in value order.
const PIANO_KEYS: [&str; 12] = ["C", "CSharpDFlat", "D", "DSharpEFlat", "E", "F", "FSharpGFlat", "G", "GSharpAFlat", "A", "ASharpBFlat", "B"];

/// BellClip.GetDisplayName(shorten: true).
pub fn bell_short_name(name: &str, key: i32) -> String {
    bell_display_name(name, key, true)
}

/// BellClip.GetDisplayName: a sharp's key name with "Sharp" made "♯\n" (shortened: cut to six characters first; else
/// piano_key_label, "Flat" made "♭" too); otherwise the key name.
pub fn bell_display_name(name: &str, key: i32, shorten: bool) -> String {
    let k = PIANO_KEYS[key as usize];
    if !name.contains("Sharp") {
        return k.trim().to_string();
    }
    if shorten { k[..6].replace("Sharp", "\u{266f}\n") } else { super::menu::piano_key_label(key as usize) }
}

impl Game {
    /// UniversalBellBehaviour's increment/decrement label: "Set to " + BellClips[BellClipIndex + step]'s short name.
    /// List<T>'s indexer throws ArgumentOutOfRangeException past either end; the menu shows a button only while its
    /// condition (the same bound) holds, so only a dump of every row reads it.
    pub fn universal_bell_label(&self, b: usize, step: i32) -> String {
        let i = self.universal_bells[b].BellClipIndex + step;
        match usize::try_from(i).ok().and_then(|i| self.bell_clips.get(i)) {
            Some((name, key)) => format!("Set to {}", bell_short_name(name, *key)),
            None => "!ArgumentOutOfRangeException".into(),
        }
    }

    /// UniversalBellBehaviour.Start's three buttons ("Set bell note" opens the piano dialog).
    pub(crate) fn universal_bell_menu(&mut self, b: usize) {
        use super::menu::{Cond, Label, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.universal_bells[b].physical].menu;
        menu.push(B::new("bellSetNote", crate::il!(str "UniversalBellBehaviour::Start" 1), crate::il!(str "UniversalBellBehaviour::Start" 1), A::BellPiano(b)));
        menu.push(B { label: Label::BellNote(b, 1), condition: Cond::BellNote(b, 1), ..B::new("bellIncrementNote", "", crate::il!(str "UniversalBellBehaviour::Start" 4), A::BellNote(b, 1)) });
        menu.push(B { label: Label::BellNote(b, -1), condition: Cond::BellNote(b, -1), ..B::new("bellDecrementNote", "", crate::il!(str "UniversalBellBehaviour::Start" 6), A::BellNote(b, -1)) });
    }

    /// UniversalBellBehaviour.Start, UniversalBellBehaviour.SetBellIndex and UniversalBellBehaviour.GetBellClip (the menu's SetBellIndex is menu.rs): SetBellIndex makes the physical's OverrideImpactSounds one clip long (it is
    /// reused when it already is) and OverrideShotSounds the same array.
    pub fn universal_bell_start(&mut self, b: usize) {
        // DetailViewLabel.transform.eulerAngles = default
        let label = Self::text_ref(self.universal_bells[b].DetailViewLabel);
        self.text_set_rotation_identity(label);
        self.universal_bell_menu(b);
        let i = self.universal_bells[b].BellClipIndex;
        self.universal_bell_set_index(b, i);
    }

    /// UniversalBellBehaviour.SetBellIndex(i): BellClipIndex = i; GetBellClip indexes the list; the colours and label
    /// are presentation.
    pub fn universal_bell_set_index(&mut self, b: usize, i: i32) {
        self.universal_bells[b].BellClipIndex = i;
        if !(0..self.bell_clips.len() as i32).contains(&self.universal_bells[b].BellClipIndex) {
            crate::game::menu::throw("ArgumentOutOfRangeException: Index was out of range. Must be non-negative and less than the size of the collection.
Parameter name: index");
        }
        let p = self.universal_bells[b].physical;
        self.physicals[p].OverrideImpactSounds = 1;
        self.physicals[p].OverrideShotSounds = 1;
        let clip = self.bell_clip_refs[self.universal_bells[b].BellClipIndex as usize].clone();
        let id = self.physicals[p].id;
        self.audio.clip_fields.insert((id, "OverrideImpactSounds".into()), vec![clip.clone()]);
        self.audio.clip_fields.insert((id, "OverrideShotSounds".into()), vec![clip]);
        self.universal_bell_write_label(b);
    }

    /// SetBellIndex's last line: DetailViewLabel.text = bellClip.GetDisplayName(shorten: true).
    fn universal_bell_write_label(&mut self, b: usize) {
        let (name, key) = self.bell_clips[self.universal_bells[b].BellClipIndex as usize].clone();
        let field = self.universal_bells[b].DetailViewLabel;
        self.text_write(field, &bell_short_name(&name, key));
    }

    /// UniversalBellBehaviour.FixedUpdate: the body's velocity change, in the bell's local x, kicks the clapper.
    pub fn universal_bell_fixed_update(&mut self, b: usize) {
        let p = self.universal_bells[b].physical;
        let rb = self.physicals[p].rigidbody.clone();
        let velocity = rb.velocity(&self.world);
        let previous = self.universal_bells[b].previousVelocity;
        let delta = [velocity[0] - previous[0], velocity[1] - previous[1], 0.0];
        // base.transform.InverseTransformVector(delta).x
        let local = self.anchored_inverse_vector(BareAnchor::Physical(p), &[], delta);
        let s = &mut self.universal_bells[b];
        s.clapperRotVel -= local[0] * s.ClapperVelocityInfluence;
        s.previousVelocity = velocity;
    }

    /// UniversalBellBehaviour.Update: the clapper's velocity eases, gravity swings it, and it moves.
    pub fn universal_bell_update(&mut self, b: usize) {
        // DetailViewLabel.transform.eulerAngles = default
        let label = Self::text_ref(self.universal_bells[b].DetailViewLabel);
        self.text_set_rotation_identity(label);
        let p = self.universal_bells[b].physical;
        let dt = self.time.deltaTime;
        let eased = mathf::lerp_factor_dt(0.9, dt);
        {
            let s = &mut self.universal_bells[b];
            s.clapperRotVel = mathf::lerp(s.clapperRotVel, 0.0, eased);
        }
        let rot = self.universal_bells[b].clapperRot;
        if rot < -24.0 {
            self.universal_bells[b].clapperRot = -24.0;
            self.universal_bell_bounce(b);
        } else if rot > 24.0 {
            self.universal_bells[b].clapperRot = 24.0;
            self.universal_bell_bounce(b);
        }
        let rb = self.physicals[p].rigidbody.clone();
        let g = [self.world.gravity.x, self.world.gravity.y];
        // Physics2D.gravity.magnitude * rigidbody.gravityScale
        let num = mathf::magnitude2(g) * rb.gravity_scale(&self.world);
        if num > f32::EPSILON {
            let vector = [g[0] / num, g[1] / num];
            // base.transform.InverseTransformDirection(vector): the inverse rotations only, no scale.
            let v3 = self.anchored_inverse_direction(p, [vector[0], vector[1], 0.0]);
            let angle = (crate::game::mathf::atan2(v3[0], (0.0 - v3[1]))) * 57.29578;
            let clamped = mathf::clamp(angle - self.universal_bells[b].clapperRot, -90.0, 90.0);
            // Vector2.Dot(v, Vector2.down) = v.x * 0 + v.y * -1
            let dot = v3[0] * 0.0 + v3[1] * -1.0;
            let weight = mathf::max(dot, 0.05);
            let time_scale = self.time.timeScale;
            let s = &mut self.universal_bells[b];
            s.clapperRotVel += num / 9.81 * time_scale * clamped * weight;
        }
        let s = &mut self.universal_bells[b];
        s.clapperRot += s.clapperRotVel * dt;
        // clapperTransform.localEulerAngles = new Vector3(0, 0, clapperRot)
        let (rot, clapper) = (s.clapperRot, s.clapper);
        if let Some(t) = clapper {
            self.with_component_instance(t, |_, inst, o| inst.set_local_rotation(o, super::menu::local_euler_z_rotation(rot)));
        }
    }

    /// The bounce: the clapper's velocity takes the body's spin and reverses, and a Dynamic body's spin loses 10%.
    fn universal_bell_bounce(&mut self, b: usize) {
        let p = self.universal_bells[b].physical;
        let rb = self.physicals[p].rigidbody.clone();
        let spin = rb.angular_velocity(&self.world);
        {
            let s = &mut self.universal_bells[b];
            s.clapperRotVel = mathf::lerp(s.clapperRotVel, spin, 0.2);
            s.clapperRotVel *= -0.25;
        }
        if rb.bodyType == 0 {
            let spin = rb.angular_velocity(&self.world);
            rb.set_angular_velocity(&mut self.world, spin * 0.9);
        }
    }

    /// Transform.InverseTransformDirection of physical `p`'s object: each node's inverse rotation, root first.
    pub(super) fn anchored_inverse_direction(&self, p: usize, v: [f32; 3]) -> [f32; 3] {
        let mut v = [v[0], v[1], v[2], 0.0];
        for node in self.anchored_nodes(BareAnchor::Physical(p), &[]).iter().rev() {
            v = crate::unity::transform::inverse_vector(&Trs { s: [1.0; 4], ..*node }, v);
        }
        [v[0], v[1], v[2]]
    }

    /// UniversalBellBehaviour.Use and UniversalBellBehaviour.Play: ClipSource.PlayOneShot(GetBellClip().Clip), the clapper kicks by ClapperIntensity and the body takes 0.005 of it as torque.
    pub fn universal_bell_use(&mut self, b: usize) {
        let id = self.universal_bells[b].id;
        let source = self.audio.source_field(id, "ClipSource").unwrap_or_else(|| super::menu::throw("NullReferenceException: UniversalBellBehaviour.ClipSource"));
        let clip = self.bell_clip_refs.get(self.universal_bells[b].BellClipIndex as usize).cloned().flatten();
        self.audio.play_one_shot(source, clip.as_ref(), 1.0);
        let i = self.universal_bells[b].ClapperIntensity;
        self.universal_bells[b].clapperRotVel += i;
        let p = self.universal_bells[b].physical;
        let rb = self.physicals[p].rigidbody.clone();
        rb.add_torque(&mut self.world, i * 0.005);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn short_names_match_get_display_name() {
        assert_eq!(super::bell_short_name("C#4 Sharp", 1), "C\u{266F}\n");
        assert_eq!(super::bell_short_name("C4", 0), "C");
        assert_eq!(super::bell_short_name("A Sharp", 10), "A\u{266F}\n");
    }
}
