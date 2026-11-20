//! The liquids of Chemistry.cs (Beverage M04, Coolant, Debug discoloration, Exotic, Inert, Tritium; 1.28.45's Nasumica,
//! Amaraevelum and Dulcamarus), Blood,
//! Nitroglycerine and Oil: their Liquid virtual members. Exotic's, Tritium's and Nitroglycerine's live with the rest of
//! their code (container.rs, glow.rs, explosion.rs).

use super::container::Owner;
use super::{mathf, Game};

pub fn is_chemistry_liquid(class: &str) -> bool {
    matches!(
        class,
        "BeverageM04" | "Blood" | "Coolant" | "DebugDiscolorationLiquid" | "ExoticLiquid" | "InertLiquid" | "Nitroglycerine" | "Oil" | "Tritium"
            | "Nasumica" | "Amaraevelum" | "Dulcamarus"
    )
}

impl Game {
    /// BeverageM04.OnEnterContainer, Blood.OnEnterContainer, Coolant.OnEnterContainer, DebugDiscolorationLiquid.OnEnterContainer, InertLiquid.OnEnterContainer, Oil.OnEnterContainer, Nasumica.OnEnterContainer, Amaraevelum.OnEnterContainer, Dulcamarus.OnEnterContainer: empty.
    pub(crate) fn chemistry_liquid_on_enter_container(&mut self, liquid: usize, o: Owner) {
        match super::circulation::LIQUIDS[liquid].1 {
            "ExoticLiquid" => self.exotic_liquid_on_enter_container(),
            "Tritium" => self.tritium_on_enter_container(liquid, o),
            "Nitroglycerine" => self.nitroglycerine_on_enter_container(liquid, o),
            _ => {}
        }
    }

    /// BeverageM04.OnExitContainer, Blood.OnExitContainer, Coolant.OnExitContainer, DebugDiscolorationLiquid.OnExitContainer, ExoticLiquid.OnExitContainer, InertLiquid.OnExitContainer, Oil.OnExitContainer, Nasumica.OnExitContainer, Amaraevelum.OnExitContainer, Dulcamarus.OnExitContainer: empty.
    pub(crate) fn chemistry_liquid_on_exit_container(&mut self, liquid: usize, o: Owner) {
        match super::circulation::LIQUIDS[liquid].1 {
            "Tritium" => self.tritium_on_exit_container(liquid, o),
            "Nitroglycerine" => self.nitroglycerine_on_exit_container(liquid, o),
            _ => {}
        }
    }

    /// BeverageM04.OnEnterLimb, Blood.OnEnterLimb, Coolant.OnEnterLimb, InertLiquid.OnEnterLimb, Nasumica.OnEnterLimb: empty.
    pub(crate) fn chemistry_liquid_on_enter_limb(&mut self, liquid: usize, l: usize) {
        match super::circulation::LIQUIDS[liquid].1 {
            // DebugDiscolorationLiquid.OnEnterLimb: limb.Color = Color.magenta.
            "DebugDiscolorationLiquid" => self.limbs[l].Color = [1.0, 0.0, 1.0, 1.0],
            "ExoticLiquid" => self.exotic_liquid_on_enter_limb(l),
            "Tritium" => self.tritium_on_enter_limb(l),
            // Nitroglycerine.OnEnterLimb: limb.Person.Wince(12f).
            "Nitroglycerine" => self.person_wince(self.limbs[l].Person, 12.0),
            "Oil" => self.oil_on_enter_limb(l),
            "Amaraevelum" => self.amaraevelum_on_enter_limb(l),
            "Dulcamarus" => self.dulcamarus_on_enter_limb(liquid, l),
            _ => {}
        }
    }

    /// Liquid.OnUpdateNew (1.28.45; BloodContainer.Update calls it each frame for every liquid it holds): the base is
    /// empty, Nasumica and Amaraevelum override it.
    pub(crate) fn liquid_on_update_new(&mut self, liquid: usize, o: Owner) {
        match super::circulation::LIQUIDS[liquid].1 {
            "Nasumica" => self.nasumica_on_update_new(liquid, o),
            "Amaraevelum" => self.amaraevelum_on_update_new(liquid, o),
            _ => {}
        }
    }

