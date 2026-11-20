//! ContextMenuOptionComponent.Buttons (each PhysicalBehaviour's list, filled by the scripts' Start) and
//! ContextMenuBehaviour's dynamic buttons: Show (on the "context" press, after HandleTools selected the hovered object)
//! merges the selected objects' buttons by Identity in selection order, skipping those whose Condition is false; a
//! button met again gains that object's actions and takes its LabelWhenMultipleAreSelected. A click runs every action,
//! then Hide. The fixed buttons (Delete, Copy, ...) are the menu prefab's (ui/tables.rs).
#![allow(non_snake_case)]

use super::{d, Game};

/// The scripts that add context menu buttons (from the game's menus, reference-probe/game/menus.txt) whose buttons
/// are not rewritten: a marker button on the object stands for them in the menu.
pub const NOT_YET: &[&str] = &[];

/// A button string built at run time ("setWingTo" + a wing type's name): kept once per distinct text.
pub fn intern(s: String) -> &'static str {
    static SEEN: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap();
    if let Some(&x) = seen.iter().find(|&&x| x == s) {
        return x;
    }
    let x: &'static str = Box::leak(s.into_boxed_str());
    seen.push(x);
    x
}

/// ContextMenuButton
#[derive(Default, Clone, Debug)]
pub struct MenuButton {
    pub Identity: &'static str,
    pub label: Label,
    pub Description: &'static str,
    pub LabelWhenMultipleAreSelected: &'static str,
    pub condition: Cond,
    pub actions: Vec<MenuAction>,
}

/// LabelGetter
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Label {
    Text(&'static str),
    /// LimbBehaviour: Broken ? "Mend bone" : "Break bone"
    Bone(usize),
    /// DamagableMachineryBehaviour / DestroyableBehaviour: !Indestructible ? "Make indestructible" : "Make destructible"
    MachineryIndestructible(usize),
    DestroyableIndestructible(usize),
    /// SyringeBehaviour: !Finite ? "Disable infinite source" : "Enable infinite source"
    SyringeInfinite(usize),
    /// ButtonBehaviour / DetectorBehaviour: !TriggerOnExit ? "Double trigger" : "Single trigger"
    ButtonTrigger(usize),
    DetectorTrigger(usize),
    /// DetectorBehaviour: !ShowBeam ? "Show beam" : "Hide beam"
    DetectorBeam(usize),
    /// CarBehaviour: !IsBrakeEngaged ? "Engage parking brakes" : "Disengage parking brakes"
    CarBrake(usize),
    /// !(Health > 0) ? "Repair engine" : "Break engine"
    CarEngine(usize),
    /// TelevisionBehaviour / TyreDeflationBehaviour: !Indestructible ? "Make indestructible" : "Make destructible"
    TelevisionIndestructible(usize),
    /// CathodeLightBehaviour: the same label.
    CathodeLightIndestructible(usize),
    TyreIndestructible(usize),
    /// LaserReceiverBehaviour: !DoubleTrigger ? "Double trigger receiver" : "Single trigger receiver"
    LaserReceiverTrigger(usize),
    /// LiquidLevelGateBehaviour: !TriggerOnExit ? "Double trigger" : "Single trigger".
    LiquidLevelGateTrigger(usize),
    /// KeyTriggerBehaviour: !DoubleTrigger ? "Double trigger" : "Single trigger".
    KeyTriggerDouble(usize),
    /// UniversalBellBehaviour: "Set to " + the note BellClipIndex + step away.
    BellNote(usize, i32),
    /// InvisibleOptionInContextMenu: !phys.spriteRenderer.enabled ? "Make visible" : "Make invisible"
    Invisible(usize),
    /// ParticleMachineBehaviour: (getValue() ? "Disable " : "Enable ") + smoke / fire / electricity
    ParticleMachine(usize, usize),
    /// GateBehaviour: !DoubleTrigger ? "Double trigger gate" : "Single trigger gate"
    GateTrigger(usize),
    /// MagnetBehaviour: !Reversed ? "Set to repulse" : "Set to attract"
    MagnetReversed(usize),
    /// MuteBehaviour: !Muted ? "Mute" : "Unmute"
    Mute(usize),
}

impl Default for Label {
    /// No label text.
    fn default() -> Self {
        Label::Text("")
    }
}

/// Condition
/// Default: a null Condition (always shown).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Cond {
    #[default]
    Always,
    /// LimbBehaviour: Person.OverridePoseIndex != the index.
    PoseIsNot(usize, i32),
    /// DamagableMachineryBehaviour "repairMachinery": Destroyed && CanRepair.
    MachineryRepair(usize),
    /// "shieldFromHeat" (false) / "unshieldFromHeat" (true): !Indestructible && HeatShielded == the flag &&
    /// !float.IsPositiveInfinity(TemperatureThreshold).
    MachineryShielded(usize, bool),
    /// IndestructibilityCanBeToggled
    MachineryToggle(usize),
    DestroyableToggle(usize),
    /// SyringeBehaviour: Pressure != the mode && Finite.
    SyringeModeIsNot(usize, i32),
    /// CanToggleInfinite
    SyringeCanToggle(usize),
    /// LiquidentifierBehaviour / LiquidDuplicatorBehaviour: !isBusy.
    LiquidentifierIdle(usize),
    LiquidDuplicatorIdle(usize),
    /// HovercarBehaviour: base.enabled.
    HovercarEnabled(usize),
    /// UniversalBellBehaviour: BellClipIndex < Count - 1 (step 1) or BellClipIndex > 0 (step -1).
    BellNote(usize, i32),
    /// AirfoilBehaviour: CurrentWingTypeName != WingTypes[i].Name
    WingNot(usize, usize),
    /// LiquidContainerLeakingBehaviour: holes.Count > 0
    LiquidLeakHoles(usize),
    /// BloodTankBehaviour: Mode != mode.
    BloodTankNot(usize, i32),
    /// LiquidPumpBehaviour: Pressure (Mode) != mode.
    LiquidPumpNot(usize, i32),
    /// TelevisionBehaviour: Broken.
    TelevisionBroken(usize),
    /// CathodeLightBehaviour: Broken.
    CathodeLightBroken(usize),
    /// TyreDeflationBehaviour: Deflated.
    TyreDeflated(usize),
    /// SpeedometerBehaviour "resetTopSpeed": TopSpeedMs > 0 (1.28.45, v45_new.rs).
    SpeedometerRecord(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAction {
    /// SpeedometerBehaviour "resetTopSpeed": TopSpeedMs = 0 (1.28.45, v45_new.rs).
    SpeedometerReset(usize),
    /// BuzzerBehaviour "chooseAudioClip": OpenFile (1.28.45, v45_new.rs).
    BuzzerChooseAudio(usize),
    /// PhysicalBehaviour "setTemperature": Utils.OpenFloatInputDialog (a dialog box: UI not rewritten).
    /// PhysicalBehaviour "setAngle": DialogBoxManager.TextEntry (UI not rewritten).
    /// LimbBehaviour "inspectLimb": LimbStatusViewBehaviour.
    InspectLimb(usize),
    /// LimbBehaviour "breakBones": HealBone if Broken, else BreakBone.
    ToggleBone(usize),
    /// LimbBehaviour: Person.OverridePoseIndex = the index.
    Pose(usize, i32),
    /// DamagableMachineryBehaviour: Health = initialHealth; ActOnRepair().
    MachineryRepair(usize),
    /// HeatShielded = the flag.
    MachineryShield(usize, bool),
    /// Indestructible = !Indestructible; once indestructible, repaired as above.
    MachineryToggle(usize),
    /// DestroyableBehaviour: Indestructible = !Indestructible.
    DestroyableToggle(usize),
    /// RandomSpriteBehaviour: chosenIndex += the step; SetSpriteToIndex().
    Skin(usize, i32),
    /// SyringeBehaviour: PressureMode = the mode.
    SyringeMode(usize, i32),
    /// Finite = !Finite; PressureMode = Push.
    SyringeInfinite(usize),
    /// TriggerOnExit = !TriggerOnExit.
    ButtonTrigger(usize),
    DetectorTrigger(usize),
    /// DetectorBehaviour: ShowBeam = !ShowBeam; UpdateActivated().
    DetectorBeam(usize),
    /// CarBehaviour: ReverseGear (MotorSpeed = -MotorSpeed).
    CarReverse(usize),
    /// IsBrakeEngaged = !IsBrakeEngaged.
    CarBrake(usize),
    CarEngine(usize),
    /// TelevisionBehaviour: Repair().
    TelevisionRepair(usize),
    /// CathodeLightBehaviour: Repair().
    CathodeLightRepair(usize),
    /// CathodeLightBehaviour: Indestructible = !Indestructible; indestructible refills and mends it.
    CathodeLightToggle(usize),
    /// Indestructible = !Indestructible; Repair() if now indestructible and Broken.
    TelevisionToggle(usize),
    /// TyreDeflationBehaviour: Repair().
    TyreRepair(usize),
    /// Indestructible = !Indestructible; Repair() if now indestructible.
    TyreToggle(usize),
    /// StartCoroutine(FlushRoutine()).
    LiquidentifierFlush(usize),
    LiquidDuplicatorFlush(usize),
    /// UniversalBellBehaviour "bellSetNote": PianoDialogBox.Show (the bell).
    BellPiano(usize),
    /// UniversalBellBehaviour: SetBellIndex(BellClipIndex + step) (the colour, sounds and label are presentation;
    /// the sound arrays stay one clip long), then Play.
    BellNote(usize, i32),
    /// InvisibleOptionInContextMenu: Invisible = !Invisible; UpdateVisibility
    InvisibleToggle(usize),
    /// JukeboxBehaviour.NextSong (1) or PreviousSong (-1)
    JukeboxSong(usize, i32),
    /// ParticleMachineBehaviour: setValue(!getValue()); UpdateActivation (particles).
    ParticleMachineToggle(usize, usize),
    /// AirfoilBehaviour.SetWingType(WingTypes[i])
    WingSet(usize, usize),
    /// GateBehaviour: DoubleTrigger = !DoubleTrigger
    GateTrigger(usize),
    /// LiquidContainerLeakingBehaviour.Repair
    LiquidLeakRepair(usize),
    /// BoatMotorBehaviour.Reverse
    BoatReverse(usize),
    /// RotorBehaviour: SetSpeed(0 - Speed)
    RotorReverse(usize),
    /// MagnetBehaviour: Reversed = !Reversed
    MagnetReverse(usize),
    /// BloodTankBehaviour: Mode = mode; UpdateActivation.
    BloodTankMode(usize, i32),
    /// LiquidPumpBehaviour.SetMode(mode)
    LiquidPumpMode(usize, i32),
    /// RandomCarTextureBehaviour: chosenIndex += step; SetSpritesToIndex.
    CarTexture(usize, i32),
    /// HovercarBehaviour: BaseSpeed = 0 - BaseSpeed.
    HovercarReverse(usize),
    /// LaserReceiverBehaviour: DoubleTrigger = !DoubleTrigger.
    LaserReceiverTrigger(usize),
    /// LiquidLevelGateBehaviour: TriggerOnExit = !TriggerOnExit.
    LiquidLevelGateTrigger(usize),
    /// KeyTriggerBehaviour: DoubleTrigger = !DoubleTrigger.
    KeyTriggerDouble(usize),
    /// CounterBehaviour: CurrentCounter = MinValue.
    CounterReset(usize),
    /// A button that opens a number entry box (Utils.OpenFloatInputDialog and WinchBehaviour.SetLimitFromContextMenu).
    Entry(Entry),
    /// KeyTriggerBehaviour "changeKeyTrigger": each selected trigger opens its own DialogBoxManager.KeyEntry.
    KeyChange(usize),
    /// MuteBehaviour: SetMute(!Muted).
    Mute(usize),
    /// A script whose buttons are not rewritten yet.
    NotYet(&'static str),
}

impl MenuAction {
    /// The action opens a dialog or panel the UI does not have yet.
    pub fn needs_ui(self) -> bool {
        matches!(self, MenuAction::NotYet(_))
    }
}

impl MenuButton {
    /// ContextMenuButton..ctor (the four overloads: the condition is `condition`, the label getter `label`)
    pub fn new(Identity: &'static str, label: &'static str, Description: &'static str, action: MenuAction) -> Self {
        // The constructors set LabelWhenMultipleAreSelected to the description.
        MenuButton { Identity, label: Label::Text(label), Description, LabelWhenMultipleAreSelected: Description, condition: Cond::Always, actions: vec![action] }
    }
}

fn trigger_label(on_exit: bool) -> &'static str {
    if on_exit { crate::il!(str "ButtonBehaviour::<Start>b__9_0" 1) } else { crate::il!(str "ButtonBehaviour::<Start>b__9_0" 0) }
}

fn indestructible_label(indestructible: bool) -> &'static str {
    if indestructible { crate::il!(str "CathodeLightBehaviour::<Start>b__13_2" 1) } else { crate::il!(str "CathodeLightBehaviour::<Start>b__13_2" 0) }
}

/// What a number entry box sets: the setValueFunction of each Utils.OpenFloatInputDialog (and the winch's own
/// SetLimitFromContextMenu), by script.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Entry {
    CouplerStick(usize),
    GateThreshold(usize),
    GateMaxPower(usize),
    HoverHeight(usize),
    Lagbox(usize),
    LiquidGate(usize),
    Metronome(usize),
    ServoDelta(usize),
    ServoSpeed(usize),
    RotorSpeed(usize),
    TimedGate(usize),
    WinchLower(usize),
    WinchUpper(usize),
    PointerSpeed(usize),
    /// DetectorBehaviour "setDetectorRange".
    DetectorRange(usize),
    /// Utils.OpenIntInputDialog
    CounterWrap(usize),
    /// Utils.OpenTextInputDialog
    DisplayText(usize),
    RadioChannel(usize),
    KeyDescription(usize),
    /// HeatingElementBehaviour "Set target temperature" and PhysicalBehaviour "setTemperature" (the physical).
    HeatTarget(usize),
    Temperature(usize),
    /// PhysicalBehaviour "setAngle": DialogBoxManager.TextEntry (the physical).
    Angle(usize),
    /// BuzzerBehaviour "setLoudness" (1.28.45, v45_new.rs).
    BuzzerLoudness(usize),
    /// Utils.OpenColourInputDialog
    LedColour(usize),
    CathodeColour(usize),
    LaserColour(usize),
    /// EditableTintBehaviour: its index in Game::editable_tints.
    Tint(usize),
    /// ContextMenuBehaviour.SaveAction's TextEntry: the contraption name (contraption.rs).
    SaveContraption,
}

