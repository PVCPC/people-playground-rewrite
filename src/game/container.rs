//! BloodContainer: the liquid store of CirculationBehaviour and FlaskBehaviour/GasTankBehaviour, with its virtual
//! members dispatched on the subclass that owns it.
#![allow(non_snake_case)]

use super::circulation::{change_red_to_orange, liquid_color, mix_instructions, RefFloat, LIQUIDS};
use super::{d, mathf, script, CsDict, Game};

script! {
    BloodContainer {
        TotalLiquidAmount: f32 = 0.0,
        isComputedColourCached: bool = false,
        cachedComputedColour: [f32; 4] = [0.0, 0.0, 0.0, 0.0],
        MeasuredPressure: f32 = 0.0,
        shouldDeleteEmptyEntries: bool = false,
        timer: f32 = 0.0,
    }
    links {
        LiquidDistribution: CsDict<usize, RefFloat> = CsDict::default(),
        /// SerialisableDistributions (LiquidID, Amount): what OnBeforeSerialise stores and OnAfterDeserialise reads.
        SerialisableDistributions: Vec<(String, f32)> = Vec::new(),
    }
}

/// BloodContainer.liquidBuffer's length (its static setup: new Liquid[256]).
pub(crate) const LIQUID_BUFFER: usize = 256;

/// A BloodContainer reference: the subclass component that is it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// CirculationBehaviour: the limb index.
    Circulation(usize),
    /// FlaskBehaviour or GasTankBehaviour: the `Game::flasks` index.
    Flask(usize),
    /// SyringeBehaviour or a subclass: the `Game::syringes` index.
    Syringe(usize),
    /// ValveBehaviour: the `Game::valves` index (valve.rs).
    Valve(usize),
    /// LiquidDuplicatorBehaviour: the `Game::liquid_duplicators` index (liquid_duplicator.rs).
    LiquidDuplicator(usize),
    /// LiquidPumpBehaviour: the `Game::liquid_pumps` index (liquid_pump.rs).
    LiquidPump(usize),
    /// CentrifugeBehaviour: the `Game::centrifuges` index (centrifuge.rs).
    Centrifuge(usize),
    /// LiquidentifierBehaviour: the `Game::liquidentifiers` index (liquidentifier.rs).
    Liquidentifier(usize),
    /// LiquidCanisterBehaviour: the `Game::liquid_canisters` index (liquid_canister.rs).
    LiquidCanister(usize),
    /// LiquidFilterBehaviour: the `Game::liquid_filters` index (liquid_filter.rs).
    LiquidFilter(usize),
    /// BloodTankBehaviour: the `Game::blood_tanks` index (blood_tank.rs).
    BloodTank(usize),
    /// LiquidOutletBehaviour: the `Game::liquid_outlets` index (liquid_outlet.rs).
    LiquidOutlet(usize),
    /// ReactorConduitBehaviour: the `Game::reactor_conduits` index (reactor.rs).
    ReactorConduit(usize),
}

impl Game {
    /// The PhysicalBehaviour on the container's GameObject (its transform).
    pub fn container_physical(&self, o: Owner) -> usize {
        match o {
            Owner::Circulation(l) => self.limbs[l].PhysicalBehaviour,
            Owner::Flask(f) => self.flasks[f].Phys.unwrap_or_else(|| unimplemented!("mod-only: BloodContainer.transform of a flask without a PhysicalBehaviour")),
            Owner::Syringe(s) => self.syringes[s].Phys,
            Owner::Valve(v) => self.valves[v].physical,
            Owner::BloodTank(t) => self.blood_tanks[t].physical,
            Owner::LiquidCanister(c) => self.liquid_canisters[c].physical,
            Owner::LiquidFilter(f) => self.liquid_filters[f].physical,
            Owner::Liquidentifier(l) => self.liquidentifiers[l].physical,
            Owner::Centrifuge(c) => self.centrifuges[c].physical,
            Owner::LiquidPump(l) => self.liquid_pumps[l].physical,
            Owner::LiquidDuplicator(l) => self.liquid_duplicators[l].physical,
            Owner::LiquidOutlet(l) => self.liquid_outlets[l].physical,
            Owner::ReactorConduit(c) => self.reactor_conduits[c].physical,
        }
    }