    /// Nasumica.OnUpdateNew: a zombie Gorse or Human limb is cured (some of the liquid used up); its heat and rot fade.
    /// An Android limb heats up.
    fn nasumica_on_update_new(&mut self, this: usize, o: Owner) {
        let Owner::Circulation(l) = o else { return };
        match self.limbs[l].SpeciesIdentity.clone().as_str() {
            "Gorse" | "Human" => {
                if self.limbs[l].IsZombie {
                    let amount = mathf::min(self.get_amount(o, this), 0.1);
                    self.remove_liquid(o, this, amount);
                    let r = self.random.value();
                    self.limb_wince(l, r);
                    self.limbs[l].IsZombie = false;
                }
                self.limbs[l].InternalTemperature *= mathf::pow(0.5, self.time.deltaTime);
                let rotten = self.skins[l].rottenProgress * mathf::pow(0.9, self.time.deltaTime);
                self.skins[l].set_rotten(rotten);
            }
            "Android" => {
                let p = self.limbs[l].PhysicalBehaviour;
                // (float)Random.Range(1, 5) * Time.deltaTime
                let r = self.random.range_int(1, 5) as f32;
                self.physicals[p].Temperature += r * self.time.deltaTime;
            }
            _ => {}
        }
    }

    /// Amaraevelum.OnEnterLimb: a connected Human limb's person gets 0.25 pain.
    fn amaraevelum_on_enter_limb(&mut self, l: usize) {
        if self.limbs[l].SpeciesIdentity == "Human" && self.nodes[l].IsConnectedToRoot {
            self.add_pain(self.limbs[l].Person, 0.25);
        }
    }

    /// Amaraevelum.OnUpdateNew: a Gorse or Human limb uses it up at 0.25/s, loses strength, and heals: wounds, acid,
    /// rot and bleeding fade, health climbs back. An Android limb gains charge.
    fn amaraevelum_on_update_new(&mut self, this: usize, o: Owner) {
        let Owner::Circulation(l) = o else { return };
        match self.limbs[l].SpeciesIdentity.clone().as_str() {
            "Gorse" | "Human" => {
                let dt = self.time.deltaTime;
                let amount = mathf::min(self.get_amount(o, this), dt * 0.25);
                self.remove_liquid(o, this, amount);
                self.limbs[l].BaseStrength *= mathf::pow(0.9, dt);
                let num = mathf::pow(0.2, dt);
                let skin = &mut self.skins[l];
                let acid = skin.acidProgress * num;
                skin.set_acid(acid);
                let rotten = skin.rottenProgress * num;
                skin.set_rotten(rotten);
                for p in skin.damagePoints.iter_mut() {
                    p[2] *= num;
                }
                // skinMaterialHandler.Sync(): the damage points to the material (rendering).
                let limb = &mut self.limbs[l];
                limb.Health = mathf::clamp(limb.Health + dt, 0.0, limb.InitialHealth);
                self.circulations[l].InternalBleedingIntensity *= num;
                self.circulations[l].BleedingRate *= num;
            }
            "Android" => {
                let p = self.limbs[l].PhysicalBehaviour;
                let r = self.random.range_int(1, 50) as f32;
                self.physicals[p].charge = self.physicals[p].Charge() + r * self.time.deltaTime;
            }
            _ => {}
        }
    }

    /// Dulcamarus.OnEnterLimb: a connected Human limb's person gets a DulcamarusBehaviour (once) and the liquid is used
    /// up; an Android limb ignores it; any other limb crushes its whole connected body (Traverse over the nodes).
    fn dulcamarus_on_enter_limb(&mut self, this: usize, l: usize) {
        match self.limbs[l].SpeciesIdentity.clone().as_str() {
            "Human" => {
                let person = self.limbs[l].Person;
                if self.nodes[l].IsConnectedToRoot && !self.dulcamarus_on(person) {
                    self.dulcamarus_add(person);
                    let o = Owner::Circulation(l);
                    let amount = self.get_amount(o, this);
                    self.remove_liquid(o, this, amount);
                }
                return;
            }
            "Android" => return,
            _ => {}
        }
        if self.physicals[self.limbs[l].PhysicalBehaviour].isDisintegrated {
            return;
        }
        // Traverse(hashSet, limb.NodeBehaviour): a HashSet with no removals enumerates in insertion order.
        let mut network = Vec::new();
        // Recursion depth first, in Connections order.
        fn traverse(g: &Game, network: &mut Vec<usize>, node: usize) {
            if network.contains(&node) {
                return;
            }
            network.push(node);
            for c in g.nodes[node].Connections.clone() {
                // A destroyed node's OptionalLimb is destroyed too: op_Implicit is false.
                let Some(c) = c else { continue };
                if !g.physicals[g.limbs[c].PhysicalBehaviour].isDisintegrated {
                    traverse(g, network, c);
                }
            }
        }
        traverse(self, &mut network, l);
        for n in network {
            self.limb_crush(n);
        }
    }

