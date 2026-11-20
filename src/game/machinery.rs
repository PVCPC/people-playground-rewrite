//! DamagableMachineryBehaviour: health, heat/fire/water damage and the broken state of machines.
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    Machinery {
        FirearmDamageMultiplier: f32 = 0.0,
        ExplosionDamageMultiplier: f32 = 0.0,
        StabDamageMultiplier: f32 = 0.0,
        BreakDamage: f32 = 0.0,
        ExplodesOnBreak: bool = false,
        Waterproof: bool = true,
        IndestructibilityCanBeToggled: bool = true,
        TemperatureThreshold: f32 = f32::INFINITY,
        FireDamageThreshold: f32 = 1.0,
        Health: f32 = 10.0,
        CanRepair: bool = true,
        Destroyed: bool = false,
        Indestructible: bool = false,
        HeatShielded: bool = false,
    }
    links {
        /// GetComponent<PhysicalBehaviour>()
        PhysicalBehaviour: usize = 0,
        initialHealth: f32 = 0.0,
        RandomOffset: [f32; 4] = [0.0; 4],
        t: f32 = 0.0,
        /// originalMaterial (the SpriteRenderer's sharedMaterial at Awake) and brokenMachineMaterial
        /// (Resources.Load<Material>("Materials/BrokenMachine")).
        originalMaterial: Option<crate::assets::db::ObjRef> = None,
        brokenMachineMaterial: Option<crate::assets::db::ObjRef> = None,
        /// propertyBlock: Start's copy of the SpriteRenderer's block (None before Start).
        propertyBlock: Option<super::props::PropBlock> = None,
        /// SparkPrefab (or the Resources fallback) and ExplosionPrefab are set.
        SparkPrefab: bool = false,
        ExplosionPrefab: bool = false,
        /// The serialized SparkPrefab's source prefab (None: the Resources one).
        SparkPrefabSource: Option<crate::assets::db::ObjRef> = None,
        /// ExplosionPrefab's source prefab.
        ExplosionPrefabSource: Option<crate::assets::db::ObjRef> = None,
        /// Explosion: the parameters ActOnDamage explodes with (ExplodesOnBreak).
        Explosion: super::explosion::ExplosionParameters = Default::default(),
        /// The live entries of BehavioursToToggle (class name and component id) and GameObjectsToToggle (the object's
        /// Transform id).
        BehavioursToToggle: Vec<(String, i32)> = Vec::new(),
        GameObjectsToToggle: Vec<i32> = Vec::new(),
        /// The component's instance id.
        id: i32 = 0,
        /// OnDamaged's and OnRepaired's runtime listeners (AddListener), in the order they were added.
        runtime_listeners: Vec<MachineryListener> = Vec::new(),
    }
}

/// A runtime listener on DamagableMachineryBehaviour.OnDamaged and OnRepaired.
#[derive(Clone, Copy, Debug)]
pub enum MachineryListener {
    /// FanBehaviour.Awake: UpdateActivated on both.
    FanUpdateActivated(usize),
}

impl Game {
    /// UnityEvent.Invoke on OnDamaged / OnRepaired: the persistent calls, then the runtime listeners.
    fn machinery_invoke(&mut self, m: usize, name: &str) {
        self.invoke_event(self.machinery[m].id, name);
        for l in self.machinery[m].runtime_listeners.clone() {
            match l {
                MachineryListener::FanUpdateActivated(f) => self.fan_behaviour_update_activated(f),
            }
        }
    }

    /// DamagableMachineryBehaviour.Awake (the spark prefab is a Resources load).
    pub fn machinery_awake(&mut self, db: Option<&crate::assets::db::AssetDb>, m: usize) {
        self.machinery[m].initialHealth = self.machinery[m].Health;
        let x = self.random.value() * 1000.0;
        let y = self.random.value() * 1000.0;
        self.machinery[m].RandomOffset = [x, y, 0.0, 0.0];
        let p = self.machinery[m].PhysicalBehaviour;
        self.machinery[m].originalMaterial = self.physicals[p].sprite_material;
        let db = db.or(self.assets.as_deref()).expect("DamagableMachineryBehaviour without the game's data");
        // Material is class 21.
        self.machinery[m].brokenMachineMaterial = db.resource("Materials/BrokenMachine", 21).unwrap_or_else(|e| panic!("Resources.Load(Materials/BrokenMachine): {e}"));
    }

    /// DamagableMachineryBehaviour.Start, with its four context menu buttons.
    pub fn machinery_start(&mut self, m: usize) {
        let p = self.machinery[m].PhysicalBehaviour;
        self.machinery[m].propertyBlock = Some(self.get_property_block(super::visual::SpriteAt::Physical(p)));
        if self.machinery[m].Destroyed {
            self.machinery_set_as_broken(m, false);
        }
        use super::menu::{Cond, Label, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.machinery[m].PhysicalBehaviour].menu;
        menu.push(B { condition: Cond::MachineryRepair(m), ..B::new("repairMachinery", "Repair", crate::il!(str "DamagableMachineryBehaviour::Start" 2), A::MachineryRepair(m)) });
        menu.push(B { condition: Cond::MachineryShielded(m, false), ..B::new("shieldFromHeat", crate::il!(str "DamagableMachineryBehaviour::Start" 4), crate::il!(str "DamagableMachineryBehaviour::Start" 5), A::MachineryShield(m, true)) });
        menu.push(B { condition: Cond::MachineryShielded(m, true), ..B::new("unshieldFromHeat", crate::il!(str "DamagableMachineryBehaviour::Start" 7), crate::il!(str "DamagableMachineryBehaviour::Start" 8), A::MachineryShield(m, false)) });
        menu.push(B { label: Label::MachineryIndestructible(m), condition: Cond::MachineryToggle(m), ..B::new("machineryIndestructible", "", crate::il!(str "DamagableMachineryBehaviour::Start" 10), A::MachineryToggle(m)) });
    }