impl Entry {
    /// The calling behaviour's type T: the box applies to every selected object with one.
    fn same_type(self, o: Entry) -> bool {
        use Entry::*;
        std::mem::discriminant(&self) == std::mem::discriminant(&o) || matches!((self, o), (WinchLower(_) | WinchUpper(_), WinchLower(_) | WinchUpper(_)))
    }
}

/// UnityEngine.LogType
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LogType {
    Error,
    Assert,
    Warning,
    Log,
    Exception,
}

/// An open DialogBoxManager.TextEntry: its message, the input field's placeholder and text, and what Apply sets.
#[derive(Clone, Debug, PartialEq)]
pub struct EntryBox {
    /// TMP_InputField.ContentType.DecimalNumber (else Standard).
    pub number: bool,
    pub title: String,
    pub placeholder: String,
    pub text: String,
    pub targets: Vec<Entry>,
}

/// PianoDialogBox while shown: SelectedBellClip and the apply delegate's copy of the selected bells.
#[derive(Clone, Debug, PartialEq)]
pub struct Piano {
    pub selected: PianoPick,
    pub targets: Vec<usize>,
}

/// SelectedBellClip: the bell's own BellClips entry (no key is that object, so none lights up) or a key's copy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PianoPick {
    Bell(usize),
    Key(usize),
}

impl Piano {
    /// UiPianoKeyBehaviour.Update: the key's target graphic is green while `PianoDialogBox.SelectedBellClip == BellClip`, else
    /// white (ui/view.rs draws the keys with it); a key's copy is the one object that is its BellClip, and the bell's own entry is none of them.
    pub fn key_lit(&self, key: usize) -> bool {
        self.selected == PianoPick::Key(key)
    }
}

/// PianoKey's names.
pub const PIANO_KEY_NAMES: [&str; 12] = ["C", "CSharpDFlat", "D", "DSharpEFlat", "E", "F", "FSharpGFlat", "G", "GSharpAFlat", "A", "ASharpBFlat", "B"];

/// The dialog's UiPianoKeyBehaviour keys in the Main scene's order: the white keys C 3 to B 6, then the black ones;
/// each BellClip's Name and Key.
pub fn piano_keys() -> Vec<(String, usize)> {
    let mut keys = Vec::new();
    for black in [false, true] {
        for octave in 3..=6 {
            for (k, name) in PIANO_KEY_NAMES.iter().enumerate() {
                if name.contains("Sharp") == black {
                    keys.push((format!("{name} {octave}"), k));
                }
            }
        }
    }
    keys
}

/// UiPianoKeyBehaviour.Start: the button's onClick plays Source.PlayOneShot(BellClip.Clip) and sets SelectedBellClip = BellClip
/// (ui/mod.rs `Act::PianoKey`); the key's text is BellClip.GetDisplayName(false), this.
pub fn piano_key_label(key: usize) -> String {
    PIANO_KEY_NAMES[key].replace("Sharp", "\u{266f}\n").replace("Flat", "\u{266d}")
}

impl Game {
    /// PianoDialogBox.ApplyClicked: the delegate finds SelectedBellClip's Name in BellClips and sets every bell.
    pub fn piano_apply(&mut self) {
        // PianoDialogBox.Hide: the dialog is inactive once the callback has run (`piano` is None)
        let Some(p) = self.piano.take() else { return };
        let name = match p.selected {
            PianoPick::Bell(i) => self.bell_clips[i].0.clone(),
            PianoPick::Key(k) => piano_keys()[k].0.clone(),
        };
        let Some(num) = self.bell_clips.iter().position(|c| c.0 == name) else {
            self.ui_sound(super::audio::UiSoundKind::Error);
            self.contraption.dialogs.push(super::contraption::DialogRequest::notification(crate::il!(str "UniversalBellBehaviour/<>c__DisplayClass13_0::<Start>b__8" 0).into()));
            return;
        };
        for b in p.targets {
            self.universal_bell_set_index(b, num as i32);
        }
    }
}

/// LimbStatusViewBehaviour.Main: gameObject.activeSelf, Limbs, t, the two texts and the four bars' localScale.y.
#[derive(Clone, Debug, PartialEq)]
pub struct LimbView {
    pub active: bool,
    pub limbs: Vec<usize>,
    pub(crate) t: f32,
    /// LimbDamageSourceStats: "<b>{n} limbs selected</b>", the "<color=#FF5555>" limb state, the source stats.
    pub stats: [String; 3],
    pub blood: String,
    /// ConsciousnessBar, ShockBar, OxygenBar, AdrenalineBar.
    pub bars: [f32; 4],
}

/// The scene's UpdateInterval (the script default is 1).
const LIMB_VIEW_INTERVAL: f32 = 0.1;

impl Default for LimbView {
    /// The Main scene's texts and bar scales, shown until the first Refresh.
    fn default() -> Self {
        // LimbStatusViewBehaviour.Awake: Main = this
        // LimbStatusViewBehaviour.Start: SetActive(false)
        LimbView {
            active: false,
            limbs: Vec::new(),
            t: 0.0,
            stats: [String::new(), String::new(), "12 bruises\n3 gunshots wounds\n1 stab wound\n43% burnt\n5% acid damage\n".into()],
            blood: "integrity\n25% \n\nblood amount\n50%\n\nblood pressure\n65%\n\ninternal temp.\n37 \u{b0}C".into(),
            bars: [0.5; 4],
        }
    }
}

impl Game {
    /// LimbStatusViewBehaviour.Update (Unity calls it while the view is active: a frame-loop caller checks `active`).
    pub fn limb_view_update(&mut self) {
        self.limb_view.t += self.time.unscaledDeltaTime;
        if self.limb_view.t > LIMB_VIEW_INTERVAL {
            self.limb_view.t = 0.0;
            self.limb_view_refresh();
        }
    }

    /// LimbStatusViewBehaviour.Refresh (StringBuilder.AppendLine ends a line with Environment.NewLine, "\r\n" on Windows; the
    /// carriage return takes no room when drawn).
    pub(crate) fn limb_view_refresh(&mut self) {
        let limbs = self.limb_view.limbs.clone();
        if limbs.is_empty() || limbs.iter().any(|&l| self.physicals[self.limbs[l].PhysicalBehaviour].destroyed) {
            self.limb_view.stats = Default::default();
            self.limb_view.blood.clear();
            self.limb_view.active = false;
            return;
        }
        let stats = [format!("{} limbs selected", limbs.len()), self.limb_view_state(&limbs), self.limb_view_sources(&limbs)];
        self.limb_view.stats = stats;
        self.limb_view.blood = self.limb_view_blood(&limbs);
    }

    /// LimbStatusViewBehaviour.GetBloodInformation (also sets the bars).
    pub(crate) fn limb_view_blood(&mut self, limbs: &[usize]) -> String {
        use super::mathf::clamp01;
        use super::temperature::round_to_int;
        let [mut n1, mut n2, mut n3, mut n4, mut n5, mut n6, mut n7, mut n8] = [0.0f32; 8];
        for &l in limbs {
            let (limb, c) = (&self.limbs[l], &self.circulations[l]);
            let p = &self.people[limb.Person];
            n1 += clamp01(limb.Health / limb.InitialHealth);
            n2 += self.get_amount(super::container::Owner::Circulation(l), self.blood_type(l));
            n3 += c.BloodFlow;
            n4 += clamp01(p.Consciousness);
            n5 += clamp01(p.ShockLevel);
            n6 += clamp01(p.OxygenLevel);
            n7 += clamp01(clamp01(p.AdrenalineLevel));
            n8 += limb.InternalTemperature;
        }
        let n = limbs.len() as f32;
        n2 /= n;
        n4 /= n;
        n5 /= n;
        n6 /= n;
        n7 /= n;
        n8 /= n;
        n8 = self.prefs.celsius_to_preference(n8);
        self.limb_view.bars = [n4, n5, n6, n7];
        let suffix = super::utils::get_temperature_unit_suffix(self.prefs.TemperatureUnit);
        format!(
            "integrity\r\n{}%\n\nblood amount\r\n{}%\n\nblood pressure\r\n{}%\n\ninternal temp.\r\n{} {}",
            round_to_int((d(n1) / n * 100.0f32).f()),
            round_to_int(n2 * 100.0),
            round_to_int((d(n3) / n * 100.0f32).f()),
            round_to_int(n8),
            suffix
        )
    }

    /// LimbStatusViewBehaviour.GetLimbState
    pub(crate) fn limb_view_state(&self, limbs: &[usize]) -> String {
        let (mut bleeding, mut numb, mut dismembered, mut internal, mut lungs, mut heart, mut braindead, mut brain_damage) = (false, false, false, false, false, false, false, false);
        let mut b: u8 = 1;
        for &l in limbs {
            let (limb, c) = (&self.limbs[l], &self.circulations[l]);
            bleeding |= c.BleedingRate > 0.5;
            internal |= c.InternalBleedingIntensity > 0.1;
            lungs |= limb.LungsPunctured;
            numb |= limb.Numbness > 0.5;
            heart |= c.WasInitiallyPumping && !c.IsPump;
            dismembered |= limb.IsDismembered;
            if limb.HasBrain {
                brain_damage |= self.people[limb.Person].BrainDamaged;
                braindead |= self.people[limb.Person].Braindead;
            }
            if self.limb_is_considered_alive(l) {
                if b != u8::MAX {
                    b = 0;
                }
            } else if limb.HasBrain {
                b = u8::MAX;
            }
        }
        let mut s = String::new();
        for (on, line) in [(b != 0, crate::il!(str "LimbStatusViewBehaviour::GetLimbState" 0)), (bleeding, crate::il!(str "LimbStatusViewBehaviour::GetLimbState" 1)), (internal, crate::il!(str "LimbStatusViewBehaviour::GetLimbState" 2)), (lungs, crate::il!(str "LimbStatusViewBehaviour::GetLimbState" 3)), (numb, crate::il!(str "LimbStatusViewBehaviour::GetLimbState" 4)), (dismembered, crate::il!(str "LimbStatusViewBehaviour::GetLimbState" 5)), (heart, crate::il!(str "LimbStatusViewBehaviour::GetLimbState" 6)), (braindead, crate::il!(str "LimbStatusViewBehaviour::GetLimbState" 7)), (brain_damage, crate::il!(str "LimbStatusViewBehaviour::GetLimbState" 8))] {
            if on {
                s += line;
                s += "\r\n";
            }
        }
        s
    }