    pub fn bc(&self, o: Owner) -> &BloodContainer {
        match o {
            Owner::Circulation(l) => &self.circulations[l].base,
            Owner::Flask(f) => &self.flasks[f].base,
            Owner::Syringe(s) => &self.syringes[s].base,
            Owner::Valve(v) => &self.valves[v].base,
            Owner::BloodTank(t) => &self.blood_tanks[t].base,
            Owner::LiquidCanister(c) => &self.liquid_canisters[c].base,
            Owner::LiquidFilter(f) => &self.liquid_filters[f].base,
            Owner::Liquidentifier(l) => &self.liquidentifiers[l].base,
            Owner::Centrifuge(c) => &self.centrifuges[c].base,
            Owner::LiquidPump(l) => &self.liquid_pumps[l].base,
            Owner::LiquidDuplicator(l) => &self.liquid_duplicators[l].base,
            Owner::LiquidOutlet(l) => &self.liquid_outlets[l].base,
            Owner::ReactorConduit(c) => &self.reactor_conduits[c].base,
        }
    }

    pub fn bc_mut(&mut self, o: Owner) -> &mut BloodContainer {
        match o {
            Owner::Circulation(l) => &mut self.circulations[l].base,
            Owner::Flask(f) => &mut self.flasks[f].base,
            Owner::Syringe(s) => &mut self.syringes[s].base,
            Owner::Valve(v) => &mut self.valves[v].base,
            Owner::BloodTank(t) => &mut self.blood_tanks[t].base,
            Owner::LiquidCanister(c) => &mut self.liquid_canisters[c].base,
            Owner::LiquidFilter(f) => &mut self.liquid_filters[f].base,
            Owner::Liquidentifier(l) => &mut self.liquidentifiers[l].base,
            Owner::Centrifuge(c) => &mut self.centrifuges[c].base,
            Owner::LiquidPump(l) => &mut self.liquid_pumps[l].base,
            Owner::LiquidDuplicator(l) => &mut self.liquid_duplicators[l].base,
            Owner::LiquidOutlet(l) => &mut self.liquid_outlets[l].base,
            Owner::ReactorConduit(c) => &mut self.reactor_conduits[c].base,
        }
    }

    /// Behaviour.enabled. No flask or syringe code disables one.
    fn container_enabled(&self, o: Owner) -> bool {
        match o {
            Owner::Circulation(l) => self.circulations[l].enabled,
            Owner::Valve(v) => self.valves[v].enabled,
            Owner::BloodTank(t) => self.blood_tanks[t].enabled,
            Owner::LiquidCanister(c) => self.liquid_canisters[c].enabled,
            Owner::LiquidFilter(f) => self.liquid_filters[f].enabled,
            Owner::Liquidentifier(l) => self.liquidentifiers[l].enabled,
            Owner::Centrifuge(c) => self.centrifuges[c].enabled,
            Owner::LiquidPump(l) => self.liquid_pumps[l].enabled,
            Owner::LiquidDuplicator(l) => self.liquid_duplicators[l].enabled,
            Owner::LiquidOutlet(l) => self.liquid_outlets[l].enabled,
            Owner::ReactorConduit(c) => self.reactor_conduits[c].enabled,
            Owner::Flask(_) | Owner::Syringe(_) => true,
        }
    }

