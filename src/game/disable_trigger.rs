//! DisableWhenControlTrigger (the tutorial hints): hides its object once its control is released or its PlayerPrefs key exists,
//! then shows the next step. PlayerPrefs are the stored registry values (`player_prefs::store`) plus the keys set in this
//! run, which stay in memory (the registry is only read).
#![allow(non_snake_case)]

use super::player_prefs::{value_name, PlayerPrefsStore};
use super::Game;

#[derive(Clone, Debug)]
pub struct DisableWhenControlTrigger {
    /// The component id.
    pub id: i32,
    /// The GameObject's name (`base.name`).
    pub name: String,
    pub Control: String,
    pub IdentityOverride: String,
    /// The NextStep component's id.
    pub NextStep: Option<i32>,
    pub NeverSkip: bool,
}

impl DisableWhenControlTrigger {
    /// DisableWhenControlTrigger.get_Identity: the object's name unless IdentityOverride is blank.
    pub fn identity(&self) -> &str {
        if self.IdentityOverride.trim().is_empty() { &self.name } else { &self.IdentityOverride }
    }
}

impl Game {
    /// PlayerPrefs.HasKey: a key set in this run, else a stored value.
    pub fn player_prefs_has_key(&self, key: &str) -> bool {
        self.player_prefs_set.contains_key(&value_name(key)) || super::player_prefs::store().has_key(key)
    }

    /// PlayerPrefs.SetInt
    pub fn player_prefs_set_int(&mut self, key: &str, value: i32) {
        self.player_prefs_set.insert(value_name(key), value);
    }

    /// DisableWhenControlTrigger.Update of `Game::disable_triggers[i]`.
    pub fn disable_trigger_update(&mut self, i: usize) {
        let t = self.disable_triggers[i].clone();
        let identity = t.identity().to_owned();
        if self.input.up(&t.Control) || (self.player_prefs_has_key(&identity) && !t.NeverSkip) {
            self.player_prefs_set_int(&identity, 0);
            self.with_component_instance(t.id, |g, inst, o| g.set_active_scripts(inst, o, false));
            if let Some(next) = t.NextStep {
                self.with_component_instance(next, |g, inst, o| g.set_active_scripts(inst, o, true));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_falls_back_to_the_name() {
        let mut t = DisableWhenControlTrigger { id: 1, name: "Step1".into(), Control: "drag".into(), IdentityOverride: " ".into(), NextStep: None, NeverSkip: false };
        assert_eq!(t.identity(), "Step1");
        t.IdentityOverride = "Move".into();
        assert_eq!(t.identity(), "Move");
    }
}