    /// DamagableMachineryBehaviour.FixedUpdate
    pub fn machinery_fixed_update(&mut self, m: usize) {
        let mc = &self.machinery[m];
        if mc.Indestructible || mc.Destroyed {
            return;
        }
        let ph = &self.physicals[mc.PhysicalBehaviour];
        if !mc.HeatShielded && (ph.BurnProgress > mc.FireDamageThreshold || ph.Temperature > mc.TemperatureThreshold) {
            self.machinery[m].Health = 0.0;
            self.machinery_act_on_damage(m);
        }
        let mc = &self.machinery[m];
        if !mc.Waterproof && self.physicals[mc.PhysicalBehaviour].Wetness as f64 > 0.95 && self.random.value() as f64 > 0.9 {
            self.machinery[m].Health = 0.0;
            self.machinery_act_on_damage(m);
        }
    }

    /// DamagableMachineryBehaviour.Update
    pub fn machinery_update(&mut self, m: usize) {
        self.machinery[m].t += self.time.deltaTime;
        let mc = &self.machinery[m];
        let points = |g: &Game| g.physicals[g.machinery[m].PhysicalBehaviour].LocalColliderGridPoints.as_ref().expect("LocalColliderGridPoints before PhysicalBehaviour.Start").len();
        if mc.Destroyed && points(self) != 0 && mc.t > 1.0 && mc.SparkPrefab && self.random.value() < 4e-5 {
            self.machinery[m].t = 0.0;
            let p = self.machinery[m].PhysicalBehaviour;
            let n = points(self) as i32;
            let i = self.random.range_int(0, n) as usize;
            let vector = self.physicals[p].LocalColliderGridPoints.as_ref().unwrap()[i];
            // Instantiate(SparkPrefab, base.transform.TransformPoint(vector), Quaternion.identity)
            let at = self.transform_point3(p, [vector[0], vector[1], 0.0]);
            match self.machinery[m].SparkPrefabSource {
                Some(source) => {
                    self.spawn_prefab(source, at);
                }
                None => {
                    self.spawn_resource("Prefabs/BrokenElectronicsSpark", at);
                }
            }
        }
    }

    /// DamagableMachineryBehaviour.Stabbed
    pub fn machinery_stabbed(&mut self, m: usize, stabber: usize) {
        let mc = &self.machinery[m];
        if !mc.Indestructible && !mc.Destroyed && self.physicals[stabber].StabCausesWound {
            self.machinery[m].Health -= mc.StabDamageMultiplier;
            self.machinery_act_on_damage(m);
        }
    }

    /// DamagableMachineryBehaviour.OnFragmentHit
    pub fn machinery_on_fragment_hit(&mut self, m: usize, force: f32) {
        let mc = &mut self.machinery[m];
        if !mc.Indestructible {
            mc.Health = (super::d(mc.Health) - super::d(force) * mc.ExplosionDamageMultiplier * 250.0f32).f();
            self.machinery_act_on_damage(m);
        }
    }

    /// DamagableMachineryBehaviour.OnEMPHit: when not Indestructible and not Destroyed, SetAsBroken() then the spark clone
    /// (rendering). Instantiate(null) throws, so a machinery without its SparkPrefab stops the run.
    pub(super) fn machinery_on_emp_hit(&mut self, m: usize) {
        let mc = &self.machinery[m];
        if !mc.Indestructible && !mc.Destroyed {
            self.machinery_set_as_broken(m, true);
            if !self.machinery[m].SparkPrefab {
                crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null.");
            }
        }
    }

    /// DamagableMachineryBehaviour.BreakPermanently: when not Indestructible and not Destroyed, OnEMPHit, then CanRepair is off.
    pub(crate) fn machinery_break_permanently(&mut self, m: usize) {
        if !self.machinery[m].Indestructible && !self.machinery[m].Destroyed {
            self.machinery_on_emp_hit(m);
            self.machinery[m].CanRepair = false;
        }
    }

    /// DamagableMachineryBehaviour.Slice: when not Indestructible and not Destroyed, Health = 0 and ActOnDamage.
    pub(crate) fn machinery_slice(&mut self, m: usize) {
        let mc = &mut self.machinery[m];
        if !mc.Indestructible && !mc.Destroyed {
            mc.Health = 0.0;
            self.machinery_act_on_damage(m);
        }
    }

    /// DamagableMachineryBehaviour.Break: when not Indestructible and not Destroyed, Health -= BreakDamage and ActOnDamage.
    pub(crate) fn machinery_break(&mut self, m: usize) {
        let mc = &mut self.machinery[m];
        if !mc.Indestructible && !mc.Destroyed {
            mc.Health -= mc.BreakDamage;
            self.machinery_act_on_damage(m);
        }
    }