    /// BloodContainer.Limits: CirculationBehaviour.Limits (0, 1), FlaskBehaviour.Limits
    /// (0, Capacity), GasTankBehaviour's (0, 20), SyringeBehaviour.Limits FlaskBehaviour's (0, Capacity), GasTankBehaviour's (0, 20), SyringeBehaviour's (0, 1.4),
    /// ValveBehaviour's (0, 0.5).
    pub fn container_limits(&self, o: Owner) -> [f32; 2] {
        match o {
            Owner::Circulation(_) => [0.0, 1.0],
            Owner::Flask(f) => [0.0, self.flasks[f].limit],
            Owner::Syringe(_) => [0.0, 1.4],
            Owner::Valve(_) => self.valve_limits(),
            // LiquidCanisterBehaviour.Limits
            Owner::LiquidCanister(c) => [0.0, self.liquid_canisters[c].Capacity],
            // LiquidFilterBehaviour.Limits
            Owner::LiquidFilter(_) => super::liquid_filter::liquid_filter_limits(),
            // LiquidentifierBehaviour.Limits
            Owner::Liquidentifier(_) => [0.0, 7.0],
            // CentrifugeBehaviour.Limits
            Owner::Centrifuge(_) => [0.0, 7.0],
            // LiquidPumpBehaviour.Limits
            Owner::LiquidPump(_) => [0.0, 1.0],
            // LiquidDuplicatorBehaviour.Limits
            Owner::LiquidDuplicator(l) => [0.0, self.liquid_duplicators[l].Capacity],
            // BloodTankBehaviour.Limits
            Owner::BloodTank(_) => [0.0, 14.0],
            // LiquidOutletBehaviour.Limits
            Owner::LiquidOutlet(_) => [0.0, 2.8],
            // ReactorConduitBehaviour.Limits
            Owner::ReactorConduit(_) => [0.0, 1.0],
        }
    }

    /// BloodContainer.AllowPressureTransfer: a ValveBehaviour's is ValvePosition > 0.5; every other subclass rewritten is true.
    pub(crate) fn container_allows_pressure(&self, o: Owner) -> bool {
        match o {
            Owner::Valve(v) => self.valves[v].ValvePosition > 0.5,
            _ => true,
        }
    }

    /// BloodContainer.AllowsOverflow, FlaskBehaviour.AllowsOverflow: BloodContainer's true, overridden to false except in LiquidOutletBehaviour (true) and those that
    /// keep the base: CirculationBehaviour and LiquidDuplicatorBehaviour.
    pub fn allows_overflow(&self, o: Owner) -> bool {
        matches!(o, Owner::Circulation(_) | Owner::LiquidOutlet(_) | Owner::LiquidDuplicator(_))
    }

    /// BloodContainer.get_LowerLimit: Limits.x.
    pub(crate) fn lower_limit(&self, o: Owner) -> f32 {
        self.container_limits(o)[0]
    }

    /// BloodContainer.IsFull: ScaledLiquidAmount >= percentage (the default is 0.999).
    pub fn is_full(&self, o: Owner, percentage: f32) -> bool {
        self.scaled_liquid_amount(o) >= percentage
    }

    /// BloodContainer.GetPercentageOf: 0 when the container allows overflow, else GetAmount(liquid) / UpperLimit.
    #[allow(dead_code)]
    pub fn get_percentage_of(&self, o: Owner, liquid: usize) -> f32 {
        if self.allows_overflow(o) {
            return 0.0;
        }
        self.get_amount(o, liquid) / self.upper_limit(o)
    }

    /// BloodContainer.ForceRecalculateTotalLiquidAmount: the total is re-summed from the raw amounts, in registry order.
    #[allow(dead_code)]
    pub fn force_recalculate_total_liquid_amount(&mut self, o: Owner) {
        self.bc_mut(o).TotalLiquidAmount = 0.0;
        for item in 0..LIQUIDS.len() {
            if let Some(r) = self.bc(o).LiquidDistribution.get(item).copied() {
                self.bc_mut(o).TotalLiquidAmount += r.raw;
            }
        }
    }

    /// BloodContainer.get_BloodAmount (obsolete): TotalLiquidAmount.
    pub fn blood_amount(&self, o: Owner) -> f32 {
        self.bc(o).TotalLiquidAmount
    }

    /// BloodContainer.set_BloodAmount (obsolete): ForceSetAllLiquid.
    pub fn set_blood_amount(&mut self, o: Owner, value: f32) {
        self.force_set_all_liquid(o, value);
    }

    /// BloodContainer.IsComputerColourCached
    pub fn is_computer_colour_cached(&self, o: Owner) -> bool {
        self.bc(o).isComputedColourCached
    }