    /// LimbStatusViewBehaviour.GetSourceStats
    pub(crate) fn limb_view_sources(&self, limbs: &[usize]) -> String {
        use super::mathf::clamp01;
        use super::temperature::round_to_int;
        let (mut bruises, mut gunshots, mut stabs, mut broken) = (0i32, 0i32, 0i32, 0i32);
        let (mut burnt, mut acid, mut rotten) = (0.0f32, 0.0f32, 0.0f32);
        for &l in limbs {
            let (limb, c) = (&self.limbs[l], &self.circulations[l]);
            rotten = (d(rotten) + d(clamp01(self.skins[l].rottenProgress)) * 100.0f32).f();
            acid = (d(acid) + d(clamp01(self.skins[l].acidProgress)) * 100.0f32).f();
            burnt = (d(burnt) + d(clamp01(self.physicals[limb.PhysicalBehaviour].BurnProgress)) * 100.0f32).f();
            if limb.Broken {
                broken += 1;
            }
            stabs += c.StabWoundCount as i32;
            bruises += limb.BruiseCount as i32;
            gunshots += c.GunshotWoundCount as i32;
        }
        let n = limbs.len() as f32;
        let (burnt, acid, rotten) = (round_to_int(burnt / n) as f32, round_to_int(acid / n) as f32, round_to_int(rotten / n) as f32);
        let plural = |k: i32, one: &str, many: &str| if k == 1 { one.to_string() } else { many.to_string() };
        let mut s = String::new();
        if bruises > 0 {
            s += &format!("{bruises} {}\n", plural(bruises, crate::il!(str "LimbStatusViewBehaviour::GetSourceStats" 3), crate::il!(str "LimbStatusViewBehaviour::GetSourceStats" 2)));
        }
        if stabs > 0 {
            s += &format!("{stabs} stab {}\n", plural(stabs, crate::il!(str "LimbStatusViewBehaviour::GetSourceStats" 7), crate::il!(str "LimbStatusViewBehaviour::GetSourceStats" 6)));
        }
        if gunshots > 0 {
            s += &format!("{gunshots} gunshot {}\n", plural(gunshots, crate::il!(str "LimbStatusViewBehaviour::GetSourceStats" 7), crate::il!(str "LimbStatusViewBehaviour::GetSourceStats" 6)));
        }
        if broken > 0 {
            s += &if limbs.len() == 1 { crate::il!(str "LimbStatusViewBehaviour::GetSourceStats" 17).to_string() } else { format!("{broken} broken {}\n", plural(broken, crate::il!(str "LimbStatusViewBehaviour::GetSourceStats" 15), crate::il!(str "LimbStatusViewBehaviour::GetSourceStats" 14))) };
        }
        if burnt > 0.0 {
            s += &format!("{}% burnt\n", cs_float(burnt));
        }
        if rotten > 0.0 {
            s += &format!("{}% rotted\n", cs_float(rotten));
        }
        if acid > 0.0 {
            s += &format!("{}% skin damage\n", cs_float(acid));
        }
        s
    }
}

/// An open DialogBoxManager.KeyEntry: the key trigger it sets, KeyTriggerEditor.SetKey and its WaitForUserInput.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyBox {
    pub trigger: usize,
    pub key: i32,
    pub wait: Option<KeyWait>,
}

/// KeyTriggerEditor.WaitForUserInput: oldKey, oldPos and which WaitUntil it is at.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyWait {
    old: i32,
    pos: Option<[f32; 2]>,
    stage: u8,
}

impl KeyWait {
    /// KeyTriggerEditor.StartWaiting, KeyTriggerEditor.Start (EditOnAwake) and KeyTriggerEditor.Awake: oldKey and oldPos are kept and the button
    /// shows "Press any button..." (the Button lookup and Cursor.visible are presentation).
    pub fn new(old: i32, pos: Option<[f32; 2]>) -> Self {
        KeyWait { old, pos, stage: 0 }
    }
}

/// KeyTriggerEditor.SetText: KeyCode.ToString().
pub fn key_name(code: i32) -> String {
    super::keycode::KEYCODES.iter().find(|x| x.0 == code).map_or(code.to_string(), |x| x.1.to_string())
}

impl KeyBox {
    /// The text on the key button.
    pub fn text(&self) -> String {
        if self.wait.is_some() { crate::il!(str "KeyTriggerEditor/<WaitForUserInput>d__10::MoveNext" 0).into() } else { key_name(self.key) }
    }

    /// KeyTriggerEditor.WaitForUserInput and KeyTriggerEditor.HasReceivedUserInput, one frame: WaitUntil(!Input.anyKey); WaitUntil(HasReceivedUserInput() || moved over 200
    /// pixels); then on a key WaitUntil(!Input.anyKey), else SetKey = oldKey. HasReceivedUserInput takes the first
    /// KeyCode (in value order) released this frame. (InputSystem.Save rewrites the unchanged controls file.)
    pub fn step(&mut self, keys: &std::collections::HashMap<String, super::tool::Control>, mouse: Option<[f32; 2]>) {
        let Some(w) = self.wait.as_mut() else { return };
        let any = keys.values().any(|c| c.held);
        let moved = match (w.pos, mouse) {
            (Some(a), Some(b)) => ((d(a[0]) - d(b[0])) * (d(a[0]) - d(b[0])) + (d(a[1]) - d(b[1])) * (d(a[1]) - d(b[1]))).0.sqrt() as f32 > 200.0,
            _ => false,
        };
        match w.stage {
            0 => {
                if !any {
                    w.stage = 1;
                }
            }
            1 => {
                let up = super::keycode::KEYCODES.iter().find(|(_, n)| keys.get(*n).is_some_and(|c| c.up));
                if let Some(&(code, _)) = up {
                    self.key = code;
                }
                if up.is_some() || moved {
                    if moved {
                        self.key = w.old;
                        self.wait = None;
                    } else {
                        w.stage = 2;
                    }
                }
            }
            _ => {
                if !any {
                    self.wait = None;
                }
            }
        }
    }
}

/// The open ColorpickerDialogBehaviour: its HSV, Description text, the hex code input field and what Apply sets.
#[derive(Clone, Debug, PartialEq)]
pub struct ColourBox {
    pub hsv: [f32; 3],
    pub description: &'static str,
    /// HexCodeInput's text, and whether it has focus (onEndEdit fires when it loses it).
    pub hex: String,
    pub editing: bool,
    pub targets: Vec<Entry>,
}

impl ColourBox {
    /// ColorpickerDialogBehaviour.get_RGB: the RGB property's getter.
    pub fn rgb(&self) -> [f32; 4] {
        let c = hsv_to_rgb(self.hsv);
        crate::ui::uifn("ColorpickerDialogBehaviour.get_RGB", || colour_state(c));
        c
    }

    /// ColorpickerDialogBehaviour.set_RGB: Color.RGBToHSV into HSV.
    pub(crate) fn set_rgb(&mut self, c: [f32; 4]) {
        self.hsv = rgb_to_hsv(c);
        let h = self.hsv;
        crate::ui::uifn("ColorpickerDialogBehaviour.set_RGB", || format!("{:.3},{:.3},{:.3}", h[0], h[1], h[2]));
    }

    /// ColorpickerDialogBehaviour.UpdateHexCodeInput
    pub fn update_hex(&mut self) {
        self.hex = format!("#{}", html_rgb(self.rgb()));
        let hex = self.hex.clone();
        crate::ui::uifn("ColorpickerDialogBehaviour.UpdateHexCodeInput", || hex);
    }

    /// ColorpickerDialogBehaviour.Awake's HexCodeInput.onEndEdit, with ColorpickerDialogBehaviour.set_RGB (the colour's HSV); its other listeners are `drag_sv` and `drag_hue`.
    pub fn end_edit(&mut self) {
        if !std::mem::take(&mut self.editing) {
            return;
        }
        let mut e = self.hex.clone();
        if !e.starts_with('#') {
            e.insert(0, '#');
        }
        if let Some(c) = parse_html_colour(&e) {
            self.set_rgb(c);
            // UpdateElements: the preview box shows RGB.
            let c = hsv_to_rgb(self.hsv);
            crate::ui::uifn("ColorpickerDialogBehaviour.UpdateElements", || colour_state(c));
        }
    }

    /// Update while dragging the saturation/value box: the handle's offset from the box's top left as fractions.
    pub fn drag_sv(&mut self, x: f32, y: f32) {
        self.hsv[1] = x.abs();
        self.hsv[2] = 1.0 - y.abs();
        self.update_hex();
    }

    /// Update while dragging the hue bar: the handle's offset from its top as a fraction.
    pub fn drag_hue(&mut self, y: f32) {
        self.hsv[0] = 1.0 - y.abs();
        self.update_hex();
    }
}

/// Quaternion.Euler(0, 0, z): z * (PI / 180) (Vector3 * float), then the native EulerToQuaternion (18073d0c0, kOrderUnityDefault)
/// halves it and takes the CRT sinf and cosf (x, y, z in turn; x and y give 0 and 1). The products and sums are the native ones in
/// order: they decide a zero's sign (Euler(0, 0, -0), a flipped spawn at 0 degrees, is (0, 0, +0, 1)).
pub fn euler_z_rotation(z: f32) -> [f32; 4] {
    let h = z * (std::f32::consts::PI / 180.0) * 0.5;
    let (sx, cx, sy, cy, sz, cz) = (0.0f32, 1.0f32, 0.0f32, 1.0f32, crate::unity::sinf(h), crate::unity::cosf(h));
    let f3 = ((sx * cy + cx * 0.0) + sy * 0.0) - 0.0;
    let f1 = ((cx * sy + cy * 0.0) + sx * 0.0) - 0.0;
    let f6 = ((cx * cy - sx * 0.0) - sy * 0.0) - 0.0;
    let f2 = ((cy * 0.0 + cx * 0.0) + 0.0) - sx * sy;
    [
        ((cz * f3 + f6 * 0.0) + sz * f1) - f2 * 0.0,
        ((cz * f1 + f6 * 0.0) + f2 * 0.0) - sz * f3,
        ((cz * f2 + sz * f6) + f3 * 0.0) - f1 * 0.0,
        ((cz * f6 - f3 * 0.0) - f1 * 0.0) - sz * f2,
    ]
}

#[test]
fn euler_z_rotation_zero_signs() {
    assert_eq!(euler_z_rotation(-0.0).map(f32::to_bits), [0, 0, 0, 0x3f80_0000]);
    let q = euler_z_rotation(-30.0);
    assert_eq!(q.map(f32::to_bits), [0, 0, crate::unity::sinf(-30.0 * (std::f32::consts::PI / 180.0) * 0.5).to_bits(), crate::unity::cosf(-30.0 * (std::f32::consts::PI / 180.0) * 0.5).to_bits()]);
}

/// `transform.localEulerAngles = (0, 0, z)`: Transform::SetLocalEulerAngles, Quaternion.Euler's CRT quaternion, then
/// SetLocalR's (1800ca1c0) normalize (gapB r11: GaugeBehaviour.SetArm's arm was one ulp off without it; the
/// SetLocalEuler polynomial path does not match the recording).
pub fn local_euler_z_rotation(z: f32) -> [f32; 4] {
    crate::unity::transform::normalize(euler_z_rotation(z))
}

/// Color.RGBToHSV
pub fn rgb_to_hsv(c: [f32; 4]) -> [f32; 3] {
    let [r, g, b, _] = c;
    if b > g && b > r {
        rgb_to_hsv_helper(4.0, b, r, g)
    } else if g > r {
        rgb_to_hsv_helper(2.0, g, b, r)
    } else {
        rgb_to_hsv_helper(0.0, r, g, b)
    }
}

/// Color.RGBToHSVHelper
fn rgb_to_hsv_helper(offset: f32, dominant: f32, one: f32, two: f32) -> [f32; 3] {
    let v = dominant;
    if v == 0.0 {
        return [0.0, 0.0, v];
    }
    let num = if !(one > two) { one } else { two };
    let num2 = v - num;
    let (mut h, s) = if num2 != 0.0 { ((d(offset) + (d(one) - d(two)) / d(num2)).f(), num2 / v) } else { ((d(offset) + (d(one) - d(two))).f(), 0.0) };
    h /= 6.0;
    if h < 0.0 {
        h += 1.0;
    }
    [h, s, v]
}

/// Color.HSVToRGB(H, S, V) (hdr: no clamp); the alpha is Color.white's.
/// A colour as the per-function checks write it (r,g,b to three places).
pub(crate) fn colour_state(c: [f32; 4]) -> String {
    format!("{:.3},{:.3},{:.3}", c[0], c[1], c[2])
}

pub fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 4] {
    if s == 0.0 {
        return [v, v, v, 1.0];
    }
    if v == 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let num = h * 6.0;
    let num2 = num.floor() as i32;
    let num3 = num - num2 as f32;
    let num4 = (d(v) * (d(1.0) - d(s))).f();
    let num5 = (d(v) * (d(1.0) - d(s) * d(num3))).f();
    let num6 = (d(v) * (d(1.0) - d(s) * (d(1.0) - d(num3)))).f();
    let (r, g, b) = match num2 {
        0 | 6 => (v, num6, num4),
        1 => (num5, v, num4),
        2 => (num4, v, num6),
        3 => (num4, num5, v),
        4 => (num6, num4, v),
        5 | -1 => (v, num4, num5),
        _ => (0.0, 0.0, 0.0),
    };
    [r, g, b, 1.0]
}

/// ColorUtility.ToHtmlStringRGB: Mathf.RoundToInt (Math.Round, half to even) of each channel * 255, clamped.
pub fn html_rgb(c: [f32; 4]) -> String {
    let byte = |x: f32| ((x * 255.0) as f64).round_ties_even().clamp(i32::MIN as f64, i32::MAX as f64).clamp(0.0, 255.0) as u8;
    format!("{:02X}{:02X}{:02X}", byte(c[0]), byte(c[1]), byte(c[2]))
}