    /// InertLiquid.OnUpdate: Liquid's, empty.
    pub(super) fn chemistry_liquid_on_update(&mut self, liquid: usize, o: Owner) {
        match super::circulation::LIQUIDS[liquid].1 {
            "BeverageM04" => self.beverage_m04_on_update(o),
            "Blood" => self.blood_on_update(liquid, o),
            "Coolant" => self.coolant_on_update(o),
            // DebugDiscolorationLiquid is a TemporaryBodyLiquid that calls OnEnterLimb every update.
            "DebugDiscolorationLiquid" => self.temporary_body_liquid_on_update(liquid, o, 0.8, true),
            "ExoticLiquid" => self.exotic_liquid_on_update(liquid, o),
            "Tritium" => self.tritium_on_update(o),
            "Nitroglycerine" => self.nitroglycerine_on_update(liquid, o),
            // Oil.OnUpdate: OnEnterLimb every update.
            "Oil" => {
                if let Owner::Circulation(l) = o {
                    self.oil_on_enter_limb(l);
                }
            }
            _ => {}
        }
    }

    /// BeverageM04.OnUpdate: base.OnUpdate (Liquid's, empty), then a switch on the limb's species.
    pub(crate) fn beverage_m04_on_update(&mut self, o: Owner) {
        let Owner::Circulation(l) = o else { return };
        match self.limbs[l].SpeciesIdentity.clone().as_str() {
            "Human" => {
                if self.nodes[l].IsConnectedToRoot {
                    self.people[self.limbs[l].Person].AdrenalineLevel += 0.1;
                }
            }
            "Gorse" => {
                let acid = self.skins[l].acidProgress + 0.05;
                self.skins[l].set_acid(acid);
                if self.nodes[l].IsConnectedToRoot {
                    self.people[self.limbs[l].Person].Consciousness *= 0.8;
                }
            }
            "Android" => {
                let p = self.limbs[l].PhysicalBehaviour;
                self.physicals[p].charge = self.physicals[p].Charge() + self.random.value();
            }
            _ => {}
        }
    }

    /// Blood.OnUpdate: an Android's limb now and then gains charge.
    pub(crate) fn blood_on_update(&mut self, _liquid: usize, o: Owner) {
        let Owner::Circulation(l) = o else { return };
        if self.limbs[l].SpeciesIdentity == "Android" && self.random.value() > 0.995 {
            let p = self.limbs[l].PhysicalBehaviour;
            self.physicals[p].charge = self.physicals[p].Charge() + 5.0;
        }
    }

    /// Coolant.OnUpdate
    pub(crate) fn coolant_on_update(&mut self, o: Owner) {
        let Owner::Circulation(l) = o else { return };
        if self.limbs[l].SpeciesIdentity == "Android" {
            let limb = &mut self.limbs[l];
            limb.InternalTemperature = mathf::lerp(limb.InternalTemperature, limb.BodyTemperature, 0.5);
            return;
        }
        let p = self.limbs[l].PhysicalBehaviour;
        if self.physicals[p].Temperature < 40.0 {
            self.physicals[p].Temperature += self.random.value();
        }
        if self.random.value() > 0.9 {
            self.limbs[l].InternalTemperature += 200.0;
        }
        if self.limbs[l].RoughClassification == 0 {
            let person = self.limbs[l].Person;
            self.people[person].AngleOffset += self.random.range(-2.0, 2.0);
            if self.random.value() > 0.98 {
                self.people[person].BrainDamaged = true;
            }
        }
    }

    /// Oil.OnEnterLimb
    pub(crate) fn oil_on_enter_limb(&mut self, l: usize) {
        if self.limbs[l].SpeciesIdentity != "Android" {
            self.limbs[l].Numbness += 0.5;
            self.circulations[l].IsPump = false;
        }
    }
}