    /// BloodContainer.ForceSetAllLiquid: every entry is scaled so the total becomes `amount` (clamped to the limits);
    /// nothing happens while the container is empty.
    pub fn force_set_all_liquid(&mut self, o: Owner, mut amount: f32) {
        if mathf::approximately(self.bc(o).TotalLiquidAmount, 0.0) {
            return;
        }
        amount = mathf::clamp(amount, self.lower_limit(o), self.upper_limit(o));
        let num = amount / self.bc(o).TotalLiquidAmount;
        let c = self.bc_mut(o);
        c.TotalLiquidAmount *= num;
        for (_, r) in c.LiquidDistribution.iter_mut() {
            let v = r.raw * num;
            r.set_raw(v);
        }
        c.isComputedColourCached = false;
        c.shouldDeleteEmptyEntries = true;
    }

    /// BloodContainer.UpperLimit
    pub(crate) fn upper_limit(&self, o: Owner) -> f32 {
        if !self.allows_overflow(o) { self.container_limits(o)[1] } else { f32::MAX }
    }

    /// BloodContainer.ScaledLiquidAmount
    pub fn scaled_liquid_amount(&self, o: Owner) -> f32 {
        let [x, y] = self.container_limits(o);
        mathf::map_range(x, y, 0.0, 1.0, self.bc(o).TotalLiquidAmount)
    }

    /// BloodContainer.GetAmount
    pub fn get_amount(&self, o: Owner, liquid: usize) -> f32 {
        self.bc(o).LiquidDistribution.get(liquid).map_or(0.0, |r| r.raw)
    }

    /// InfiniteLiquidSourceBehaviour.Update, InfiniteLiquidSourceBehaviour.Awake (the container it found is `infinite_liquids`): AddLiquid(Liquid.GetLiquid(LiquidID), deltaTime * 10);
    /// a LiquidID that is empty or unknown only logs a warning.
    pub fn infinite_liquid_update(&mut self, i: usize) {
        let (id, owner) = self.infinite_liquids[i].clone();
        if id.trim().is_empty() || !super::circulation::liquid_has_id(&id) {
            return;
        }
        let liquid = super::circulation::liquid_get(&id).expect("Liquid.HasID said it is registered");
        self.add_liquid(owner, liquid, (super::d(self.time.deltaTime) * 10.0f32).f());
    }

    /// BloodContainer.AddLiquid
    pub fn add_liquid(&mut self, o: Owner, liquid: usize, amount: f32) -> f32 {
        if amount <= mathf::EPSILON {
            return 0.0;
        }
        let (lower, upper) = (self.lower_limit(o), self.upper_limit(o));
        let c = self.bc_mut(o);
        let num = mathf::clamp(c.TotalLiquidAmount + amount, lower, upper) - c.TotalLiquidAmount;
        if num.is_nan() || num <= mathf::EPSILON {
            return 0.0;
        }
        c.isComputedColourCached = false;
        c.TotalLiquidAmount += num;
        if let Some(r) = c.LiquidDistribution.get_mut(liquid) {
            let v = r.raw + num;
            r.set_raw(v);
        } else {
            c.LiquidDistribution.insert(liquid, RefFloat::new(num));
            self.on_liquid_enter(o, liquid);
        }
        num
    }

    /// AddLiquid(null, amount) of a BloodContainer: the totals change, then the null key throws in LiquidDistribution's TryGetValue.
    pub fn add_null_liquid(&mut self, o: Owner, amount: f32) -> f32 {
        if amount <= mathf::EPSILON {
            return 0.0;
        }
        let (lower, upper) = (self.lower_limit(o), self.upper_limit(o));
        let c = self.bc_mut(o);
        let num = mathf::clamp(c.TotalLiquidAmount + amount, lower, upper) - c.TotalLiquidAmount;
        if num.is_nan() || num <= mathf::EPSILON {
            return 0.0;
        }
        c.isComputedColourCached = false;
        c.TotalLiquidAmount += num;
        super::menu::throw("ArgumentNullException: Value cannot be null.")
    }