/// ColorUtility.TryParseHtmlString of a "#" string (the native TryParseHtmlColor): at most 9 bytes, hex digits after
/// the "#", and "#RGB", "#RGBA" (each digit doubled), "#RRGGBB" or "#RRGGBBAA"; the alpha starts at 255.
pub fn parse_html_colour(s: &str) -> Option<[f32; 4]> {
    let b = s.as_bytes();
    if b.first() != Some(&b'#') || b.len() > 9 || !b[1..].iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let digits: Vec<u8> = match b.len() {
        4 | 5 => b[1..].iter().flat_map(|&c| [c, c]).collect(),
        7 | 9 => b[1..].to_vec(),
        _ => return None,
    };
    let mut out = [255u8; 4];
    for (o, pair) in out.iter_mut().zip(digits.chunks(2)) {
        *o = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    Some(out.map(|x| x as f32 / 255.0))
}

/// Single.ToString() under the invariant culture, as Mono's runtime formats it: "G" with 7 significant digits,
/// scientific ("E+XX") below 1e-5 or from 1e7.
pub fn cs_float(v: f32) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return "0".into();
    }
    let e = format!("{:.6e}", v as f64);
    let (mantissa, exp) = e.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let mut s = String::from(if neg { "-" } else { "" });
    if (-5..7).contains(&exp) {
        if exp < 0 {
            s += "0.";
            s += &"0".repeat((-exp - 1) as usize);
            s += digits;
        } else {
            let int = (exp + 1) as usize;
            if digits.len() <= int {
                s += digits;
                s += &"0".repeat(int - digits.len());
            } else {
                s += &digits[..int];
                s += ".";
                s += &digits[int..];
            }
        }
    } else {
        s += &digits[..1];
        if digits.len() > 1 {
            s += ".";
            s += &digits[1..];
        }
        s += &format!("E{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    s
}

/// Single.TryParse(s) under the invariant culture (NumberStyles.Float | AllowThousands): surrounding white space, a
/// leading sign, group separators in the integer part, a decimal point and an exponent; "Infinity", "-Infinity" and
/// "NaN"; an out-of-range value fails. The digits are parsed to double and rounded to float, as the runtime does.
pub fn cs_parse_float(s: &str) -> Option<f32> {
    let t = s.trim_matches(|c: char| c == ' ' || ('\u{9}'..='\u{d}').contains(&c));
    match t {
        "Infinity" | "+Infinity" => return Some(f32::INFINITY),
        "-Infinity" => return Some(f32::NEG_INFINITY),
        "NaN" => return Some(f32::NAN),
        _ => {}
    }
    let b = t.as_bytes();
    let mut i = 0;
    let mut out = String::new();
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        out.push(b[i] as char);
        i += 1;
    }
    let mut digits = 0;
    while i < b.len() && (b[i].is_ascii_digit() || (b[i] == b',' && digits > 0)) {
        if b[i] != b',' {
            out.push(b[i] as char);
            digits += 1;
        }
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        out.push('.');
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            out.push(b[i] as char);
            digits += 1;
            i += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        let mut exp = String::from("e");
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            exp.push(b[j] as char);
            j += 1;
        }
        let start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            exp.push(b[j] as char);
            j += 1;
        }
        if j > start {
            out += &exp;
            i = j;
        }
    }
    if i != b.len() {
        return None;
    }
    let v = out.parse::<f64>().ok()? as f32;
    v.is_finite().then_some(v)
}

/// Int32.TryParse(s) under the invariant culture (NumberStyles.Integer): surrounding white space, a leading sign and
/// digits; out of range fails.
pub fn cs_parse_int(s: &str) -> Option<i32> {
    let t = s.trim_matches(|c: char| c == ' ' || ('\u{9}'..='\u{d}').contains(&c));
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.strip_prefix('+').unwrap_or(t).parse().ok()
}

impl Game {
    /// Utils.OpenFloatInputDialog, Utils.OpenIntInputDialog, Utils.OpenTextInputDialog, Utils.IsFirstOfTypeInSelection (and the winch's SetLimitFromContextMenu): with several objects selected only the
    /// first of the type opens the box, which then applies to every selected one with that script.
    fn entry_open(&mut self, e: Entry, actions: &[MenuAction]) {
        use Entry::*;
        let first = actions.iter().find_map(|a| match *a {
            MenuAction::Entry(o) if o.same_type(e) => Some(o),
            _ => None,
        });
        if first != Some(e) {
            return;
        }
        let targets = actions.iter().filter_map(|a| match *a {
            MenuAction::Entry(o) if o.same_type(e) => Some(o),
            _ => None,
        }).collect();
        // Utils.OpenColourInputDialog: ColorpickerDialogBehaviour.Open(initialValue).
        // ColorpickerDialogBehaviour.Open: RGB = the colour, UpdateHexCodeInput; DialogBox.OpenDialogboxCount and the UI blocker are `colour.is_some()`
        // ColorpickerDialogBehaviour.Start: the dialog starts closed (`colour` is None)
        // ColorpickerDialogBehaviour.UpdateElements: ui/view.rs draws the handles, the image's _Hue (ui/loading.rs) and the preview from ColourBox::hsv.
        let colour = match e {
            LedColour(i) => Some((self.leds[i].Color, crate::il!(str "CathodeLightBehaviour::<Start>b__13_1" 1))),
            CathodeColour(i) => Some((self.cathode_lights[i].Color, crate::il!(str "CathodeLightBehaviour::<Start>b__13_1" 1))),
            LaserColour(i) => Some((self.lasers[i].UserSetColour, crate::il!(str "LaserBehaviour::<Start>b__24_1" 1))),
            Tint(i) => Some((self.editable_tints[i], crate::il!(str "EditableTintBehaviour::<Start>b__5_1" 1))),
            _ => None,
        };
        if let Some((c, description)) = colour {
            // ColorpickerDialogBehaviour.Open(Color) on a closed box: RGB = the colour, UpdateElements, UpdateHexCodeInput.
            let open = self.colour.is_none();
            let mut b = ColourBox { hsv: [0.0; 3], description, hex: String::new(), editing: false, targets };
            b.set_rgb(c);
            let preview = hsv_to_rgb(b.hsv);
            crate::ui::uifn("ColorpickerDialogBehaviour.UpdateElements", || colour_state(preview));
            b.update_hex();
            if open {
                let hex = b.hex.clone();
                crate::ui::uifn("ColorpickerDialogBehaviour.Open", || format!("True {hex}"));
            }
            // ColorpickerDialogBehaviour.Open: an open one does nothing more than AddUiBlocker once.
            if self.colour.is_none() {
                self.global_add_ui_blocker();
            }
            self.colour = Some(b);
            return;
        }
        let (value, title, placeholder) = match e {
            LedColour(_) | CathodeColour(_) | LaserColour(_) | Tint(_) | SaveContraption => unreachable!(),
            CouplerStick(c) => (self.couplers[c].StickDistance, crate::il!(str "AdhesiveCouplerBehaviour::<Start>b__18_0" 0), "Distance".into()),
            GateThreshold(g) => (self.gates[g].ThresholdPercentage * 100.0, crate::il!(str "GateBehaviour::<Start>b__17_0" 0), crate::il!(str "GateBehaviour::<Start>b__17_0" 1).into()),
            GateMaxPower(g) => (self.gates[g].MaxPower, crate::il!(str "GateBehaviour::<Start>b__17_1" 0), crate::il!(str "GateBehaviour::<Start>b__17_1" 1).into()),
            HoverHeight(h) => (self.hover_thrusters[h].BaseHoverHeight * 0.82397, crate::il!(str "HoverThrusterBehaviour::<Start>b__16_0" 0), "Target height in meters from 0.5 m to 10 km".into()),
            Lagbox(g) => (self.lagboxes[g].DelayModifier, crate::il!(str "LagboxBehaviour::<Start>b__10_0" 0), "Target delay in seconds from 0.04s to 60s".into()),
            LiquidGate(g) => (self.liquid_level_gates[g].Threshold * 100.0, crate::il!(str "LiquidLevelGateBehaviour::<Start>b__12_2" 0), crate::il!(str "GateBehaviour::<Start>b__17_0" 1).into()),
            // Utils.OpenFloatInputDialog(Loudness * 0.82397f, ..., "Set loudness", $"Minimum distance from {1f}m to {20f}m")
            BuzzerLoudness(b) => (self.v45.buzzers[b].Loudness * 0.82397, "Set loudness", format!("Minimum distance from {}m to {}m", cs_float(1.0), cs_float(20.0))),
            Metronome(m) => (self.metronomes[m].TempoModifier, crate::il!(str "MetronomeBehaviour::<Start>b__18_0" 0), "Target rate in Hertz from 0.125 Hz to 32 Hz".into()),
            ServoDelta(i) => (self.tr.motorised_hinge_behaviour[i].Delta, crate::il!(str "MotorisedHingeBehaviour::<Start>b__10_0" 0), "Angle".into()),
            ServoSpeed(i) => (self.tr.motorised_hinge_behaviour[i].Speed, crate::il!(str "MotorisedHingeBehaviour::<Start>b__10_2" 0), "Speed".into()),
            RotorSpeed(i) => (self.tr.rotor_behaviour[i].Speed, crate::il!(str "RotorBehaviour::<Start>b__7_0" 0), "Speed".into()),
            TimedGate(g) => (self.timed_gates[g].DelayModifier, crate::il!(str "TimedGateBehaviour::<Start>b__6_0" 0), "Target duration in seconds from 0.04s to 16s".into()),
            WinchLower(w) => (self.winches[w].LowerLimit, "Set winch lower limit", crate::il!(str "WinchBehaviour::SetLimitFromContextMenu" 2).into()),
            WinchUpper(w) => (self.winches[w].UpperLimit, "Set winch upper limit", crate::il!(str "WinchBehaviour::SetLimitFromContextMenu" 2).into()),
            PointerSpeed(i) => (self.pointers[i].Speed, crate::il!(str "PointerBehaviour::<Start>b__10_0" 0), "Speed".into()),
            DetectorRange(i) => (self.detectors[i].Range * 0.82397, crate::il!(str "DetectorBehaviour::<Start>b__22_2" 0), "Target distance in meters from 0.2 m to 40 m".into()),
            HeatTarget(i) => {
                let r = self.heating_elements[i].AllowedTemperatureRange;
                (self.prefs.celsius_to_preference(self.heating_elements[i].TargetTemperature), crate::il!(str "HeatingElementBehaviour::<Start>b__10_0" 0), format!("Target temperature in {} ({} to {})", self.prefs.temperature_unit_name(), cs_float(r[0]), cs_float(r[1])))
            }
            // SelectedObjects.Count > 1: every selected object has a PhysicalBehaviour, so one action each.
            Temperature(p) => {
                let many = actions.iter().filter(|a| matches!(a, MenuAction::Entry(Temperature(_)))).count() > 1;
                (self.prefs.celsius_to_preference(if many { self.AmbientTemperature } else { self.physicals[p].Temperature }), crate::il_v!(str "PhysicalBehaviour::<Start>b__137_0" 0, "PhysicalBehaviour::<Start>b__148_0" 0), format!("Target temperature in {}", self.prefs.temperature_unit_name()))
            }
            // With several selected only SelectedObjects[0] opens it; its "0" goes to the text component only, so the
            // field itself starts empty. With one, InputField.text = "0".
            Angle(_) => {
                let many = targets.len() > 1;
                return self.entry = Some(EntryBox { number: true, title: crate::il_v!(str "PhysicalBehaviour::<Start>b__137_1" 0, "PhysicalBehaviour::<Start>b__148_1" 0).into(), placeholder: crate::il_v!(str "PhysicalBehaviour::<Start>b__137_1" 0, "PhysicalBehaviour::<Start>b__148_1" 0).into(), text: if many { String::new() } else { "0".into() }, targets });
            }
            CounterWrap(c) => {
                let s = &self.counters[c];
                let text = format!("Value from {} to {}", s.MinValue, s.MaxValue + 1);
                self.entry = Some(EntryBox { number: true, title: crate::il!(str "CounterBehaviour::<Start>b__8_1" 0).into(), placeholder: text, text: s.ResetValue.to_string(), targets });
                return;
            }
            DisplayText(d) => return self.entry = Some(EntryBox { number: false, title: crate::il!(str "DisplayBehaviour::<Start>b__11_0" 0).into(), placeholder: crate::il!(str "DisplayBehaviour::<Start>b__11_0" 1).into(), text: self.displays[d].Value.clone(), targets }),
            RadioChannel(t) => return self.entry = Some(EntryBox { number: false, title: crate::il!(str "TransmitterBehaviour::<Start>b__9_0" 0).into(), placeholder: crate::il!(str "TransmitterBehaviour::<Start>b__9_0" 1).into(), text: self.transmitters[t].Channel.clone(), targets }),
            KeyDescription(k) => {
                // $"...{KeyCode}...": the enum's name.
                let code = self.key_triggers[k].KeyCode;
                let name = super::keycode::KEYCODES.iter().find(|x| x.0 == code).map_or(code.to_string(), |x| x.1.to_string());
                let title = format!("Set description\n\"Press {name} to...\"");
                return self.entry = Some(EntryBox { number: false, title, placeholder: crate::il!(str "KeyTriggerBehaviour::<Start>b__12_1" 1).into(), text: self.key_triggers[k].Description.clone(), targets });
            }
        };
        self.entry = Some(EntryBox { number: true, title: title.into(), placeholder, text: cs_float(value), targets });
    }

