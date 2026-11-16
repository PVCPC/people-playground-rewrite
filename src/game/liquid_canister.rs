//! LiquidCanisterBehaviour: a BloodContainer of Capacity that does not overflow, and floats less the fuller it is. Its Text
//! label is a world text (world_text.rs), rewritten by UpdateText on the timer and on each liquid entering or leaving.
#![allow(non_snake_case)]

use super::container::{BloodContainer, Owner};
use super::{script, Game};
use crate::game_assets::{Curve, CurveCache};

script! {
    LiquidCanister {
        Capacity: f32 = 70.0,
        TextUpdateFrequency: f32 = 1.0,
    }
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// phys: GetComponent<PhysicalBehaviour>() in Awake.
        physical: usize = 0,
        /// BuoyancyResponseCurve and its native evaluation cache.
        BuoyancyResponseCurve: Option<Curve> = None,
        cache: CurveCache = CurveCache::default(),
        /// Text (world_text.rs).
        Text: Option<usize> = None,
        /// The private t: the time since UpdateText.
        t: f32 = 0.0,
    }
}

impl Game {
    // LiquidCanisterBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>(), the `physical` link.
    /// LiquidCanisterBehaviour.UpdateText (also Start, OnLiquidEnter and OnLiquidExit)
    pub fn liquid_canister_update_text(&mut self, c: usize) {
        let o = Owner::LiquidCanister(c);
        self.liquid_canisters[c].t = 0.0;
        // Mathf.CeilToInt: (int)Math.Ceiling((double)f)
        let b = ((self.bc(o).TotalLiquidAmount * (5.0f32 / 14.0f32)) as f64).ceil() as i32;
        let num = ((self.liquid_canisters[c].Capacity * (5.0f32 / 14.0f32)) as f64).ceil() as i32;
        let text = format!("{}L / {num}L", b.min(num));
        let keys = self.bc(o).LiquidDistribution.keys();
        let shown = match keys.len() {
            0 => "<i>(empty)</i>".to_string(),
            1 => format!("<i>{}</i>\n{text}", super::utils::escape_rich_text(&super::circulation::liquid_display_name(keys[0]).replace('\n', " "))),
            _ => format!("<i>Mixture</i>\n{text}"),
        };
        let field = self.liquid_canisters[c].Text;
        self.text_write(field, &shown);
    }

    /// LiquidCanisterBehaviour.Update
    pub fn liquid_canister_update(&mut self, c: usize) {
        let o = Owner::LiquidCanister(c);
        self.blood_container_update(o);
        self.liquid_canisters[c].t += self.time.deltaTime;
        let x = self.scaled_liquid_amount(o);
        let s = &mut self.liquid_canisters[c];
        let v = s.BuoyancyResponseCurve.as_ref().expect("LiquidCanisterBehaviour.BuoyancyResponseCurve").evaluate(&mut s.cache, x);
        let p = s.physical;
        self.physicals[p].BuoyancyModifier = 1.0 - v;
        if self.liquid_canisters[c].t > 1.0 / self.liquid_canisters[c].TextUpdateFrequency {
            self.liquid_canister_update_text(c);
        }
    }
}