    /// BloodContainer.RemoveLiquid
    pub fn remove_liquid(&mut self, o: Owner, liquid: usize, amount: f32) -> f32 {
        if amount <= mathf::EPSILON {
            return 0.0;
        }
        let (lower, upper) = (self.lower_limit(o), self.upper_limit(o));
        let c = self.bc_mut(o);
        let mut num = mathf::clamp(c.TotalLiquidAmount - amount, lower, upper) - c.TotalLiquidAmount;
        if num.is_nan() || num >= -1e-45 {
            return 0.0;
        }
        if let Some(r) = c.LiquidDistribution.get_mut(liquid) {
            if r.raw + num < 0.0 {
                num = 0.0 - r.raw;
            }
            c.TotalLiquidAmount += num;
            let v = r.raw + num;
            r.set_raw(v);
        }
        c.isComputedColourCached = false;
        c.shouldDeleteEmptyEntries = true;
        num
    }

    /// BloodContainer.Drain
    pub fn drain(&mut self, o: Owner, amount: f32) {
        let count = self.bc(o).LiquidDistribution.len();
        if amount <= mathf::EPSILON || count == 0 {
            return;
        }
        let amount2 = amount / count as f32;
        for liquid in self.bc(o).LiquidDistribution.keys() {
            self.remove_liquid(o, liquid, amount2);
        }
        self.bc_mut(o).shouldDeleteEmptyEntries = true;
    }

    /// BloodContainer.DeleteEmptyLiquidEntries
    pub fn delete_empty_liquid_entries(&mut self, o: Owner) {
        self.bc_mut(o).shouldDeleteEmptyEntries = false;
        // Liquid.LiquidSet: the registry order.
        for item in 0..LIQUIDS.len() {
            let Some(r) = self.bc(o).LiquidDistribution.get(item).copied() else { continue };
            if r.raw <= 0.02 && (r.raw <= mathf::EPSILON || r.has_decreased(0.0001) || !r.has_changed()) {
                self.bc_mut(o).TotalLiquidAmount -= r.raw;
                self.on_liquid_exit(o, item);
                self.bc_mut(o).LiquidDistribution.remove(item);
            }
            // The removed RefFloat is still updated; it is unreachable afterwards.
            if let Some(r) = self.bc_mut(o).LiquidDistribution.get_mut(item) {
                r.Previous = r.raw;
            }
        }
    }

    /// BloodContainer.TransferTo(Liquid, float, BloodContainer)
    pub fn transfer_liquid_to(&mut self, o: Owner, liquid: usize, amount: f32, target: Owner) -> f32 {
        if !self.container_enabled(o) || !self.container_enabled(target) || amount <= mathf::EPSILON {
            return 0.0;
        }
        let num = self.remove_liquid(o, liquid, amount).abs();
        if num > mathf::EPSILON {
            self.add_liquid(target, liquid, num);
        }
        num
    }

    /// BloodContainer.TransferTo(float, BloodContainer, proportional = true)
    pub fn transfer_to(&mut self, o: Owner, amount: f32, target: Owner) -> f32 {
        if !self.container_enabled(o) || !self.container_enabled(target) || amount <= mathf::EPSILON {
            return 0.0;
        }
        let total = self.bc(o).TotalLiquidAmount;
        let mut num = 0.0;
        // The enumerator throws if the dictionary changes; RemoveLiquid only changes values.
        for key in self.bc(o).LiquidDistribution.keys() {
            let raw = self.bc(o).LiquidDistribution.get(key).unwrap().raw;
            // Evaluated in double, rounded at the call.
            let amount2 = (d(amount) * (d(raw) / total)).f();
            num += self.transfer_liquid_to(o, key, amount2, target);
        }
        num
    }

    /// BloodContainer.ForceCalculateComputedColor: the cache is dropped, then GetComputedColor(fallback) recomputes.
    pub fn force_calculate_computed_color(&mut self, o: Owner, fallback: [f32; 4]) -> [f32; 4] {
        self.bc_mut(o).isComputedColourCached = false;
        self.get_computed_color(o, fallback)
    }