    /// The box's Apply: each target parses the text (a failure shows its notification each time) and sets its value.
    pub fn entry_apply(&mut self) {
        use super::mathf::{clamp, clamp01};
        use Entry::*;
        let Some(b) = self.entry.take() else { return };
        for t in b.targets {
            match t {
                SaveContraption => {
                    self.contraption_ask_for_save(&b.text);
                    continue;
                }
                DisplayText(d) => {
                    self.display_set_text(d, b.text.clone());
                    continue;
                }
                RadioChannel(i) => {
                    self.transmitter_set_channel(i, b.text.clone());
                    continue;
                }
                KeyDescription(k) => {
                    self.key_triggers[k].Description = b.text.clone();
                    self.key_trigger_update_display(k);
                    continue;
                }
                _ => {}
            }
            if let CounterWrap(c) = t {
                match cs_parse_int(&b.text) {
                    // Mathf.Clamp(int)
                    Some(v) => {
                        let s = &mut self.counters[c];
                        s.ResetValue = v.clamp(s.MinValue, s.MaxValue + 1);
                    }
                    None => self.notify(crate::il!(str "Utils/<>c__DisplayClass98_0`1::<OpenFloatInputDialog>g__setValue|0" 0).into()),
                }
                continue;
            }
            // setAngle: float.TryParse, else nothing; transform.eulerAngles = (0, 0, result).
            if let Angle(p) = t {
                if let Some(v) = cs_parse_float(&b.text) {
                    self.physical_set_rotation(p, euler_z_rotation(v));
                }
                continue;
            }
            let Some(v) = cs_parse_float(&b.text) else {
                self.notify(if matches!(t, WinchLower(_) | WinchUpper(_)) { crate::il!(str "WinchBehaviour/<>c__DisplayClass20_0::<SetLimitFromContextMenu>g__setLimit|1" 0) } else { crate::il!(str "Utils/<>c__DisplayClass98_0`1::<OpenFloatInputDialog>g__setValue|0" 0) }.into());
                continue;
            };
            match t {
                CouplerStick(c) => self.coupler_set_stick_distance(c, v),
                GateThreshold(g) => self.gate_set_threshold(g, v),
                GateMaxPower(g) => self.gates[g].MaxPower = v,
                HoverHeight(h) => self.hover_thrusters[h].BaseHoverHeight = clamp(v * 1.2136364, 0.6068182, 12136.364),
                Lagbox(g) => {
                    self.lagboxes[g].DelayModifier = clamp(v, 0.04, 60.0);
                    let p = self.lagboxes[g].physical;
                    self.physicals[p].ActivationPropagationDelay = self.lagboxes[g].DelayModifier;
                }
                LiquidGate(g) => self.liquid_level_gates[g].Threshold = clamp01(v / 100.0),
                Metronome(m) => self.metronomes[m].TempoModifier = clamp(v, 0.125, 32.0),
                BuzzerLoudness(b) => {
                    self.v45.buzzers[b].Loudness = v * 1.2136364;
                    self.buzzer_update_loudness(b);
                }
                ServoDelta(i) => self.motorised_hinge_behaviour_set_delta(i, v),
                ServoSpeed(i) => self.tr.motorised_hinge_behaviour[i].Speed = clamp(v, 0.0, 10.0),
                RotorSpeed(i) => self.rotor_behaviour_set_speed(i, v),
                TimedGate(g) => self.timed_gates[g].DelayModifier = clamp(v, 0.04, 16.0),
                WinchLower(w) => {
                    let a = self.winches[w].UpperLimit - 0.01;
                    // Mathf.Min: a < b ? a : b
                    self.winches[w].LowerLimit = if a < v { a } else { v };
                    self.winch_update_limits(w);
                }
                PointerSpeed(i) => self.pointers[i].Speed = clamp(v, 0.0, 10.0),
                DetectorRange(i) => self.detectors[i].Range = clamp(v * 1.2136364, 0.24272728, 48.545456),
                HeatTarget(i) => {
                    let r = self.heating_elements[i].AllowedTemperatureRange;
                    self.heating_elements[i].TargetTemperature = clamp(self.prefs.preference_to_celsius(v), r[0], r[1]);
                }
                Temperature(p) => self.physicals[p].Temperature = clamp(self.prefs.preference_to_celsius(v), -273.0, 1e9),
                CounterWrap(_) | DisplayText(_) | RadioChannel(_) | KeyDescription(_) | LedColour(_) | CathodeColour(_) | LaserColour(_) | Tint(_) | Angle(_) | SaveContraption => unreachable!(),
                WinchUpper(w) => {
                    let b = self.winches[w].LowerLimit + 0.01;
                    // Mathf.Max: a > b ? a : b
                    self.winches[w].UpperLimit = if v > b { v } else { b };
                    self.winch_update_limits(w);
                }
            }
        }
    }
}

impl Game {
    /// Global.AddUiBlocker
    pub fn global_add_ui_blocker(&mut self) {
        self.ActiveUiBlockers += 1;
    }

    /// Global.RemoveUiBlocker
    pub fn global_remove_ui_blocker(&mut self) {
        self.ActiveUiBlockers -= 1;
    }

    /// Global.get_ActiveUiBlock: ActiveUiBlockers > 0.
    pub fn global_active_ui_block(&self) -> bool {
        self.ActiveUiBlockers > 0
    }

    /// ColorpickerDialogBehaviour.Apply and ColorpickerDialogBehaviour.Close and EditableTintBehaviour.SetTint (the click on Apply ends the hex field's edit first): OnApply(RGB), each
    /// selected target's setValueFunction.
    pub fn colour_apply(&mut self) {
        use Entry::*;
        let Some(mut b) = self.colour.take() else { return };
        self.global_remove_ui_blocker();
        crate::ui::uifn("ColorpickerDialogBehaviour.Apply", || "False".into());
        crate::ui::uifn("ColorpickerDialogBehaviour.Close", || "False".into());
        b.end_edit();
        let c = b.rgb();
        let first = b.targets.first().copied();
        for t in b.targets {
            match t {
                // The callback's UpdateBulbMaskColour is the clicked LED's (`this`, the first target).
                LedColour(i) => {
                    self.leds[i].Color = c;
                    self.leds[i].colourCached = false;
                    if let Some(LedColour(owner)) = first {
                        self.led_update_bulb_mask_colour(owner);
                    }
                }
                CathodeColour(i) => self.cathode_lights[i].Color = [c[0], c[1], c[2], 1.0],
                LaserColour(i) => {
                    let [h, s, v] = rgb_to_hsv(c);
                    // Mathf.Max(0.01f, V)
                    self.lasers[i].UserSetColour = hsv_to_rgb([h, s, if 0.01 > v { 0.01 } else { v }]);
                }
                Tint(i) => self.editable_tint_set_tint(i, c),
                _ => unreachable!(),
            }
        }
    }

    /// EditableTintBehaviour.SetTint(color): Tint = color; the renderer's block with _Tint = color set back on it.
    pub fn editable_tint_set_tint(&mut self, i: usize, color: [f32; 4]) {
        self.editable_tints[i] = color;
        let at = super::visual::SpriteAt::Physical(self.editable_tint_phys[i]);
        let mut block = self.get_property_block(at);
        block.set_color("_Tint", color);
        self.set_property_block(at, &block);
    }
}

/// TexturepackInitialisationBehaviour's serialised fields (the texture pack folder and its meta file's name).
#[derive(Clone, Debug, PartialEq)]
pub struct TexturepackInitialisation {
    pub Path: String,
    pub MetaFileName: Option<String>,
}

impl Default for TexturepackInitialisation {
    fn default() -> Self {
        Self {
            Path: "TexturePacks".to_string(),
            MetaFileName: Default::default(),
        }
    }
}

impl TexturepackInitialisation {
    /// TexturepackInitialisationBehaviour..ctor: Path = "TexturePacks"; MetaFileName stays null.
    #[allow(dead_code)]
    pub fn new() -> Self {
        TexturepackInitialisation { Path: "TexturePacks".into(), MetaFileName: None }
    }
}

#[cfg(test)]
mod entry_tests {
    use super::{cs_float, cs_parse_float};

    #[test]
    fn key_wait() {
        use crate::game::tool::Control;
        let held = |n: &str, up: bool| std::collections::HashMap::from([(n.to_string(), Control { down: false, held: !up, up })]);
        let none = std::collections::HashMap::new();
        let mut b = super::KeyBox { trigger: 0, key: 32, wait: Some(super::KeyWait::new(32, Some([0.0, 0.0]))) };
        b.step(&held("Mouse0", false), Some([0.0, 0.0]));
        b.step(&none, Some([0.0, 0.0]));
        b.step(&held("A", true), Some([0.0, 0.0]));
        assert_eq!((b.key, b.wait.is_some()), (97, true));
        b.step(&none, Some([0.0, 0.0]));
        assert_eq!((b.key, b.text()), (97, "A".to_string()));
        let mut b = super::KeyBox { trigger: 0, key: 32, wait: Some(super::KeyWait::new(32, Some([0.0, 0.0]))) };
        b.step(&none, Some([0.0, 0.0]));
        b.step(&held("B", true), Some([300.0, 0.0]));
        assert_eq!((b.key, b.wait), (32, None));
    }

    #[test]
    fn set_angle() {
        assert_eq!(super::euler_z_rotation(0.0), [0.0, 0.0, 0.0, 1.0]);
        let q = super::euler_z_rotation(90.0);
        assert!((q[2] - 0.70710677).abs() < 1e-7 && (q[3] - 0.70710677).abs() < 1e-7);
    }