    /// DamagableMachineryBehaviour.ForceBreak
    pub(crate) fn machinery_force_break(&mut self, m: usize) {
        let mc = &mut self.machinery[m];
        if !mc.Indestructible && !mc.Destroyed {
            mc.Health = -1.0;
            self.machinery_act_on_damage(m);
        }
    }

    /// DamagableMachineryBehaviour.Repair: when CanRepair, the temperature is held just under TemperatureThreshold, Health
    /// gains a quarter of its initial value (clamped to it), and ActOnRepair runs.
    pub(crate) fn machinery_repair(&mut self, m: usize) {
        if !self.machinery[m].CanRepair {
            return;
        }
        let p = self.machinery[m].PhysicalBehaviour;
        let limit = self.machinery[m].TemperatureThreshold - 1.0;
        self.physicals[p].Temperature = super::mathf::min(self.physicals[p].Temperature, limit);
        let mc = &mut self.machinery[m];
        mc.Health += mc.initialHealth / 4.0;
        mc.Health = super::mathf::clamp(mc.Health, 0.0, mc.initialHealth);
        self.machinery_act_on_repair(m);
    }

    /// DamagableMachineryBehaviour.ActOnRepair: if Health is above 0.001, the object is no longer Destroyed, its
    /// BurnProgress is 0, the SpriteRenderer gets originalMaterial back, BehavioursToToggle are enabled and
    /// GameObjectsToToggle set active. OnRepaired must reach only presentation.
    pub(crate) fn machinery_act_on_repair(&mut self, m: usize) {
        if self.machinery[m].Health <= 0.001 {
            return;
        }
        let p = self.machinery[m].PhysicalBehaviour;
        self.physicals[p].BurnProgress = 0.0;
        self.machinery[m].Destroyed = false;
        self.physicals[p].sprite_material = self.machinery[m].originalMaterial;
        for (class, id) in self.machinery[m].BehavioursToToggle.clone() {
            if self.destroyed_components.contains(&id) {
                return self.menu_throw(&format!("DamagableMachineryBehaviour.ActOnRepair: the destroyed {class} in BehavioursToToggle (MissingReferenceException)"));
            }
            self.behaviour_enable(&class, id);
        }
        for id in self.machinery[m].GameObjectsToToggle.clone() {
            if self.destroyed_components.contains(&id) {
                return self.menu_throw("DamagableMachineryBehaviour.ActOnRepair: a destroyed GameObjectsToToggle entry (MissingReferenceException)");
            }
            self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, true));
        }
        self.machinery_invoke(m, "OnRepaired");
    }

    /// The Behaviour.enabled of component `id`, a BehavioursToToggle class.
    fn toggled_enabled(&mut self, class: &str, id: i32) -> &mut bool {
        macro_rules! find {
            ($arena:ident) => {
                &mut self.$arena.iter_mut().find(|x| x.id == id).unwrap_or_else(|| panic!("a {class} in the arena")).enabled
            };
        }
        match class {
            "LEDBulbBehaviour" => find!(leds),
            "DetectorBehaviour" => find!(detectors),
            "GeneratorBehaviour" => find!(generators),
            "SingleFloodlightBehaviour" => find!(floodlights),
            "SpikeGrenadeBehaviour" => find!(spike_grenades),
            "TemperatureRayGunBehaviour" => find!(ray_guns),
            "ThrusterbedBehaviour" => find!(thrusterbeds),
            "ArchelixCasterBehaviour" => find!(archelix_casters),
            "BlasterBehaviour" => find!(blasters),
            "BeamformerBehaviour" => find!(beamformers),
            "GenericScifiWeapon40Behaviour" => find!(scifi40s),
            "CeilingTurretBehaviour" => find!(ceiling_turrets),
            "PointerBehaviour" => find!(pointers),
            "AtomBombBehaviour" | "SeismicChargeBehaviour" => find!(prefab_bombs),
            "GateBehaviour" => find!(gates),
            "HoverThrusterBehaviour" => find!(hover_thrusters),
            "BlackHoleBehaviour" => find!(black_holes),
            "HovercarBehaviour" => find!(hovercars),
            "TransmitterBehaviour" => find!(transmitters),
            "AdhesiveCouplerBehaviour" => find!(couplers),
            "JukeboxBehaviour" => find!(jukeboxes),
            "AntennaReceiverBehaviour" => find!(antenna_receivers),
            "WinchBehaviour" => find!(winches),
            "ConveyorBeltBehaviour" => find!(conveyors),
            "RandomEventBehaviour" => find!(random_events),
            "TemperatureTransferBehaviour" => find!(temperature_transfers),
            "EMPBehaviour" => find!(emps),
            "ProjectileLauncherBehaviour" => find!(launchers),
            _ => unimplemented!("mod-only: DamagableMachineryBehaviour: {class}.enabled"),
        }
    }

    /// Behaviour.enabled = true on component `id`: on an active GameObject, AddToManager and the OnEnable.
    pub(super) fn behaviour_enable(&mut self, class: &str, id: i32) {
        // Already enabled: nothing happens. Only the classes SetAsBroken disables are ever disabled at run time.
        let prefab_enabled = self.with_component_instance(id, |_, inst, o| {
            let k = inst.objects[o].components.iter().position(|&c| c == id).unwrap();
            matches!(&inst.prefab.objects[o].components[k].1, crate::assets::db::Component::Script { enabled: true, .. })
        });
        if self.script_enabled(id, prefab_enabled) {
            return;
        }
        // A Start held back while the Behaviour was disabled is due now (the next run_pending_starts re-checks the object).
        while let Some(i) = self.deferred_starts.iter().position(|s| s.0 == id) {
            let start = self.deferred_starts.remove(i).1;
            self.pending_starts.push(start);
        }
        if self.toggle_on(class, id) {
            return;
        }
        if !self.with_component_instance(id, |_, inst, o| inst.objects[o].active) {
            return;
        }
        if let Some(&(order, b)) = self.behaviour_of.get(&id) {
            self.behaviours.add(order, b);
        }
        match class {
            // OnEnable: Joint.useMotor = true.
            "CeilingTurretBehaviour" => {
                let i = self.ceiling_turrets.iter().position(|x| x.id == id).unwrap();
                self.ceiling_turret_on_enable(i);
            }
            // OnEnable: UpdateActivation.
            "ObstructionEmitterBehaviour" => {
                let i = self.obstruction_emitters.iter().position(|x| x.id == id).unwrap();
                self.unity_callback(|g| g.obstruction_emitter_on_enable(i));
            }
            // OnEnable: Hinge.useMotor = true.
            "PointerBehaviour" => {
                let i = self.pointers.iter().position(|x| x.id == id).unwrap();
                self.pointer_on_enable(i);
            }
            // OnEnable: SliderJoint.useMotor = true.
            "WinchBehaviour" => {
                let i = self.winches.iter().position(|x| x.id == id).unwrap();
                self.winch_on_enable(i);
            }
            // OnEnable: TextMeshPro.enabled (presentation).
            "TransmitterBehaviour" => {
                let i = self.transmitters.iter().position(|x| x.id == id).expect("a TransmitterBehaviour in the arena");
                self.transmitter_on_enable(i);
            }
            "BeamformerBehaviour" => {
                let i = self.beamformers.iter().position(|x| x.id == id).unwrap();
                self.beamformer_on_enable(i);
            }
            "GateBehaviour" => {
                let i = self.gates.iter().position(|x| x.id == id).expect("a GateBehaviour in the arena");
                self.unity_callback(|g| g.gate_on_enable(i));
            }
            // OnEnable: UpdateActivation (IdleLoop).
            "HoverThrusterBehaviour" => {
                let i = self.hover_thrusters.iter().position(|x| x.id == id).expect("a HoverThrusterBehaviour in the arena");
                self.hover_thruster_update_activation(i);
            }
            // OnEnable: IdleLoop.Play(); AccLoop.Play() (the fan is presentation).
            "HovercarBehaviour" => {
                let i = self.hovercars.iter().position(|x| x.id == id).expect("a HovercarBehaviour in the arena");
                self.unity_callback(|g| g.hovercar_on_enable(i));
            }
            // OnEnable: UpdateActivation.
            "HeartMonitorBehaviour" => {
                let i = self.heart_monitors.iter().position(|x| x.id == id).expect("a HeartMonitorBehaviour in the arena");
                self.heart_monitor_update_activation(i);
            }
            "AntennaReceiverBehaviour" => {
                let i = self.antenna_receivers.iter().position(|x| x.id == id).expect("an AntennaReceiverBehaviour in the arena");
                self.antenna_receiver_on_enable(i);
            }
            "JukeboxBehaviour" => {
                let i = self.jukeboxes.iter().position(|x| x.id == id).expect("a JukeboxBehaviour in the arena");
                self.jukebox_on_enable(i);
            }
            // Every other class: the OnEnable a SetActive runs (script_on_enable_arm), none for a class without one.
            c if !super::declares(c, "OnEnable") => {}
            // A translated script's OnEnable may SetActive objects of its own clone, so it runs with the clone not checked out.
            name if super::translated::CLASSES.contains(&name) => {
                let Some(&(_, super::Behaviour::Translated(t))) = self.behaviour_of.get(&id) else { unreachable!("{class}: not a translated behaviour in the arena") };
                if self.tr_enabled(t) {
                    self.tr_on_enable(t);
                }
            }
            _ => self.with_component_instance(id, |g, inst, o| g.script_on_enable_arm(inst, o, class, &true, id)),
        }
    }

    /// Behaviour.enabled = true on `id`; whether it already was. A class with its own flag sets that, else the id leaves `disabled_ids`.
    fn toggle_on(&mut self, class: &str, id: i32) -> bool {
        let was = match class {
            "LEDBulbBehaviour" | "DetectorBehaviour" | "GeneratorBehaviour" | "SingleFloodlightBehaviour" | "SpikeGrenadeBehaviour"
            | "TemperatureRayGunBehaviour" | "ThrusterbedBehaviour" | "ArchelixCasterBehaviour" | "BlasterBehaviour" | "BeamformerBehaviour"
            | "GenericScifiWeapon40Behaviour" | "CeilingTurretBehaviour" | "PointerBehaviour" | "AtomBombBehaviour" | "SeismicChargeBehaviour"
            | "GateBehaviour" | "HoverThrusterBehaviour" | "BlackHoleBehaviour" | "HovercarBehaviour" | "TransmitterBehaviour"
            | "AdhesiveCouplerBehaviour" | "JukeboxBehaviour" | "AntennaReceiverBehaviour" | "WinchBehaviour" | "ConveyorBeltBehaviour"
            | "RandomEventBehaviour" | "TemperatureTransferBehaviour" | "EMPBehaviour" | "ProjectileLauncherBehaviour" => std::mem::replace(self.toggled_enabled(class, id), true),
            name if super::translated::CLASSES.contains(&name) => {
                let Some(&(_, super::Behaviour::Translated(t))) = self.behaviour_of.get(&id) else { unreachable!("{class}: not a translated behaviour in the arena") };
                let was = self.tr_enabled(t);
                self.tr_set_enabled(t, true);
                was
            }
            _ => self.set_own_enabled(class, id, true),
        };
        self.disabled_ids.remove(&id);
        was
    }

    /// DamagableMachineryBehaviour.ActOnDamage
    pub(crate) fn machinery_act_on_damage(&mut self, m: usize) {
        let mc = &self.machinery[m];
        if !(mc.Health > 0.0) && !mc.Destroyed {
            self.machinery_set_as_broken(m, true);
            let p = self.machinery[m].PhysicalBehaviour;
            let position = self.physicals[p].transform.position;
            if self.machinery[m].ExplodesOnBreak {
                // Explosion.Position = base.transform.position (a struct field: it stays).
                self.machinery[m].Explosion.Position = position;
                let (under_water, ex) = (self.physicals[p].IsUnderWater, self.machinery[m].Explosion);
                self.create_explosion_with_water(under_water, ex);
            }
            if let Some(source) = self.machinery[m].ExplosionPrefabSource {
                // Instantiate(ExplosionPrefab, base.transform.position, Quaternion.identity)
                self.spawn_prefab(source, position);
            }
        }
    }

    /// DamagableMachineryBehaviour.SetAsBroken
    pub(crate) fn machinery_set_as_broken(&mut self, m: usize, run_events: bool) {
        let mc = &mut self.machinery[m];
        mc.Destroyed = true;
        mc.Health = 0.0;
        for (class, id) in self.machinery[m].BehavioursToToggle.clone() {
            // (bool)monoBehaviour: an earlier toggle may have destroyed it.
            if !self.destroyed_components.contains(&id) {
                self.behaviour_disable(&class, id);
                self.disabled_ids.insert(id);
            }
        }
        for id in self.machinery[m].GameObjectsToToggle.clone() {
            if !self.destroyed_components.contains(&id) {
                self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, false));
            }
        }
        // SpriteRenderer.sharedMaterial = brokenMachineMaterial; then the sprite's texture (_MainTex) and RandomOffset (_Offset)
        // go into the block Start copied, which SetPropertyBlock hands the renderer.
        let p = self.machinery[m].PhysicalBehaviour;
        self.physicals[p].sprite_material = self.machinery[m].brokenMachineMaterial;
        let Some((sprite, ..)) = self.physicals[p].sprite else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let Some(mut block) = self.machinery[m].propertyBlock.take() else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let db = self.assets.as_deref().expect("DamagableMachineryBehaviour without the game's data");
        let texture = db.sprite(sprite).ok().and_then(|s| db.resolve(sprite.file, s.texture)).unwrap_or_else(|| unimplemented!("mod-only: SetAsBroken on a sprite without a texture"));
        block.set_texture("_MainTex", texture);
        block.set_vector("_Offset", self.machinery[m].RandomOffset);
        self.set_property_block(super::visual::SpriteAt::Physical(p), &block);
        self.machinery[m].propertyBlock = Some(block);
        if run_events {
            self.machinery_invoke(m, "OnDamaged");
        }
    }

    /// Behaviour.enabled = false on component `id`: RemoveFromManager, with the OnDisable of an active GameObject.
    pub(super) fn behaviour_disable(&mut self, class: &str, id: i32) {
        let active = self.with_component_instance(id, |_, inst, o| inst.objects[o].active);
        match class {
            "LEDBulbBehaviour" => {
                let i = self.leds.iter().position(|x| x.id == id).expect("a LEDBulbBehaviour in the arena");
                if !std::mem::replace(&mut self.leds[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Led(i));
                if active {
                    self.led_on_disable(i);
                }
            }
            "DetectorBehaviour" => {
                let i = self.detectors.iter().position(|x| x.id == id).expect("a DetectorBehaviour in the arena");
                if !std::mem::replace(&mut self.detectors[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Detector(i));
                if active {
                    self.detector_on_disable(i);
                }
            }
            "GeneratorBehaviour" => {
                let i = self.generators.iter().position(|x| x.id == id).expect("a GeneratorBehaviour in the arena");
                if !std::mem::replace(&mut self.generators[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Generator(i));
                if active {
                    self.generator_on_disable(i);
                }
            }
            "SingleFloodlightBehaviour" => {
                let i = self.floodlights.iter().position(|x| x.id == id).expect("a SingleFloodlightBehaviour in the arena");
                if !std::mem::replace(&mut self.floodlights[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Floodlight(i));
                if active {
                    self.with_component_instance(id, |g, inst, _| g.floodlight_on_disable(i, inst));
                }
            }
            "SpikeGrenadeBehaviour" => {
                let i = self.spike_grenades.iter().position(|x| x.id == id).expect("a SpikeGrenadeBehaviour in the arena");
                if !std::mem::replace(&mut self.spike_grenades[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::SpikeGrenade(i));
                if active {
                    self.spike_grenade_on_disable(i);
                }
            }
            "TemperatureRayGunBehaviour" => {
                let i = self.ray_guns.iter().position(|x| x.id == id).expect("a TemperatureRayGunBehaviour in the arena");
                if !std::mem::replace(&mut self.ray_guns[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::RayGun(i));
                if active {
                    self.ray_gun_on_disable(i);
                }
            }
            "ThrusterbedBehaviour" => {
                let i = self.thrusterbeds.iter().position(|x| x.id == id).expect("a ThrusterbedBehaviour in the arena");
                if !std::mem::replace(&mut self.thrusterbeds[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Thrusterbed(i));
                if active {
                    self.thrusterbed_on_disable(i);
                }
            }
            "ArchelixCasterBehaviour" => {
                let i = self.archelix_casters.iter().position(|x| x.id == id).expect("an ArchelixCasterBehaviour in the arena");
                // No OnDisable, and Start only (it is in no manager).
                self.archelix_casters[i].enabled = false;
            }
            "BlasterBehaviour" => {
                let i = self.blasters.iter().position(|x| x.id == id).expect("a BlasterBehaviour in the arena");
                if !std::mem::replace(&mut self.blasters[i].enabled, false) {
                    return;
                }
                // No OnDisable.
                self.behaviours.remove(super::Behaviour::Blaster(i));
            }
            "BeamformerBehaviour" => {
                let i = self.beamformers.iter().position(|x| x.id == id).expect("a BeamformerBehaviour in the arena");
                if !std::mem::replace(&mut self.beamformers[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Beamformer(i));
                if active {
                    self.beamformer_on_disable(i);
                }
            }
            "GenericScifiWeapon40Behaviour" => {
                let i = self.scifi40s.iter().position(|x| x.id == id).expect("a GenericScifiWeapon40Behaviour in the arena");
                // No OnDisable, and no manager message.
                self.scifi40s[i].enabled = false;
            }
            // enabled = false: OnDisable (Joint.useMotor = false) when it was enabled and its object is active.
            "CeilingTurretBehaviour" => {
                let i = self.ceiling_turrets.iter().position(|x| x.id == id).expect("a CeilingTurretBehaviour in the arena");
                if !std::mem::replace(&mut self.ceiling_turrets[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::CeilingTurret(i));
                if active {
                    self.ceiling_turret_on_disable(i);
                }
            }
            // enabled = false: OnDisable (Hinge.useMotor = false) when it was enabled and its object is active.
            "PointerBehaviour" => {
                let i = self.pointers.iter().position(|x| x.id == id).expect("a PointerBehaviour in the arena");
                if !std::mem::replace(&mut self.pointers[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Pointer(i));
                if active {
                    self.pointer_on_disable(i);
                }
            }
            // No OnDisable and no manager message: Use checks enabled.
            "AtomBombBehaviour" | "SeismicChargeBehaviour" => {
                let i = self.prefab_bombs.iter().position(|x| x.id == id).expect("a bomb in the arena");
                self.prefab_bombs[i].enabled = false;
            }
            // enabled = false: OnDisable when it was enabled and its object is active.
            "GateBehaviour" => {
                let i = self.gates.iter().position(|x| x.id == id).expect("a GateBehaviour in the arena");
                if !std::mem::replace(&mut self.gates[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Gate(i));
                if active {
                    self.unity_callback(|g| g.gate_on_disable(i));
                }
            }
            "HoverThrusterBehaviour" => {
                let i = self.hover_thrusters.iter().position(|x| x.id == id).expect("a HoverThrusterBehaviour in the arena");
                if !std::mem::replace(&mut self.hover_thrusters[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::HoverThruster(i));
                if active {
                    self.hover_thruster_on_disable(i);
                }
            }
            // OnDisable: the fan (presentation) and the loops stop.
            "HovercarBehaviour" => {
                let i = self.hovercars.iter().position(|x| x.id == id).expect("a HovercarBehaviour in the arena");
                if std::mem::replace(&mut self.hovercars[i].enabled, false) {
                    self.behaviours.remove(super::Behaviour::Hovercar(i));
                    if active {
                        self.unity_callback(|g| g.hovercar_on_disable(i));
                    }
                }
            }
            "TransmitterBehaviour" => {
                let i = self.transmitters.iter().position(|x| x.id == id).expect("a TransmitterBehaviour in the arena");
                if std::mem::replace(&mut self.transmitters[i].enabled, false) {
                    self.behaviours.remove(super::Behaviour::Transmitter(i));
                    if active {
                        self.transmitter_on_disable(i);
                    }
                }
            }
            "AdhesiveCouplerBehaviour" => {
                let i = self.couplers.iter().position(|x| x.id == id).expect("an AdhesiveCouplerBehaviour in the arena");
                if !std::mem::replace(&mut self.couplers[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Coupler(i));
            }
            "JukeboxBehaviour" => {
                let i = self.jukeboxes.iter().position(|x| x.id == id).expect("a JukeboxBehaviour in the arena");
                if !std::mem::replace(&mut self.jukeboxes[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Jukebox(i));
                if active {
                    self.jukebox_on_disable(i);
                }
            }
            "AntennaReceiverBehaviour" => {
                let i = self.antenna_receivers.iter().position(|x| x.id == id).expect("an AntennaReceiverBehaviour in the arena");
                if !std::mem::replace(&mut self.antenna_receivers[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::AntennaReceiver(i));
                if active {
                    self.antenna_receiver_on_disable(i);
                }
            }
            // enabled = false: RemoveFromManager; neither has an OnDisable.
            "RandomEventBehaviour" => {
                let i = self.random_events.iter().position(|x| x.id == id).expect("a RandomEventBehaviour in the arena");
                if std::mem::replace(&mut self.random_events[i].enabled, false) {
                    self.behaviours.remove(super::Behaviour::RandomEvent(i));
                }
            }
            "TemperatureTransferBehaviour" => {
                let i = self.temperature_transfers.iter().position(|x| x.id == id).expect("a TemperatureTransferBehaviour in the arena");
                if std::mem::replace(&mut self.temperature_transfers[i].enabled, false) {
                    self.behaviours.remove(super::Behaviour::TemperatureTransfer(i));
                }
            }
            "ConveyorBeltBehaviour" => {
                let i = self.conveyors.iter().position(|x| x.id == id).expect("a ConveyorBeltBehaviour in the arena");
                if !std::mem::replace(&mut self.conveyors[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Conveyor(i));
                if active {
                    self.conveyor_on_disable(i);
                }
            }
            "WinchBehaviour" => {
                let i = self.winches.iter().position(|x| x.id == id).expect("a WinchBehaviour in the arena");
                if !std::mem::replace(&mut self.winches[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Winch(i));
                if active {
                    self.winch_on_disable(i);
                }
            }
            "EMPBehaviour" => {
                let i = self.emps.iter().position(|x| x.id == id).expect("an EMPBehaviour in the arena");
                // No OnDisable.
                self.emps[i].enabled = false;
            }
            // No OnDisable: Use checks enabled, and Update and FixedUpdate stop with the behaviour.
            "ProjectileLauncherBehaviour" => {
                let i = self.launchers.iter().position(|x| x.id == id).expect("a ProjectileLauncherBehaviour in the arena");
                if !std::mem::replace(&mut self.launchers[i].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Launcher(i));
            }
            name if super::translated::CLASSES.contains(&name) => {
                let Some(&(_, super::Behaviour::Translated(t))) = self.behaviour_of.get(&id) else { unreachable!("{class}: not a translated behaviour in the arena") };
                if !self.tr_enabled(t) {
                    return;
                }
                self.tr_set_enabled(t, false);
                self.behaviours.remove(super::Behaviour::Translated(t));
                if active {
                    self.tr_on_disable(t);
                }
            }
            // Behaviour.enabled = false on a class with its own flag, or none: RemoveFromManager with the OnDisable of an
            // active GameObject (script_on_disable), when it was enabled.
            _ => {
                let was_enabled = self.with_component_instance(id, |g, inst, o| {
                    let k = inst.objects[o].components.iter().position(|&c| c == id).expect("the component on its object");
                    let crate::assets::db::Component::Script { enabled, .. } = &inst.prefab.objects[o].components[k].1 else { unreachable!() };
                    if !g.script_enabled(id, *enabled) {
                        return false;
                    }
                    match active {
                        true => g.script_on_disable(inst, o, k, false),
                        false => {
                            if let Some(&(_, b)) = g.behaviour_of.get(&id) {
                                g.behaviours.remove(b);
                            }
                        }
                    }
                    true
                });
                if was_enabled {
                    self.set_own_enabled(class, id, false);
                }
            }
        }
    }

    /// Behaviour.enabled = false on a class that keeps its own flag (the others go in `disabled_ids`).
    /// Returns the flag it had.
    pub(crate) fn set_own_enabled(&mut self, class: &str, id: i32, on: bool) -> bool {
        macro_rules! off {
            ($arena:ident) => {
                match self.$arena.iter_mut().find(|x| x.id == id) {
                    Some(x) => std::mem::replace(&mut x.enabled, on),
                    None => unreachable!("{class}: not in the arena"),
                }
            };
        }
        match class {
            "MetronomeBehaviour" => off!(metronomes),
            "ActOnCollide" => off!(act_on_collides),
            "DecimatorBehaviour" => off!(decimators),
            "ObstructionEmitterBehaviour" => off!(obstruction_emitters),
            "DroneBehaviour" => off!(drones),
            "BlackHoleBehaviour" => off!(black_holes),
            "DisplayBehaviour" => off!(displays),
            "ImmobilityFieldBehaviour" => off!(immobility_fields),
            "ForcefieldGeneratorBehaviour" => off!(forcefields),
            "KeyTriggerBehaviour" => {
                let Some(&(_, super::Behaviour::KeyTrigger(k))) = self.behaviour_of.get(&id) else { unreachable!("KeyTrigger in the arena") };
                std::mem::replace(&mut self.key_triggers[k].enabled, on)
            }
            "LagboxBehaviour" => off!(lagboxes),
            "TimedGateBehaviour" => off!(timed_gates),
            "LaserReceiverBehaviour" => off!(laser_receivers),
            "JavelinBehaviour" => off!(javelins),
            "PhysicsGunBehaviour" => off!(physics_guns),
            "SmoothLineControllerBehaviour" => off!(smooth_lines),
            "PulseDrumBehaviour" => {
                let Some(&(_, super::Behaviour::PulseDrum(k))) = self.behaviour_of.get(&id) else { unreachable!("PulseDrum in the arena") };
                std::mem::replace(&mut self.pulse_drums[k].enabled, on)
            }
            "TeslaCoilBehaviour" => off!(tesla_coils),
            "ActivatorElectrodeBehaviour" => off!(activator_electrodes),
            "AcceleratorGunBehaviour" => off!(accelerators),
            "ActivationToggleBehaviour" => off!(activation_toggles),
            "SpeedometerBehaviour" => match self.v45.speedometers.iter_mut().find(|x| x.id == id) {
                Some(x) => std::mem::replace(&mut x.enabled, on),
                None => unreachable!("{class}: not in the arena"),
            },
            "BuzzerBehaviour" => match self.v45.buzzers.iter_mut().find(|x| x.id == id) {
                Some(x) => std::mem::replace(&mut x.enabled, on),
                None => unreachable!("{class}: not in the arena"),
            },
            _ => match on {
                true => !self.disabled_ids.remove(&id),
                false => !self.disabled_ids.insert(id),
            },
        }
    }

    /// Whether script `id` is enabled for an OnEnable: a DeleteAfterTime's runtime flag (a UseEventTrigger enables it),
    /// else the prefab's m_Enabled unless it was disabled at run time.
    pub(super) fn script_enabled(&self, id: i32, prefab_enabled: bool) -> bool {
        match self.delete_afters.iter().find(|d| d.id == id) {
            Some(d) => d.enabled,
            None => prefab_enabled && !self.script_disabled(id),
        }
    }

    /// A translated behaviour whose enabled flag is false.
    fn tr_disabled(&self, id: i32) -> bool {
        matches!(self.behaviour_of.get(&id), Some(&(_, super::Behaviour::Translated(t))) if !self.tr_enabled(t))
    }

    /// Behaviour.enabled was set to false at run time (SetAsBroken), over the prefab's m_Enabled.
    pub(super) fn script_disabled(&self, id: i32) -> bool {
        self.leds.iter().any(|x| x.id == id && !x.enabled)
            || self.random_events.iter().any(|x| x.id == id && !x.enabled)
            || self.temperature_transfers.iter().any(|x| x.id == id && !x.enabled)
            || self.detectors.iter().any(|x| x.id == id && !x.enabled)
            || self.generators.iter().any(|x| x.id == id && !x.enabled)
            || self.floodlights.iter().any(|x| x.id == id && !x.enabled)
            || self.spike_grenades.iter().any(|x| x.id == id && !x.enabled)
            || self.ray_guns.iter().any(|x| x.id == id && !x.enabled)
            || self.thrusterbeds.iter().any(|x| x.id == id && !x.enabled)
            || self.blasters.iter().any(|x| x.id == id && !x.enabled)
            || self.archelix_casters.iter().any(|x| x.id == id && !x.enabled)
            || self.beamformers.iter().any(|x| x.id == id && !x.enabled)
            || self.scifi40s.iter().any(|x| x.id == id && !x.enabled)
            || self.disabled_ids.contains(&id)
            || self.metronomes.iter().any(|x| x.id == id && !x.enabled)
            || self.act_on_collides.iter().any(|x| x.id == id && !x.enabled)
            || self.decimators.iter().any(|x| x.id == id && !x.enabled)
            || self.obstruction_emitters.iter().any(|x| x.id == id && !x.enabled)
            || self.drones.iter().any(|x| x.id == id && !x.enabled)
            || self.propellers.iter().any(|x| x.id == id && !x.enabled)
            || self.black_holes.iter().any(|x| x.id == id && !x.enabled)
            || self.displays.iter().any(|x| x.id == id && !x.enabled)
            || self.immobility_fields.iter().any(|x| x.id == id && !x.enabled)
            || self.forcefields.iter().any(|x| x.id == id && !x.enabled)
            || matches!(self.behaviour_of.get(&id), Some(&(_, super::Behaviour::KeyTrigger(k))) if !self.key_triggers[k].enabled)
            || self.lagboxes.iter().any(|x| x.id == id && !x.enabled)
            || self.timed_gates.iter().any(|x| x.id == id && !x.enabled)
            || self.laser_receivers.iter().any(|x| x.id == id && !x.enabled)
            || self.javelins.iter().any(|x| x.id == id && !x.enabled)
            || self.physics_guns.iter().any(|x| x.id == id && !x.enabled)
            || matches!(self.behaviour_of.get(&id), Some(&(_, super::Behaviour::PulseDrum(k))) if !self.pulse_drums[k].enabled)
            || self.tesla_coils.iter().any(|x| x.id == id && !x.enabled)
            || self.activator_electrodes.iter().any(|x| x.id == id && !x.enabled)
            || self.accelerators.iter().any(|x| x.id == id && !x.enabled)
            || self.activation_toggles.iter().any(|x| x.id == id && !x.enabled)
            || self.v45.speedometers.iter().any(|x| x.id == id && !x.enabled)
            || self.v45.buzzers.iter().any(|x| x.id == id && !x.enabled)
            || self.tr_disabled(id)
    }
}
