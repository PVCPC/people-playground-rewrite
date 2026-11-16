//! The stored members of a script component that its rewritten struct keeps outside the dumped `fields()` (in the struct's links, or
//! in an arena of plain values), so ObjectState reads and injects them like the dumped ones: `Game::slot_extras` names them per slot.
use super::save::SavedValue;
use super::save_slots::Slot;
use super::sentry_turret::SentryState;
use super::*;

/// One live member: how to read it as the codec's text form and how to set it from one.
pub(super) enum Extra<'a> {
    /// Any `Field`.
    F(&'a mut dyn Field),
    /// A LayerMask held as its bits (the C# LayerMask is its int).
    Mask(&'a mut u32),
    /// A float[].
    Floats(&'a mut Vec<f32>),
    /// SentryTurretAIState (an int enum).
    Sentry(&'a mut SentryState),
}

impl Extra<'_> {
    pub(super) fn get(&self) -> SavedValue {
        match self {
            Extra::F(f) => SavedValue::Field(f.show()),
            Extra::Mask(m) => SavedValue::Field((**m as i32).to_string()),
            Extra::Floats(v) => SavedValue::Floats((**v).clone()),
            Extra::Sentry(s) => SavedValue::Field(match **s {
                SentryState::Idle => 0,
                SentryState::Firing => 1,
                SentryState::Searching => 2,
                SentryState::Panicking => 3,
            }
            .to_string()),
        }
    }

    /// MonoBehaviourPrototype.InjectIntoMonoBehaviour for the member (Debug.LogWarning("Cannot set field ...") on a failure).
    pub(super) fn set(&mut self, v: &SavedValue) {
        match (self, v) {
            (Extra::F(f), SavedValue::Field(t)) => {
                let _ = f.parse(t);
            }
            (Extra::Mask(m), SavedValue::Field(t)) => {
                if let Ok(i) = str::parse::<i32>(t) {
                    **m = i as u32;
                }
            }
            (Extra::Floats(f), SavedValue::Floats(v)) => **f = v.clone(),
            (Extra::Sentry(s), SavedValue::Field(t)) => {
                if let Some(x) = str::parse::<i64>(t).ok().and_then(SentryState::from_int) {
                    **s = x;
                }
            }
            _ => {}
        }
    }
}

impl Game {
    /// The extra members of the struct in `slot` (the component `id`).
    pub(super) fn slot_extras(&mut self, slot: Slot, id: i32) -> Vec<(&'static str, Extra<'_>)> {
        match slot {
            Slot::Airfoils(i) => vec![("CurrentWingTypeName", Extra::F(&mut self.airfoils[i].CurrentWingTypeName))],
            Slot::Beamformers(i) => vec![("LayersToHit", Extra::F(&mut self.beamformers[i].LayersToHit))],
            Slot::Firearms(i) => vec![("LayersToHit", Extra::Mask(&mut self.firearms[i].LayersToHit))],
            Slot::LightningGuns(i) => vec![("Layers", Extra::Mask(&mut self.lightning_guns[i].Layers))],
            Slot::RayGuns(i) => vec![("Layers", Extra::Mask(&mut self.ray_guns[i].Layers))],
            Slot::Thrusterbeds(i) => vec![("LayersToBurn", Extra::Mask(&mut self.thrusterbeds[i].LayersToBurn))],
            Slot::Displays(i) => vec![("Value", Extra::F(&mut self.displays[i].Value))],
            Slot::Explosives(i) => vec![("Tag", Extra::F(&mut self.explosives[i].Tag))],
            Slot::Transmitters(i) => vec![("Channel", Extra::F(&mut self.transmitters[i].Channel))],
            Slot::KeyTriggers(i) => {
                let k = &mut self.key_triggers[i];
                vec![("KeyCode", Extra::F(&mut k.KeyCode)), ("Description", Extra::F(&mut k.Description))]
            }
            Slot::Syringes(i) => vec![("Fingerprint", Extra::Floats(&mut self.syringes[i].Fingerprint))],
            Slot::SentryTurrets(i) => {
                let s = &mut self.sentry_turrets[i];
                vec![("State", Extra::Sentry(&mut s.state)), ("disabled", Extra::F(&mut s.disabled)), ("timeStartedSearching", Extra::F(&mut s.timeStartedSearching))]
            }
            Slot::ShatterGenerators(i) => vec![("ConnectToRange", Extra::F(&mut self.shatter_generators[i].g.ConnectToRange))],
            Slot::Gores(l) => vec![("HadJointWhenSerialised", Extra::F(&mut self.limbs[l].gore.HadJointWhenSerialised))],
            Slot::Mutes(i) => vec![("Muted", Extra::F(&mut self.mutes[i]))],
            Slot::SerialisationHelpers(i) => vec![("IsPlaying", Extra::F(&mut self.serialisation_helpers[i].IsPlaying))],
            Slot::RandomCarTextures(i) => vec![("chosenIndex", Extra::F(&mut self.random_car_textures[i].chosenIndex))],
            Slot::Invisibles(i) => vec![("Invisible", Extra::F(&mut self.invisibles[i].Invisible))],
            Slot::EditableTints(i) => vec![("Tint", Extra::F(&mut self.editable_tints[i]))],
            Slot::ParticleMachines(i) => {
                // Activated is the Behaviour's flag (`particle_machines`), the three emit toggles are the menu's.
                let Some(&(_, Behaviour::ParticleMachine(j))) = self.behaviour_of.get(&id) else { unimplemented!("mod-only: ParticleMachineBehaviour without its behaviour") };
                let e = &mut self.particle_machine_emit[i];
                let [smoke, fire, electricity] = e;
                vec![("Activated", Extra::F(&mut self.particle_machines[j])), ("EmitSmoke", Extra::F(smoke)), ("EmitFire", Extra::F(fire)), ("EmitElectricity", Extra::F(electricity))]
            }
            _ => Vec::new(),
        }
    }
}