    /// BloodContainer.GetComputedColor(Color)
    pub fn get_computed_color(&mut self, o: Owner, fallback: [f32; 4]) -> [f32; 4] {
        let goreless = self.prefs.GorelessMode;
        let c = self.bc_mut(o);
        if c.isComputedColourCached {
            return c.cachedComputedColour;
        }
        if c.LiquidDistribution.len() == 0 || mathf::approximately(0.0, c.TotalLiquidAmount) {
            return fallback;
        }
        let mut v = [0.0f32; 4];
        for (liquid, amount) in c.LiquidDistribution.iter() {
            let num = amount.raw / c.TotalLiquidAmount;
            let color = liquid_color(*liquid);
            for i in 0..4 {
                v[i] += color[i] * num;
            }
        }
        v[3] = mathf::clamp(v[3], 0.1, 1.0);
        for x in &mut v[..3] {
            *x = mathf::clamp(*x, 0.0, 10.0);
        }
        if goreless {
            v = change_red_to_orange(v);
        }
        c.cachedComputedColour = v;
        c.isComputedColourCached = true;
        v
    }

    /// BloodContainer.Pressure: a syringe's PressureMode (Push 0, Pull 1), None (2) in every other subclass rewritten.
    pub(crate) fn container_pressure(&self, o: Owner) -> i32 {
        match o {
            Owner::Syringe(s) => self.syringes[s].PressureMode,
            Owner::LiquidDuplicator(l) => self.liquid_duplicator_pressure(l),
            Owner::LiquidPump(l) => self.liquid_pumps[l].Mode,
            // LiquidOutletBehaviour.Pressure: Pull.
            Owner::LiquidOutlet(_) => 1,
            // BloodTankBehaviour.Pressure: Push for Push, Pull for Pull and Drain, None for Idle.
            Owner::BloodTank(t) => match self.blood_tanks[t].Mode {
                0 => 0,
                1 | 3 => 1,
                _ => 2,
            },
            _ => 2,
        }
    }

    /// BloodContainer.CopyTo(amount, target, proportional): each liquid, its share of the amount (by raw amount when
    /// proportional), added to the target without taking it from this container.
    pub fn copy_to(&mut self, o: Owner, amount: f32, target: Owner, proportional: bool) {
        if !self.container_enabled(o) || !self.container_enabled(target) || amount <= mathf::EPSILON {
            return;
        }
        let total = self.bc(o).TotalLiquidAmount;
        for key in self.bc(o).LiquidDistribution.keys() {
            let raw = self.bc(o).LiquidDistribution.get(key).unwrap().raw;
            let num = if proportional { raw / total } else { 1.0 };
            // CopyTo(liquid, amount * num, target): target.AddLiquid when both are enabled and the amount is above epsilon.
            let share = amount * num;
            if !(share <= mathf::EPSILON) {
                self.add_liquid(target, key, share);
            }
        }
    }

    /// BloodContainer.Update.
    pub fn blood_container_update(&mut self, o: Owner) {
        let dt = self.time.deltaTime;
        let scaled = self.scaled_liquid_amount(o);
        let pressure = self.container_pressure(o);
        let allow_pressure = self.container_allows_pressure(o);
        let c = self.bc_mut(o);
        c.timer += dt;
        if allow_pressure {
            // BloodContainer.CalculatePressureFromContents: ScaledLiquidAmount.
            c.MeasuredPressure = mathf::lerp(c.MeasuredPressure, scaled, mathf::lerp_factor_dt(0.6, dt));
            match pressure {
                0 => c.MeasuredPressure = mathf::lerp(c.MeasuredPressure, 4096.0, mathf::lerp_factor_dt(0.95, dt)),
                1 => c.MeasuredPressure = mathf::lerp(c.MeasuredPressure, -4096.0, mathf::lerp_factor_dt(0.95, dt)),
                _ => {}
            }
        } else {
            // The pressure is not lerped: Push 4096, Pull -4096, else CalculatePressureFromContents.
            c.MeasuredPressure = match pressure {
                0 => 4096.0,
                1 => -4096.0,
                _ => scaled,
            };
        }
        if c.timer > 1.0 {
            c.timer = 0.0;
            self.delete_empty_liquid_entries(o);
            // The keys go into the static liquidBuffer first: past its LIQUID_BUFFER entries the copy throws, before any OnUpdate.
            let liquids = self.bc(o).LiquidDistribution.keys();
            if liquids.len() > LIQUID_BUFFER {
                crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
            }
            for liquid in liquids {
                self.liquid_on_update(liquid, o);
            }
            // Liquid.HasLiquid: every instruction's liquids come from the registry (`liquid` panics on a missing one), so it always holds.
            for m in mix_instructions() {
                let (sources, target, rate, outside_circulations) = (m.SourceLiquids, m.TargetLiquid, m.RatePerSecond, m.only_outside_circulations);
                // ContainerFilter: !(c is CirculationBehaviour)
                if outside_circulations && matches!(o, Owner::Circulation(_)) {
                    continue;
                }
                // Utils.LiquidMixProcess(container, Liquid[], result, delta)
                if sources.iter().any(|&s| self.get_amount(o, s) < rate) {
                    continue;
                }
                for &s in &sources {
                    self.remove_liquid(o, s, rate);
                }
                self.add_liquid(o, target, rate * sources.len() as f32);
            }
        }
        // 1.28.45: foreach (Liquid key in LiquidDistribution.Keys) key.OnUpdateNew(this) (v45_new hook).
        if crate::assets::il::is_1_28_45() {
            for liquid in self.bc(o).LiquidDistribution.keys() {
                self.liquid_on_update_new(liquid, o);
            }
        }
        if self.bc(o).shouldDeleteEmptyEntries {
            self.delete_empty_liquid_entries(o);
        }
    }