    #[test]
    fn colours() {
        use super::{html_rgb, hsv_to_rgb, parse_html_colour, rgb_to_hsv};
        assert_eq!(rgb_to_hsv([1.0, 0.0, 0.0, 1.0]), [0.0, 1.0, 1.0]);
        assert_eq!(rgb_to_hsv([0.0, 0.0, 1.0, 1.0]), [4.0 / 6.0, 1.0, 1.0]);
        assert_eq!(hsv_to_rgb([1.0, 1.0, 1.0]), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(hsv_to_rgb([0.5, 0.0, 0.25]), [0.25, 0.25, 0.25, 1.0]);
        assert_eq!(html_rgb([1.0, 0.5, 0.0, 1.0]), "FF8000");
        assert_eq!(html_rgb([2.0, -1.0, 0.002, 1.0]), "FF0001");
        assert_eq!(parse_html_colour("#f80"), Some([1.0, 136.0 / 255.0, 0.0, 1.0]));
        assert_eq!(parse_html_colour("#00FF0080"), Some([0.0, 1.0, 0.0, 128.0 / 255.0]));
        for bad in ["#", "#12", "#123456789", "#12345g", "red", "#red", "#123456 "] {
            assert_eq!(parse_html_colour(bad), None, "{bad}");
        }
    }

    #[test]
    fn float_text() {
        for (v, s) in [(1.0f32, "1"), (0.5, "0.5"), (1.0 / 3.0, "0.3333333"), (100.0, "100"), (-2.25, "-2.25"), (1e7, "1E+07"), (1.5e-6, "1.5E-06"), (0.0001, "0.0001"), (25.0, "25"), (1234567.0, "1234567")] {
            assert_eq!(cs_float(v), s);
        }
        assert_eq!(cs_parse_float(" 1,000.5 "), Some(1000.5));
        assert_eq!(cs_parse_float("-.5e1"), Some(-5.0));
        assert_eq!(cs_parse_float("5."), Some(5.0));
        assert_eq!(super::cs_parse_int(" -12 "), Some(-12));
        assert_eq!(super::cs_parse_int("+7"), Some(7));
        assert_eq!(super::cs_parse_int("1.5"), None);
        assert_eq!(super::cs_parse_int("99999999999"), None);
        for bad in ["", ".", "1e", "abc", "1.2.3", "1e40", ",5"] {
            assert_eq!(cs_parse_float(bad), None, "{bad}");
        }
    }
}

/// A merged dynamic button as ContextMenuBehaviour shows it.
#[derive(Clone, Debug)]
pub struct ShownButton {
    pub Identity: &'static str,
    /// The text TextUpdaterBehaviour last wrote.
    pub label: String,
    /// LabelGetter (the TextUpdaterBehaviour's StringFunction).
    pub getter: Label,
    pub Description: &'static str,
    pub actions: Vec<MenuAction>,
}

impl Game {
    /// The Starts that only add buttons (misc::Menu).
    /// The ContextMenuButtons the translated classes' Start adds (tools/iltrans leaves them out): Start adds them
    /// after everything else it does that the simulation sees (Magnet's UpdateActivated is audio and particles).
    pub(super) fn translated_menu(&mut self, b: super::translated::TBehaviour) {
        use super::translated::TBehaviour as T;
        let no_phys = || -> usize { crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let (p, buttons) = match b {
            T::BoatMotorBehaviour(i) => (self.tr.boat_motor_behaviour[i].phys, vec![MenuButton::new("reverseCar", crate::il!(str "BoatMotorBehaviour::Start" 1), crate::il!(str "BoatMotorBehaviour::Start" 2), MenuAction::BoatReverse(i))]),
            T::RotorBehaviour(i) => (self.tr.rotor_behaviour[i].PhysicalBehaviour, vec![
                MenuButton { LabelWhenMultipleAreSelected: crate::il!(str "RotorBehaviour::<Start>b__7_0" 0), ..MenuButton::new("rotorSetSpeed", crate::il!(str "RotorBehaviour::<Start>b__7_0" 0), crate::il!(str "RotorBehaviour::Start" 2), MenuAction::Entry(Entry::RotorSpeed(i))) },
                MenuButton { LabelWhenMultipleAreSelected: crate::il!(str "RotorBehaviour::Start" 5), ..MenuButton::new("reverseRotor", crate::il!(str "RotorBehaviour::Start" 5), crate::il!(str "RotorBehaviour::Start" 5), MenuAction::RotorReverse(i)) },
            ]),
            T::MagnetBehaviour(i) => (self.tr.magnet_behaviour[i].PhysicalBehaviour, vec![
                MenuButton { label: Label::MagnetReversed(i), ..MenuButton::new("reversePolarity", "", crate::il!(str "MagnetBehaviour::Start" 1), MenuAction::MagnetReverse(i)) },
            ]),
            T::MotorisedHingeBehaviour(i) => (self.tr.motorised_hinge_behaviour[i].PhysicalBehaviour, vec![
                MenuButton { LabelWhenMultipleAreSelected: crate::il!(str "MotorisedHingeBehaviour::Start" 1), ..MenuButton::new("servoSetDelta", crate::il!(str "MotorisedHingeBehaviour::Start" 1), crate::il!(str "MotorisedHingeBehaviour::Start" 2), MenuAction::Entry(Entry::ServoDelta(i))) },
                MenuButton { LabelWhenMultipleAreSelected: crate::il!(str "MotorisedHingeBehaviour::Start" 5), ..MenuButton::new("servoSetSpeed", crate::il!(str "MotorisedHingeBehaviour::Start" 5), crate::il!(str "MotorisedHingeBehaviour::Start" 6), MenuAction::Entry(Entry::ServoSpeed(i))) },
            ]),
            _ => return,
        };
        let p = p.unwrap_or_else(no_phys);
        self.physicals[p].menu.extend(buttons);
    }

    /// EditableTintBehaviour.Start and HeatingElementBehaviour.Start among the Starts that only add buttons (misc::Menu):
    /// the button; EditableTint's SetTint(Tint) property block and the heater's ActivationLight.enabled are rendering.
    pub(super) fn menu_start(&mut self, m: super::misc::Menu) {
        use super::misc::Menu;
        let p = match m {
            // LEDBulbBehaviour.Start: the button, then UpdateActivation and UpdateBulbMaskColour.
            Menu::Led(i) => {
                self.led_update_activation(i);
                self.led_update_bulb_mask_colour(i);
                self.leds[i].Phys
            }
            Menu::Liquidentifier(l) => self.liquidentifiers[l].physical,
            Menu::LiquidDuplicator(l) => self.liquid_duplicators[l].physical,
            Menu::LaserReceiver(r) => self.laser_receivers[r].physical,
            Menu::LiquidLevelGate(g) => self.liquid_level_gates[g].physical,
            Menu::KeyTrigger(k) => self.key_triggers[k].physical,
            Menu::Counter(c) => self.counters[c].physical,
            Menu::Pointer(i) => self.pointers[i].physical,
            Menu::Hovercar(h) => {
                self.hovercar_play_loops(h);
                self.hovercars[h].physical
            }
            Menu::HeatingElement(i) => self.heating_elements[i].Phys,
            Menu::Gate(g) => self.gates[g].physical,
            Menu::Invisible(i) => self.invisibles[i].physical.unwrap_or_else(|| npe()),
            Menu::Jukebox(j) => {
                self.jukebox_start_tracks(j);
                self.jukeboxes[j].physical
            }
            Menu::HoverThruster(h) => {
                self.hover_thruster_update_activation(h);
                self.hover_thrusters[h].physical
            }
            Menu::HeartMonitor(h) => {
                self.heart_monitor_update_activation(h);
                self.heart_monitors[h].physical
            }
            Menu::Display(d) => self.displays[d].physical,
            Menu::EditableTint(_, p) | Menu::ParticleMachine(_, p) => p,
            Menu::LiquidLeak(l) => self.liquid_leaks[l].physical,
        };
        // ParticleMachineBehaviour.Start: addToggleButton for smoke, fire and electricity.
        if let Menu::ParticleMachine(i, _) = m {
            for (k, id, desc) in [(0, "pmsmoke", "Toggle smoke"), (1, "pmfire", "Toggle fire"), (2, "pmelectricity", "Toggle electricity")] {
                self.physicals[p].menu.push(MenuButton { label: Label::ParticleMachine(i, k), ..MenuButton::new(id, "", desc, MenuAction::ParticleMachineToggle(i, k)) });
            }
            // UpdateActivation
            let m = self.particle_machine_of_slot(i);
            self.particle_machine_update_activation(m, false);
            return;
        }
        if let Menu::Jukebox(j) = m {
            self.physicals[p].menu.push(MenuButton::new("jukeboxNext", crate::il!(str "JukeboxBehaviour::Start" 1), crate::il!(str "JukeboxBehaviour::Start" 1), MenuAction::JukeboxSong(j, 1)));
            self.physicals[p].menu.push(MenuButton::new("jukeboxPrevious", crate::il!(str "JukeboxBehaviour::Start" 4), crate::il!(str "JukeboxBehaviour::Start" 4), MenuAction::JukeboxSong(j, -1)));
            return;
        }
        // InvisibleOptionInContextMenu.Start: Condition phys.spriteRenderer holds (Awake needed it), then UpdateVisibility.
        if let Menu::Invisible(i) = m {
            self.physicals[p].menu.push(MenuButton { label: Label::Invisible(i), ..MenuButton::new("toggleInvisibility", "", crate::il!(str "InvisibleOptionInContextMenu::Start" 1), MenuAction::InvisibleToggle(i)) });
            self.invisible_update(i);
            return;
        }
        // GateBehaviour.Start: AddRange of three.
        if let Menu::Gate(g) = m {
            self.physicals[p].menu.extend([
                MenuButton::new("setGateThreshold", crate::il!(str "GateBehaviour::Start" 1), crate::il!(str "GateBehaviour::Start" 2), MenuAction::Entry(Entry::GateThreshold(g))),
                MenuButton::new("setGateMaxPower", crate::il!(str "GateBehaviour::Start" 4), crate::il!(str "GateBehaviour::Start" 4), MenuAction::Entry(Entry::GateMaxPower(g))),
                MenuButton { label: Label::GateTrigger(g), LabelWhenMultipleAreSelected: crate::il!(str "GateBehaviour::Start" 8), ..MenuButton::new("toggleGateTriggerBehaviour", "", crate::il!(str "GateBehaviour::Start" 7), MenuAction::GateTrigger(g)) },
            ]);
            return;
        }
        // LiquidLevelGateBehaviour.Start: the trigger toggle, then AddRange of the threshold dialog.
        if let Menu::LiquidLevelGate(g) = m {
            self.physicals[p].menu.extend([
                MenuButton { label: Label::LiquidLevelGateTrigger(g), ..MenuButton::new("toggleTriggerBehaviour", "", crate::il!(str "LiquidLevelGateBehaviour::Start" 1), MenuAction::LiquidLevelGateTrigger(g)) },
                MenuButton::new("setLiquidGateThreshold", crate::il!(str "LiquidLevelGateBehaviour::Start" 3), crate::il!(str "LiquidLevelGateBehaviour::Start" 4), MenuAction::Entry(Entry::LiquidGate(g))),
            ]);
            return;
        }
        // CounterBehaviour.Start: the delay, then two buttons (the NixieTube value is rendering).
        if let Menu::Counter(c) = m {
            self.counter_start(c);
            self.physicals[p].menu.push(MenuButton::new("resetCounter", crate::il!(str "CounterBehaviour::Start" 1), crate::il!(str "CounterBehaviour::Start" 2), MenuAction::CounterReset(c)));
            self.physicals[p].menu.push(MenuButton::new("setCounterWrap", crate::il!(str "CounterBehaviour::Start" 4), crate::il!(str "CounterBehaviour::Start" 5), MenuAction::Entry(Entry::CounterWrap(c))));
            return;
        }
        // PointerBehaviour.Start: ActiveLight.enabled = Activated, then the button.
        if let Menu::Pointer(i) = m {
            self.pointer_light(i);
            self.physicals[p].menu.push(MenuButton { LabelWhenMultipleAreSelected: crate::il!(str "PointerBehaviour::Start" 1), ..MenuButton::new("pointerSetSpeed", crate::il!(str "PointerBehaviour::Start" 1), crate::il!(str "PointerBehaviour::Start" 2), MenuAction::Entry(Entry::PointerSpeed(i))) });
            return;
        }
        // HeartMonitorBehaviour.Start: UpdateActivation only (no menu button).
        if let Menu::HeartMonitor(_) = m {
            return;
        }
        // DisplayBehaviour.Start: the button, UpdateDisplay and UpdateActivation.
        if let Menu::Display(d) = m {
            self.physicals[p].menu.push(MenuButton::new("changeDisplayText", crate::il!(str "DisplayBehaviour::Start" 1), crate::il!(str "DisplayBehaviour::Start" 2), MenuAction::Entry(Entry::DisplayText(d))));
            self.display_start(d);
            return;
        }
        // KeyTriggerBehaviour.Start: the buttons, then UpdateDisplay.
        if let Menu::KeyTrigger(k) = m {
            self.physicals[p].menu.extend([
                MenuButton::new("changeKeyTrigger", crate::il!(str "KeyTriggerBehaviour::Start" 1), crate::il!(str "KeyTriggerBehaviour::Start" 2), MenuAction::KeyChange(k)),
                MenuButton::new("changeKeyTriggerDescription", crate::il!(str "KeyTriggerBehaviour::Start" 4), crate::il!(str "KeyTriggerBehaviour::Start" 5), MenuAction::Entry(Entry::KeyDescription(k))),
                MenuButton { label: Label::KeyTriggerDouble(k), LabelWhenMultipleAreSelected: crate::il!(str "KeyTriggerBehaviour::Start" 8), ..MenuButton::new("toggleKeyTriggerBehaviour", "", crate::il!(str "KeyTriggerBehaviour::Start" 7), MenuAction::KeyTriggerDouble(k)) },
            ]);
            self.key_trigger_update_display(k);
            return;
        }
        let b = match m {
            // !ColorpickerDialogBehaviour.IsOpen: no dialog is open while a menu is.
            Menu::Led(i) => MenuButton::new("setBulbColour", crate::il!(str "CathodeLightBehaviour::Start" 1), crate::il!(str "CathodeLightBehaviour::Start" 2), MenuAction::Entry(Entry::LedColour(i))),
            Menu::Liquidentifier(l) => MenuButton { condition: Cond::LiquidentifierIdle(l), ..MenuButton::new("flushLiquidentifier", "Flush", crate::il!(str "LiquidDuplicatorBehaviour::Start" 2), MenuAction::LiquidentifierFlush(l)) },
            Menu::LiquidDuplicator(l) => MenuButton { condition: Cond::LiquidDuplicatorIdle(l), ..MenuButton::new("flushLiquidentifier", "Flush", crate::il!(str "LiquidDuplicatorBehaviour::Start" 2), MenuAction::LiquidDuplicatorFlush(l)) },
            // "heatingElementThingWhateverWhoCaresAtThisPoint" + ActiveCooling (bool.ToString)
            Menu::HeatingElement(i) => MenuButton::new(
                if self.heating_elements[i].ActiveCooling { "heatingElementThingWhateverWhoCaresAtThisPointTrue" } else { "heatingElementThingWhateverWhoCaresAtThisPointFalse" },
                crate::il!(str "HeatingElementBehaviour::<Start>b__10_0" 0), crate::il!(str "HeatingElementBehaviour::<Start>b__10_0" 0), MenuAction::Entry(Entry::HeatTarget(i))),
            Menu::LiquidLeak(l) => MenuButton { condition: Cond::LiquidLeakHoles(l), ..MenuButton::new("repairLiquidHoles", crate::il!(str "LiquidContainerLeakingBehaviour::Start" 1), crate::il!(str "LiquidContainerLeakingBehaviour::Start" 2), MenuAction::LiquidLeakRepair(l)) },
            Menu::Gate(_) | Menu::ParticleMachine(..) | Menu::Jukebox(_) | Menu::Invisible(_) | Menu::LiquidLevelGate(_) | Menu::KeyTrigger(_) | Menu::Counter(_) | Menu::Pointer(_) | Menu::HeartMonitor(_) => unreachable!(),
            Menu::HoverThruster(h) => MenuButton::new("setHoverThrusterRange", crate::il!(str "HoverThrusterBehaviour::Start" 1), crate::il!(str "HoverThrusterBehaviour::Start" 1), MenuAction::Entry(Entry::HoverHeight(h))),
            Menu::Display(_) => unreachable!(),
            // !ColorpickerDialogBehaviour.IsOpen: no dialog is open while a menu is.
            Menu::EditableTint(i, _) => MenuButton::new("setEditableTint", crate::il!(str "CathodeLightBehaviour::Start" 1), crate::il!(str "EditableTintBehaviour::Start" 2), MenuAction::Entry(Entry::Tint(i))),
            Menu::Hovercar(h) => MenuButton { condition: Cond::HovercarEnabled(h), ..MenuButton::new("reverseCar", crate::il!(str "BoatMotorBehaviour::Start" 1), crate::il!(str "BoatMotorBehaviour::Start" 2), MenuAction::HovercarReverse(h)) },
            // LaserReceiverBehaviour.Start
            Menu::LaserReceiver(r) => MenuButton { label: Label::LaserReceiverTrigger(r), LabelWhenMultipleAreSelected: crate::il!(str "LaserReceiverBehaviour::Start" 2), ..MenuButton::new("toggleGateTriggerBehaviour", "", crate::il!(str "LaserReceiverBehaviour::Start" 1), MenuAction::LaserReceiverTrigger(r)) },
        };
        self.physicals[p].menu.push(b);
        // HeatingElementBehaviour.Start: after the button, ActivationLight.enabled = Activated.
        if let Menu::HeatingElement(i) = m {
            self.heating_element_light(i);
        }
        // EditableTintBehaviour.Start: after the button, SetTint(Tint).
        if let Menu::EditableTint(i, _) = m {
            self.editable_tint_set_tint(i, self.editable_tints[i]);
        }
        // LiquidContainerLeakingBehaviour.Start: after the button, the saved holes.
        if let Menu::LiquidLeak(l) = m {
            self.liquid_leak_start_holes(l);
        }
    }

    pub fn menu_label(&self, l: Label) -> String {
        match l {
            Label::Text(t) => t.into(),
            Label::Invisible(i) => (if self.invisibles[i].physical.is_some_and(|p| self.physicals[p].sprite_enabled) { crate::il!(str "InvisibleOptionInContextMenu::<Start>b__6_0" 1) } else { crate::il!(str "InvisibleOptionInContextMenu::<Start>b__6_0" 0) }).into(),
            Label::ParticleMachine(i, k) => format!("{}{}", if self.particle_machine_emit[i][k] { crate::il!(str "ParticleMachineBehaviour/<>c__DisplayClass9_0::<Start>b__7" 1) } else { crate::il!(str "ParticleMachineBehaviour/<>c__DisplayClass9_0::<Start>b__7" 0) }, ["smoke", "fire", "electricity"][k]),
            Label::GateTrigger(g) => if !self.gates[g].DoubleTrigger { crate::il!(str "GateBehaviour::<Start>b__17_4" 0) } else { crate::il!(str "GateBehaviour::<Start>b__17_4" 1) }.into(),
            Label::MagnetReversed(m) => if !self.tr.magnet_behaviour[m].Reversed { crate::il!(str "MagnetBehaviour::<Start>b__13_0" 0) } else { crate::il!(str "MagnetBehaviour::<Start>b__13_0" 1) }.into(),
            Label::BellNote(b, step) => self.universal_bell_label(b, step),
            Label::Bone(limb) => if self.limbs[limb].Broken { crate::il_v!(str "LimbBehaviour::<CreateContextMenuOptions>b__100_1" 1, "LimbBehaviour::<CreateContextMenuOptions>b__102_1" 1) } else { crate::il_v!(str "LimbBehaviour::<CreateContextMenuOptions>b__100_1" 0, "LimbBehaviour::<CreateContextMenuOptions>b__102_1" 0) }.into(),
            Label::MachineryIndestructible(m) => indestructible_label(self.machinery[m].Indestructible).into(),
            Label::DestroyableIndestructible(x) => indestructible_label(self.destroyables[x].Indestructible).into(),
            Label::SyringeInfinite(s) => if self.syringes[s].Finite { crate::il_v!(str "SyringeBehaviour::<AddButtons>b__17_7" 1, "SyringeBehaviour::<AddButtons>b__18_7" 1) } else { crate::il_v!(str "SyringeBehaviour::<AddButtons>b__17_7" 0, "SyringeBehaviour::<AddButtons>b__18_7" 0) }.into(),
            Label::ButtonTrigger(b) => trigger_label(self.buttons[b].TriggerOnExit).into(),
            Label::DetectorTrigger(i) => trigger_label(self.detectors[i].TriggerOnExit).into(),
            Label::CarBrake(c) => if self.cars[c].IsBrakeEngaged { crate::il!(str "CarBehaviour::<Start>b__21_0" 1) } else { crate::il!(str "CarBehaviour::<Start>b__21_0" 0) }.into(),
            Label::CarEngine(c) => if self.cars[c].Health > 0.0 { crate::il!(str "CarBehaviour::<Start>b__21_2" 1) } else { crate::il!(str "CarBehaviour::<Start>b__21_2" 0) }.into(),
            Label::TelevisionIndestructible(t) => indestructible_label(self.televisions[t].Indestructible).into(),
            Label::CathodeLightIndestructible(c) => indestructible_label(self.cathode_lights[c].Indestructible).into(),
            Label::TyreIndestructible(t) => indestructible_label(self.tyres[t].Indestructible).into(),
            Label::LiquidLevelGateTrigger(g) => trigger_label(self.liquid_level_gates[g].TriggerOnExit).into(),
            Label::KeyTriggerDouble(k) => trigger_label(self.key_triggers[k].DoubleTrigger).into(),
            Label::LaserReceiverTrigger(r) => if self.laser_receivers[r].DoubleTrigger { crate::il!(str "LaserReceiverBehaviour::<Start>b__6_0" 1) } else { crate::il!(str "LaserReceiverBehaviour::<Start>b__6_0" 0) }.into(),
            Label::Mute(m) => if self.mutes[m] { "Unmute" } else { "Mute" }.into(),
            Label::DetectorBeam(i) => if self.detectors[i].ShowBeam { crate::il!(str "DetectorBehaviour::<Start>b__22_3" 1) } else { crate::il!(str "DetectorBehaviour::<Start>b__22_3" 0) }.into(),
        }
    }

    pub fn menu_condition(&self, c: Cond) -> bool {
        match c {
            Cond::Always => true,
            Cond::SpeedometerRecord(s) => self.v45.speedometers[s].TopSpeedMs > 0.0,
            Cond::PoseIsNot(person, i) => self.people[person].OverridePoseIndex != i,
            Cond::MachineryRepair(m) => self.machinery[m].Destroyed && self.machinery[m].CanRepair,
            Cond::MachineryShielded(m, s) => {
                let mc = &self.machinery[m];
                !mc.Indestructible && mc.HeatShielded == s && mc.TemperatureThreshold != f32::INFINITY
            }
            Cond::MachineryToggle(m) => self.machinery[m].IndestructibilityCanBeToggled,
            Cond::DestroyableToggle(x) => self.destroyables[x].IndestructibilityCanBeToggled,
            Cond::SyringeModeIsNot(s, m) => self.syringes[s].PressureMode != m && self.syringes[s].Finite,
            Cond::SyringeCanToggle(s) => self.syringes[s].CanToggleInfinite,
            Cond::LiquidentifierIdle(l) => !self.liquidentifiers[l].isBusy,
            Cond::LiquidDuplicatorIdle(l) => !self.liquid_duplicators[l].isBusy,
            Cond::HovercarEnabled(h) => self.hovercars[h].enabled,
            Cond::BellNote(b, step) => {
                let i = self.universal_bells[b].BellClipIndex;
                if step > 0 { i < self.bell_clips.len() as i32 - 1 } else { i > 0 }
            }
            Cond::WingNot(a, i) => self.airfoils[a].CurrentWingTypeName != self.airfoils[a].WingTypes[i].Name,
            Cond::LiquidLeakHoles(l) => !self.liquid_leaks[l].holes.is_empty(),
            Cond::BloodTankNot(t, m) => self.blood_tanks[t].Mode != m,
            Cond::LiquidPumpNot(l, m) => self.liquid_pumps[l].Mode != m,
            Cond::TelevisionBroken(t) => self.televisions[t].Broken,
            Cond::CathodeLightBroken(c) => self.cathode_lights[c].Broken,
            Cond::TyreDeflated(t) => self.tyres[t].Deflated,
        }
    }

    /// PhysicalBehaviour.Start's two buttons.
    pub(super) fn physical_add_menu_buttons(&mut self, p: usize) {
        let menu = &mut self.physicals[p].menu;
        menu.push(MenuButton::new("setTemperature", crate::il!(str "PhysicalBehaviour::Start" 2), crate::il!(str "PhysicalBehaviour::Start" 3), MenuAction::Entry(Entry::Temperature(p))));
        menu.push(MenuButton::new("setAngle", crate::il!(str "PhysicalBehaviour::Start" 5), crate::il!(str "PhysicalBehaviour::Start" 6), MenuAction::Entry(Entry::Angle(p))));
    }

    /// LimbBehaviour.CreateContextMenuOptions
    pub(crate) fn limb_create_context_menu_options(&mut self, l: usize) {
        let (p, person) = (self.limbs[l].PhysicalBehaviour, self.limbs[l].Person);
        let menu = &mut self.physicals[p].menu;
        menu.push(MenuButton::new("inspectLimb", "Inspect", "Inspect", MenuAction::InspectLimb(l)));
        menu.push(MenuButton { label: Label::Bone(l), ..MenuButton::new("breakBones", "", crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 4), MenuAction::ToggleBone(l)) });
        for (cond, id, label, desc, multi, pose) in [
            (-1, "clearOverride", crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 6), crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 7), crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 8), -1),
            (3, "startStumbling", "Stumble", crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 11), "Stumble", 3),
            (6, "startWalking", "Walk", crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 15), "Walk", 6),
            (1, "startProtect", "Cower", crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 19), "Cower", 1),
            (7, "startSit", "Sit", crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 23), "Sit", 7),
            (8, "startPetrified", crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 26), crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 27), crate::il!(str "LimbBehaviour::CreateContextMenuOptions" 26), 8),
        ] {
            menu.push(MenuButton { condition: Cond::PoseIsNot(person, cond), LabelWhenMultipleAreSelected: multi, ..MenuButton::new(id, label, desc, MenuAction::Pose(person, pose)) });
        }
    }

    /// ContextMenuBehaviour.CreateDynamicButtons and ContextMenuBehaviour.MergeButtons over the selection.
    pub fn context_menu_buttons(&self) -> Vec<ShownButton> {
        let Some(t) = self.tool.as_ref() else { return Vec::new() };
        let mut shown: Vec<ShownButton> = Vec::new();
        for &p in &t.rawSelection {
            for b in &self.physicals[p].menu {
                if !self.menu_condition(b.condition) {
                    continue;
                }
                match shown.iter_mut().find(|s| s.Identity == b.Identity) {
                    Some(s) => {
                        s.actions.extend(b.actions.iter().copied());
                        s.getter = Label::Text(b.LabelWhenMultipleAreSelected);
                        s.label = b.LabelWhenMultipleAreSelected.into();
                    }
                    None => shown.push(ShownButton { Identity: b.Identity, label: self.menu_label(b.label), getter: b.label, Description: b.Description, actions: b.actions.clone() }),
                }
            }
        }
        crate::ui::uifn("ContextMenuBehaviour.MergeButtons", || format!("{} {}", shown.len(), shown.iter().map(|s| s.Identity).collect::<Vec<_>>().join("|")));
        shown
    }

    /// A Unity message callback (Update, FixedUpdate, ...): a managed exception thrown inside ends the callback, is
    /// logged (Debug.LogException) and the frame goes on with the next one. What the callback changed before the
    /// throw stays, as in the game.
    pub(super) fn unity_callback(&mut self, f: impl FnOnce(&mut Game)) {
        let r = catch_managed(|| f(self));
        self.log_exception(r);
    }

    /// Debug.LogException for what `catch_managed` caught.
    pub(super) fn log_exception(&mut self, caught: Option<ManagedException>) {
        if let Some(ex) = caught {
            println!("exception	{}", ex.0);
            self.log_message_received(&ex.0, LogType::Exception);
        }
    }

    /// The button's onClick: each action in turn.
    /// An exception thrown inside a button's action: within a click, UnityEvent.Invoke has no catch, so it ends the
    /// click (the later actions and ContextMenuBehaviour.Hide do not run) and ExecuteEvents logs it. Elsewhere it is
    /// not rewritten.
    pub(super) fn menu_throw(&mut self, what: &str) {
        if !self.menu_clicking {
            // Outside a click the exception leaves the callback it was thrown in, which Unity logs.
            throw(what);
        }
        // Debug.LogException
        println!("exception	{what}");
        self.log_message_received(what, LogType::Exception);
        self.menu_threw = true;
    }

    /// Application.logMessageReceived (no managed stack trace: Unity's empty one): DiskLogger's listener, then
    /// NotificationControllerBehaviour's.
    pub fn log_message_received(&mut self, condition: &str, ty: LogType) {
        if let Some(l) = self.disk_logger.as_mut() {
            l.log_message_received(condition, "", ty);
        }
        self.notification_log_message_received(condition, "", ty);
    }

    /// NotificationControllerBehaviour.LogMessageReceived: with LogDebugMessages on, the message is shown as a notification
    /// in the colour of its type (the stack trace is not shown).
    pub fn notification_log_message_received(&mut self, condition: &str, _stack_trace: &str, ty: LogType) {
        if self.prefs.LogDebugMessages {
            let colour = match ty {
                LogType::Error => crate::il!(str "NotificationControllerBehaviour::LogMessageReceived" 1),
                LogType::Assert => crate::il!(str "NotificationControllerBehaviour::LogMessageReceived" 2),
                LogType::Warning => crate::il!(str "NotificationControllerBehaviour::LogMessageReceived" 3),
                LogType::Log => crate::il!(str "NotificationControllerBehaviour::LogMessageReceived" 4),
                LogType::Exception => crate::il!(str "NotificationControllerBehaviour::LogMessageReceived" 5),
            };
            self.notifications.push(format!("<color=\"{colour}\">{condition}</color>"));
        }
    }

    /// ContextMenuBehaviour.CreateButton (the listeners) and Button.onClick: the button's actions in order, then Hide. Returns false when an action threw, and the menu
    /// stays open.
    pub fn context_menu_click(&mut self, actions: &[MenuAction]) -> bool {
        self.menu_clicking = true;
        self.menu_threw = false;
        for &a in actions {
            if self.menu_threw {
                break;
            }
            match a {
                MenuAction::Entry(e) => self.entry_open(e, actions),
                // KeyField.SetKey = KeyCode, EditOnAwake: its Start starts waiting.
                MenuAction::KeyChange(k) => self.key_boxes.push(KeyBox { trigger: k, key: self.key_triggers[k].KeyCode, wait: Some(KeyWait::new(self.key_triggers[k].KeyCode, self.input.screen.map(|s| s.mouse))) }),
                MenuAction::ToggleBone(l) => {
                    if self.limbs[l].Broken {
                        self.limb_heal_bone(l);
                    } else {
                        self.break_bone(l);
                    }
                }
                MenuAction::Pose(person, i) => self.people[person].OverridePoseIndex = i,
                MenuAction::SpeedometerReset(s) => self.v45.speedometers[s].TopSpeedMs = 0.0,
                MenuAction::BuzzerChooseAudio(b) => self.buzzer_choose_audio(b),
                MenuAction::MachineryRepair(m) => {
                    self.machinery[m].Health = self.machinery[m].initialHealth;
                    self.machinery_act_on_repair(m);
                }
                MenuAction::MachineryShield(m, s) => self.machinery[m].HeatShielded = s,
                MenuAction::MachineryToggle(m) => {
                    self.machinery[m].Indestructible = !self.machinery[m].Indestructible;
                    if self.machinery[m].Indestructible {
                        self.machinery[m].Health = self.machinery[m].initialHealth;
                        self.machinery_act_on_repair(m);
                    }
                }
                MenuAction::Skin(r, step) => {
                    self.random_sprites[r].chosenIndex = self.random_sprites[r].chosenIndex.wrapping_add(step);
                    self.random_sprite_set_to_index(r);
                }
                MenuAction::SyringeMode(s, m) => self.syringes[s].PressureMode = m,
                MenuAction::SyringeInfinite(s) => {
                    self.syringes[s].Finite = !self.syringes[s].Finite;
                    self.syringes[s].PressureMode = 0;
                }
                MenuAction::DestroyableToggle(x) => self.destroyables[x].Indestructible = !self.destroyables[x].Indestructible,
                MenuAction::ButtonTrigger(b) => self.buttons[b].TriggerOnExit = !self.buttons[b].TriggerOnExit,
                MenuAction::DetectorTrigger(i) => self.detectors[i].TriggerOnExit = !self.detectors[i].TriggerOnExit,
                MenuAction::CarReverse(c) => self.car_reverse_gear(c),
                MenuAction::CarBrake(c) => self.cars[c].IsBrakeEngaged = !self.cars[c].IsBrakeEngaged,
                MenuAction::CarEngine(c) => self.car_repair_engine(c),
                MenuAction::TelevisionRepair(t) => self.television_repair(t),
                MenuAction::CathodeLightRepair(c) => self.cathode_light_repair(c),
                MenuAction::CathodeLightToggle(c) => self.cathode_light_toggle_indestructible(c),
                MenuAction::TelevisionToggle(t) => {
                    self.televisions[t].Indestructible = !self.televisions[t].Indestructible;
                    if self.televisions[t].Indestructible && self.televisions[t].Broken {
                        self.television_repair(t);
                    }
                }
                MenuAction::TyreRepair(t) => self.tyre_repair(t),
                MenuAction::TyreToggle(t) => {
                    self.tyres[t].Indestructible = !self.tyres[t].Indestructible;
                    if self.tyres[t].Indestructible {
                        self.tyre_repair(t);
                    }
                }
                MenuAction::LiquidentifierFlush(l) => {
                    let id = self.liquidentifiers[l].id;
                    self.start_coroutine(id, id, Self::liquidentifier_flush(l));
                }
                MenuAction::LiquidDuplicatorFlush(l) => {
                    let id = self.liquid_duplicators[l].id;
                    self.start_coroutine(id, id, Self::liquid_duplicator_flush(l));
                }
                // With several selected only the first selected bell's delegate goes on; copy = every selected bell.
                MenuAction::BellPiano(b) => {
                    let bells: Vec<usize> = actions.iter().filter_map(|a| if let MenuAction::BellPiano(x) = a { Some(*x) } else { None }).collect();
                    if bells[0] != b {
                        continue;
                    }
                    // PianoDialogBox.Show: SelectedBellClip = startClip and the apply callback (`targets`); the dialog is active
                    self.piano = Some(Piano { selected: PianoPick::Bell(self.universal_bells[b].BellClipIndex as usize), targets: bells });
                }
                MenuAction::BellNote(b, step) => {
                    let i = self.universal_bells[b].BellClipIndex + step;
                    self.universal_bell_set_index(b, i);
                    self.universal_bell_use(b);
                }
                MenuAction::InvisibleToggle(i) => {
                    self.invisibles[i].Invisible = !self.invisibles[i].Invisible;
                    self.invisible_update(i);
                }
                MenuAction::JukeboxSong(j, o) => self.jukebox_song(j, o),
                MenuAction::ParticleMachineToggle(i, k) => {
                    self.particle_machine_emit[i][k] = !self.particle_machine_emit[i][k];
                    let m = self.particle_machine_of_slot(i);
                    self.particle_machine_update_activation(m, false);
                }
                MenuAction::WingSet(a, i) => self.airfoil_set_wing_type(a, i),
                MenuAction::GateTrigger(g) => self.gates[g].DoubleTrigger = !self.gates[g].DoubleTrigger,
                MenuAction::LiquidLeakRepair(l) => self.liquid_leak_repair(l),
                MenuAction::BoatReverse(b) => self.boat_motor_behaviour_reverse(b),
                MenuAction::RotorReverse(r) => {
                    let speed = self.tr.rotor_behaviour[r].Speed;
                    self.rotor_behaviour_set_speed(r, 0.0 - speed);
                }
                MenuAction::MagnetReverse(m) => self.tr.magnet_behaviour[m].Reversed = !self.tr.magnet_behaviour[m].Reversed,
                MenuAction::BloodTankMode(t, m) => {
                    self.blood_tanks[t].Mode = m;
                    self.blood_tank_update_activation(t);
                }
                MenuAction::LiquidPumpMode(l, m) => self.liquid_pump_set_mode(l, m),
                MenuAction::CarTexture(t, step) => {
                    self.random_car_textures[t].chosenIndex += step;
                    self.random_car_texture_set_to_index(t);
                }
                MenuAction::HovercarReverse(h) => self.hovercars[h].BaseSpeed = 0.0 - self.hovercars[h].BaseSpeed,
                MenuAction::LiquidLevelGateTrigger(g) => self.liquid_level_gates[g].TriggerOnExit = !self.liquid_level_gates[g].TriggerOnExit,
                MenuAction::KeyTriggerDouble(k) => self.key_triggers[k].DoubleTrigger = !self.key_triggers[k].DoubleTrigger,
                MenuAction::CounterReset(c) => self.counter_reset(c),
                MenuAction::LaserReceiverTrigger(r) => self.laser_receivers[r].DoubleTrigger = !self.laser_receivers[r].DoubleTrigger,
                MenuAction::Mute(m) => self.mute_set(m, !self.mutes[m]),
                MenuAction::DetectorBeam(i) => {
                    self.detectors[i].ShowBeam = !self.detectors[i].ShowBeam;
                    self.detector_update_activated(i);
                }
                // Limbs = every selected object's LimbBehaviour (each selected limb runs the same delegate).
                MenuAction::InspectLimb(_) => {
                    self.limb_view.limbs = actions.iter().filter_map(|a| if let MenuAction::InspectLimb(l) = a { Some(*l) } else { None }).collect();
                    self.limb_view.active = true;
                }
                MenuAction::NotYet(class) => unimplemented!("mod-only: {class}: its context menu buttons"),
            }
        }
        self.menu_clicking = false;
        !self.menu_threw
    }
}

/// A managed exception (NullReferenceException, IndexOutOfRangeException, ...) unwinding to its Unity callback.
pub struct ManagedException(pub String);

thread_local! {
    /// How many `Game::unity_callback`s the current call is inside.
    static CATCHING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Runs one Unity message callback's body: a managed exception thrown inside ends it and is returned. For a body that
/// borrows the game itself (`unity_callback` hands it the game instead).
pub(super) fn catch_managed(f: impl FnOnce()) -> Option<ManagedException> {
    CATCHING.with(|c| c.set(c.get() + 1));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CATCHING.with(|c| c.set(c.get() - 1));
    match r {
        Ok(()) => None,
        Err(e) => match e.downcast::<ManagedException>() {
            Ok(ex) => Some(*ex),
            Err(e) => std::panic::resume_unwind(e),
        },
    }
}

/// A NullReferenceException.
#[track_caller]
pub fn npe() -> ! {
    throw("NullReferenceException: Object reference not set to an instance of an object")
}

/// Throws the managed exception `what` (e.g. "NullReferenceException: Object reference not set to an instance of an
/// object"). Outside any callback that catches it, it stays a stop.
#[track_caller]
pub fn throw(what: impl Into<String>) -> ! {
    let what = what.into();
    if std::env::var_os("PPG_THROW_AT").is_some() {
        eprintln!("throw at {}: {what}", std::panic::Location::caller());
    }
    if CATCHING.with(|c| c.get()) == 0 {
        unimplemented!("mod-only: {what} (thrown outside a rewritten Unity callback)");
    }
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<ManagedException>().is_none() {
                default(info);
            }
        }));
    });
    std::panic::panic_any(ManagedException(what))
}