    /// BloodContainer.OnLiquidEnter, CirculationBehaviour.OnLiquidEnter: OnDistributionChange (no subclass rewritten has a persistent listener), the liquid's
    /// OnEnterContainer, and CirculationBehaviour's OnEnterLimb.
    pub(crate) fn on_liquid_enter(&mut self, o: Owner, liquid: usize) {
        self.invoke_event(self.container_id(o), "OnDistributionChange");
        self.liquid_on_enter_container(liquid, o);
        if let Owner::Circulation(l) = o {
            self.liquid_on_enter_limb(liquid, l);
        }
        // LiquidCanisterBehaviour.OnLiquidEnter: base, then UpdateText.
        if let Owner::LiquidCanister(c) = o {
            self.liquid_canister_update_text(c);
        }
    }

    /// BloodContainer.ClearLiquid
    pub fn clear_liquid(&mut self, o: Owner) {
        for liquid in self.bc(o).LiquidDistribution.keys() {
            self.on_liquid_exit(o, liquid);
        }
        let c = self.bc_mut(o);
        c.LiquidDistribution = CsDict::default();
        c.TotalLiquidAmount = 0.0;
    }

    /// BloodContainer.OnLiquidExit
    pub(crate) fn on_liquid_exit(&mut self, o: Owner, liquid: usize) {
        self.invoke_event(self.container_id(o), "OnDistributionChange");
        self.liquid_on_exit_container(liquid, o);
        // LiquidCanisterBehaviour.OnLiquidExit: base, then UpdateText.
        if let Owner::LiquidCanister(c) = o {
            self.liquid_canister_update_text(c);
        }
    }

    // ---- Liquid's virtual members: GorseBlood's, syringe.rs's and chemistry.rs's ----

    fn liquid_unwritten(liquid: usize, hook: &str) -> ! {
        unimplemented!("mod-only: {}.{hook}", LIQUIDS[liquid].1)
    }

    /// ExoticLiquid.OnUpdate: container.StartCoroutine of a DelayCoroutine.
    pub fn exotic_liquid_on_update(&mut self, this: usize, c: Owner) {
        let r = self.random.value();
        if !(r > 0.8 / mathf::clamp(self.get_amount(c, this), 0.5, 2.0)) {
            return;
        }
        // PhysicalObjectsInWorldByTransform.TryGetValue(container.transform)
        let p = match c {
            Owner::Flask(f) => self.flasks[f].Phys,
            _ => Some(self.container_physical(c)),
        };
        let Some(p) = p.filter(|&p| self.physicals[p].in_world) else { return };
        let delay = self.random.value() * 0.4;
        let owner = self.container_id(c);
        self.start_wait(owner, delay, super::destroyable::Step::ExoticFlash(p));
        if self.random.value() as f64 > 0.8 {
            self.physicals[p].charge = self.physicals[p].Charge() + 0.5;
        }
        let t = self.random.value() * 5.0;
        self.physicals[p].Temperature -= t;
    }