#[cfg(test)]
mod throw_tests {
    #[test]
    fn a_thrown_exception_ends_only_its_callback() {
        let mut g = super::Game::default();
        let mut after = false;
        g.unity_callback(|_| super::throw("NullReferenceException"));
        g.unity_callback(|_| after = true);
        assert!(after);
        assert!(std::panic::catch_unwind(|| super::throw("x")).is_err());
    }

    #[test]
    fn a_start_that_throws_ends_only_itself() {
        use super::super::misc::PendingStart;
        let mut g = super::Game::default();
        g.pending_starts = vec![PendingStart::Throw("NullReferenceException".into()), PendingStart::BloodPuffSound];
        let before = g.random.clone();
        g.run_pending_starts();
        assert!(g.pending_starts.is_empty());
        assert!(g.random != before, "the Start after the throwing one ran");
    }
}

#[cfg(test)]
mod texturepack_tests {
    #[test]
    fn texturepack_initialisation_defaults() {
        let t = super::TexturepackInitialisation::new();
        assert_eq!(t.Path, "TexturePacks");
        assert_eq!(t.MetaFileName, None);
    }
}

#[cfg(test)]
mod piano_tests {
    use super::{Piano, PianoPick};

    #[test]
    fn only_the_selected_key_is_lit() {
        let mut p = Piano { selected: PianoPick::Key(3), targets: Vec::new() };
        assert!(p.key_lit(3) && !p.key_lit(4));
        p.selected = PianoPick::Bell(3);
        assert!(!p.key_lit(3));
    }
}