    /// ExoticLiquid.OnUpdate's delayed action: phys.rigidbody.AddForce(Random.insideUnitCircle), then Instantiate
    /// Prefabs/Exotic flash at container.transform.TransformPoint(phys.GetRandomGridPoint()) + insideUnitCircle * 2.
    pub(super) fn exotic_flash(&mut self, p: usize) {
        let force = self.random.inside_unit_circle();
        let rb = self.physicals[p].rigidbody.clone();
        rb.add_force(&mut self.world, force);
        let local = self.random_grid_point(p);
        let point = self.transform_point(p, local);
        let u = self.random.inside_unit_circle();
        self.spawn_resource(crate::il!(str "Chemistry/ExoticLiquid/<>c__DisplayClass6_0::<OnUpdate>b__0" 0), [point[0] + u[0] * 2.0, point[1] + u[1] * 2.0, 0.0]);
    }

    /// ExoticLiquid.OnEnterContainer
    pub(crate) fn exotic_liquid_on_enter_container(&mut self) {
        super::stats::unlock_achievement("CHEMIST");
    }

    /// ExoticLiquid.OnEnterLimb
    pub(crate) fn exotic_liquid_on_enter_limb(&mut self, l: usize) {
        // (double)Random.value > 0.9
        if self.random.value() as f64 > 0.9 && self.nodes[l].IsConnectedToRoot {
            let pain = self.random.value() * 50.0;
            self.add_pain(self.limbs[l].Person, pain);
        }
        if self.random.value() as f64 > 0.9 && self.limbs[l].RoughClassification == 0 {
            let r = self.random.value();
            self.people[self.limbs[l].Person].Consciousness *= r;
        }
    }

    /// Liquid.OnEnterContainer
    fn liquid_on_enter_container(&mut self, liquid: usize, o: Owner) {
        match LIQUIDS[liquid].1 {
            // GorseBlood.OnEnterContainer: empty. CrabBlood (extras) is Blood.
            "GorseBlood" | "CrabBlood" => {}
            c if super::syringe::is_syringe_liquid(c) => self.syringe_liquid_on_enter_container(liquid, o),
            c if super::chemistry::is_chemistry_liquid(c) => self.chemistry_liquid_on_enter_container(liquid, o),
            _ => Self::liquid_unwritten(liquid, "OnEnterContainer"),
        }
    }

    /// Liquid.OnExitContainer
    fn liquid_on_exit_container(&mut self, liquid: usize, o: Owner) {
        match LIQUIDS[liquid].1 {
            // GorseBlood.OnExitContainer: empty.
            "GorseBlood" | "CrabBlood" => {}
            c if super::syringe::is_syringe_liquid(c) => self.syringe_liquid_on_exit_container(liquid, o),
            c if super::chemistry::is_chemistry_liquid(c) => self.chemistry_liquid_on_exit_container(liquid, o),
            _ => Self::liquid_unwritten(liquid, "OnExitContainer"),
        }
    }

    /// Liquid.OnEnterLimb
    pub(super) fn liquid_on_enter_limb(&mut self, liquid: usize, l: usize) {
        match LIQUIDS[liquid].1 {
            "GorseBlood" if self.limbs[l].SpeciesIdentity == "Gorse" => {}
            "GorseBlood" => self.acid_routine(liquid, l),
            "CrabBlood" => {}
            c if super::syringe::is_syringe_liquid(c) => self.syringe_liquid_on_enter_limb(liquid, l),
            c if super::chemistry::is_chemistry_liquid(c) => self.chemistry_liquid_on_enter_limb(liquid, l),
            _ => Self::liquid_unwritten(liquid, "OnEnterLimb"),
        }
    }

    /// Liquid.OnUpdate
    pub(crate) fn liquid_on_update(&mut self, liquid: usize, o: Owner) {
        match LIQUIDS[liquid].1 {
            // Blood.OnUpdate, inherited.
            "GorseBlood" | "CrabBlood" => self.blood_on_update(liquid, o),
            c if super::syringe::is_syringe_liquid(c) => self.syringe_liquid_on_update(liquid, o),
            c if super::chemistry::is_chemistry_liquid(c) => self.chemistry_liquid_on_update(liquid, o),
            _ => Self::liquid_unwritten(liquid, "OnUpdate"),
        }
    }
}
