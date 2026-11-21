//! The wires: BloodWireBehaviour and the other DistanceJointWireBehaviours with the WireBehaviour and Hover they derive
//! from, SpringCableBehaviour and AppliedBandageBehaviour, and the DistanceWireTool and SpringJointWireTool that lay them.
//!
//! The FixedWireTool family (FixedCableTool, SteelBindingTool, WoodenBindingTool) and WoodStrutTool lay wires here too: a
//! FixedJointWireBehaviour's line is local to its body, so `vertices` hold its local points.
//!
//! The tool drags a wire from the selected object to the object under the mouse: OnDeselect adds a DistanceJoint2D
//! (maxDistanceOnly, enableCollision) between them and a BloodWireBehaviour, registered in the undo history as
//! "liquid conduit". The wire's FixedUpdate moves liquid from the higher-pressure container to the other and averages
//! the pressures; a wire between a CardiopulmonaryBypassMachineBehaviour and a living limb sets the limb's
//! ArtificialHeartbeat and BloodFlow every frame, and one to a HeartMonitorBehaviour feeds it the connected limb.
//!
//! Presentation, skipped: the LineRenderers (the tool's and the wire's: positions, colours, width, material, flow and
//! blood-colour property block, the stress colour that needs ShowLimbStatus), the machine's sounds and status text (and
//! the 3 s `flash` coroutine that only writes text), the Wire child's GameObject (4 instance ids are taken), Stats and
//! the modding events. DistanceJointWireBehaviour.Start of a wire without a typedJoint (a saved wire) is not reachable. Hover.CurrentlyHovering is only read by the context
//! menu.
#![allow(non_snake_case)]

use super::coroutine::Yield;
use super::container::Owner;
use super::undo::ObjectCreationAction;
use super::{d, mathf, BodyOwner, Behaviour, Game};

/// Which DistanceJointWireBehaviour a wire is, and so which DistanceWireTool lays it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WireKind {
    #[default]
    Blood,
    Cable,
    Rope,
    Chain,
    Copper,
    Energy,
    HeatPipe,
    Rigid,
    Spring,
    StrongSpring,
    DestructibleSpring,
    Bandage,
    WoodStrut,
    FixedCable,
    SteelBinding,
    WoodenBinding,
    /// SliderTool: a SliderJoint2D and a SliderJointWireBehaviour.
    Slider,
    /// UseWireTool and its Red, Blue and Destructible kin: the channel, and whether it can break.
    Use(u16, bool),
    /// NewUseWireBehaviour: no tool lays it; a prefab's own.
    NewUse,
}

impl WireKind {
    /// The tool class's name (ToolControllerBehaviour.SetToolByTypeName) as a kind.
    pub fn of_tool(name: &str) -> Option<Self> {
        Some(match name {
            "BloodWireTool" => Self::Blood,
            "CableWireTool" => Self::Cable,
            "RopeTool" => Self::Rope,
            "ChainWireTool" => Self::Chain,
            "CopperWireTool" => Self::Copper,
            "EnergyWireTool" => Self::Energy,
            "HeatPipeTool" => Self::HeatPipe,
            "RigidWireTool" => Self::Rigid,
            "SpringWireTool" => Self::Spring,
            "StrongSpringWireTool" => Self::StrongSpring,
            "DestructibleSpringWireTool" => Self::DestructibleSpring,
            "BandageWireTool" => Self::Bandage,
            "WoodStrutTool" => Self::WoodStrut,
            "FixedCableTool" => Self::FixedCable,
            "SteelBindingTool" => Self::SteelBinding,
            "WoodenBindingTool" => Self::WoodenBinding,
            "SliderTool" => Self::Slider,
            "UseWireTool" => Self::Use(0, false),
            "RedUseWireTool" => Self::Use(1, false),
            "BlueUseWireTool" => Self::Use(2, false),
            "DestructibleUseWireTool" => Self::Use(0, true),
            "DestructibleRedUseWireTool" => Self::Use(1, true),
            "DestructibleBlueUseWireTool" => Self::Use(2, true),
            _ => return None,
        })
    }

    /// The tool's Awake: WireMaterial.
    pub fn material(self) -> &'static str {
        match self {
            Self::Blood => "Materials/BloodWire",
            Self::Cable => "Materials/Cable",
            Self::Rope => "Materials/Rope",
            Self::Chain => "Materials/ChainWire",
            Self::Copper => "Materials/CopperWire",
            Self::Energy => "Materials/Wire",
            Self::HeatPipe => "Materials/HeatPipe",
            Self::Rigid => "Materials/RigidWire",
            Self::Spring => "Materials/SpringCable",
            Self::StrongSpring => "Materials/StrongSpringCable",
            Self::DestructibleSpring => "Materials/DestructibleSpringCable",
            Self::Bandage => "Materials/BandageWire",
            Self::WoodStrut => "Materials/WoodStrut",
            Self::FixedCable => "Materials/FixedCable",
            Self::SteelBinding => "Materials/SteelBinding",
            Self::WoodenBinding => "Materials/WoodenBinding",
            Self::Slider => "Materials/SlidePiston",
            Self::Use(_, false) => "Materials/LogicWire",
            Self::NewUse => unimplemented!("mod-only: NewUseWireBehaviour is laid by no tool"),
            Self::Use(_, true) => "Materials/DestructibleLogicWire",
        }
    }

    /// The tool's Awake: WireWidth.
    pub fn width(self) -> f32 {
        match self {
            Self::Chain | Self::HeatPipe | Self::SteelBinding | Self::WoodenBinding => 0.1,
            // SliderTool.Awake: WireWidth = 0.1f.
            Self::Slider => 0.1,
            Self::Rigid | Self::WoodStrut => 0.08,
            Self::FixedCable => 0.06,
            Self::Spring | Self::StrongSpring | Self::DestructibleSpring => 0.2,
            Self::Bandage => 0.07,
            _ => DISTANCE_WIRE_WIDTH,
        }
    }

    /// The tool's Awake: WireColor.
    pub fn color(self) -> [f32; 4] {
        match self {
            Self::Energy => [0.0, 0.0, 0.03, 1.0],
            // SliderTool never assigns WireColor: the default Color (0, 0, 0, 0).
            Self::Slider => [0.0; 4],
            _ => [1.0; 4],
        }
    }

    /// The ToolController's LineRenderer as the kind's OnToolChosen leaves it: numCapVertices, Stretch (else Tile) and
    /// sortingOrder. DistanceWireTool/FixedWireTool.OnToolChosen: 6, Tile, 2; WoodStrutTool.OnToolChosen: numCapVertices 0;
    /// SpringJointWireTool's and SliderTool's: 0 and Stretch, BandageWireTool's Tile and sortingOrder 3.
    pub fn tool_line(self) -> (usize, bool, i16) {
        let caps = if self.is_spring() || self.is_slider() || self == Self::WoodStrut { 0 } else { 6 };
        (caps, (self.is_spring() && self != Self::Bandage) || self.is_slider(), if self == Self::Bandage { 3 } else { 2 })
    }

    /// A SpringJointWireTool's: a SpringJoint2D and a SpringCableBehaviour.
    pub fn is_spring(self) -> bool {
        matches!(self, Self::Spring | Self::StrongSpring | Self::DestructibleSpring | Self::Bandage)
    }

    /// SliderTool's: a SliderJoint2D and a SliderJointWireBehaviour.
    pub fn is_slider(self) -> bool {
        self == Self::Slider
    }

    /// A FixedWireTool's: a FixedJoint2D and a FixedJointWireBehaviour.
    pub fn is_fixed(self) -> bool {
        matches!(self, Self::FixedCable | Self::SteelBinding | Self::WoodenBinding)
    }

    /// SliderJointWireBehaviour.GetVertexCount: 2 (`vertices` hold three points).
    /// RigidWireBehaviour.GetVertexCount (1), SteelBindingBehaviour.GetVertexCount, WoodStrutBehaviour.GetVertexCount, HeatPipeBehaviour.GetVertexCount and ChainBehaviour.GetVertexCount:
    /// the line has one more point (1, or 12 for the chain).
    /// WireBehaviour.GetVertexCount (12), and a DistanceJointWireBehaviour's: the line has one more point.
    pub fn vertex_count(self) -> usize {
        // WoodStrutBehaviour.GetVertexCount: 1 (the WoodStrut arm below).
        // WireBehaviour.GetVertexCount: 12 (the `_` arm).
        // ChainBehaviour.GetVertexCount: 12 (the `_` arm).
        // SteelBindingBehaviour.GetVertexCount: 1 (is_fixed).
        // RigidWireBehaviour.GetVertexCount: 1.
        // HeatPipeBehaviour.GetVertexCount: 1.
        match self {
            Self::HeatPipe | Self::Rigid | Self::WoodStrut => 1,
            Self::Slider => 2,
            _ if self.is_fixed() => 1,
            _ => 12,
        }
    }

    /// LineRenderer.numCapVertices: 6, set to 0 by ChainBehaviour.Start, SpringCableBehaviour.Start and the Created of the
    /// bindings and the strut.
    pub fn cap_vertices(self) -> usize {
        if matches!(self, Self::Chain | Self::WoodStrut | Self::SteelBinding | Self::WoodenBinding | Self::Slider) || self.is_spring() { 0 } else { 6 }
    }

    /// The name in the undo history.
    fn display_name(self) -> &'static str {
        match self {
            Self::Blood => crate::il!(str "BloodWireTool::OnJointCreate" 0),
            Self::Cable => "cable",
            Self::Rope => "rope",
            Self::Chain => "chain",
            Self::Copper => crate::il!(str "CopperWireTool::OnJointCreate" 0),
            Self::Energy => crate::il!(str "EnergyWireTool::OnJointCreate" 0),
            Self::HeatPipe => crate::il!(str "HeatPipeTool::OnJointCreate" 0),
            Self::Rigid => crate::il!(str "RigidWireTool::OnJointCreate" 0),
            Self::Spring | Self::DestructibleSpring => crate::il!(str "DestructibleSpringWireTool::OnJointCreate" 0),
            Self::StrongSpring => crate::il!(str "StrongSpringWireTool::OnJointCreate" 0),
            Self::Bandage => "bandage",
            Self::WoodStrut => crate::il!(str "WoodStrutTool::OnJointCreate" 0),
            Self::FixedCable => crate::il!(str "FixedCableTool::OnJointCreate" 0),
            Self::SteelBinding => crate::il!(str "SteelBindingTool::OnJointCreate" 0),
            Self::WoodenBinding => crate::il!(str "WoodenBindingTool::OnJointCreate" 0),
            Self::Slider => crate::il!(str "SliderTool::OnJointCreate" 0),
            Self::Use(..) => crate::il!(str "UseWireTool::CreateWire" 0),
            Self::NewUse => unimplemented!("mod-only: NewUseWireBehaviour is laid by no tool"),
        }
    }

    /// Utils.CalculateBreakForceForCable's baseStrength, referenceMass and massInfluence in OnJointCreate.
    fn break_strength(self) -> Option<(f32, f32, f32)> {
        match self {
            Self::Rope => Some((350.0, 1.0, 0.5)),
            Self::Copper => Some((250.0, 1.0, 0.5)),
            Self::DestructibleSpring => Some((850.0, 1.0, 0.5)),
            Self::WoodStrut => Some((700.0, 1.0, 1.0)),
            Self::SteelBinding => Some((6000.0, 5.0, 1.0)),
            Self::WoodenBinding => Some((4000.0, 5.0, 1.0)),
            // DestructibleUseWireTool.GetBreakForce; UseWireTool's is PositiveInfinity (set on the joint all the same: it
            // may be another wire's).
            Self::Use(_, true) => Some((850.0, 1.0, 0.5)),
            Self::Use(_, false) => Some((f32::INFINITY, 1.0, 0.5)),
            _ => None,
        }
    }

    /// The sounds JointBroken picks from: rope snap, copper wire break and spring_snap hold 3 each, steel break
    /// STEEL_BREAK_CLIPS, wooden binding break 6, Global.WoodStutSnapClips 9.
    /// The folder (None: Global.WoodStutSnapClips) and the count.
    fn break_clips(self) -> Option<(Option<&'static str>, usize)> {
        Some(match self {
            Self::SteelBinding => (Some(crate::il!(str "SteelBindingBehaviour::Created" 1)), STEEL_BREAK_CLIPS),
            Self::WoodenBinding => (Some(crate::il!(str "WoodenBindingBehaviour::JointBroken" 0)), 6),
            Self::Use(..) | Self::NewUse => (Some(crate::il!(str "NewUseWireBehaviour::JointBroken" 0)), super::use_wire::USE_WIRE_BREAK_CLIPS),
            Self::WoodStrut => (None, 9),
            Self::Rope => (Some(crate::il!(str "RopeBehaviour::JointBroken" 0)), 3),
            Self::Copper => (Some(crate::il!(str "CopperWireBehaviour::Start" 0)), 3),
            _ if self.is_spring() => (Some("Audio/spring_snap"), 3),
            _ => return None,
        })
    }

    /// SpringCableBehaviour.SpringStrength, the joint's frequency.
    fn spring_strength(self) -> f32 {
        if self == Self::StrongSpring { 8.0 } else { 2.0 }
    }
}

/// Resources.LoadAll<AudioClip>("Audio/metal stress/"), "Audio/steel break", "Audio/wooden binding stress/".
const METAL_STRESS_CLIPS: usize = 4;
const STEEL_BREAK_CLIPS: usize = 9;
const WOODEN_STRESS_CLIPS: usize = 3;
/// Global.WoodStrutStressClips
const WOOD_STRUT_STRESS_CLIPS: usize = 9;

/// One DistanceJointWireBehaviour or FixedJointWireBehaviour added by a tool.
#[derive(Clone, Debug)]
pub struct BloodWire {
    pub kind: WireKind,
    /// WireColor, and the LineRenderer's startColor and endColor.
    pub WireColor: [f32; 4],
    pub start_color: [f32; 4],
    pub end_color: [f32; 4],
    /// FixedJointWireBehaviour.localRenderEndpoint
    pub localRenderEndpoint: [f32; 2],
    /// FixedJointWireBehaviour.originalRotation and hasMovedInPauseMode
    pub originalRotation: f32,
    pub hasMovedInPauseMode: bool,
    /// FixedJointWireBehaviour.otherEnd: Start added a CableOtherEndBehaviour (Connected = this wire) to
    /// otherPhysicalBehaviour's object; Destroy(otherEnd) goes with the wire.
    pub otherEnd: bool,
    /// Rope and copper: breakForce before burning.
    pub breakForceBuffer: f32,
    /// HeatPipeBehaviour.fakeCableTemperature
    pub fakeCableTemperature: f32,
    /// GetVertexCount
    pub vertex_count: usize,
    /// AppliedBandageBehaviour.limb, with the limb's bloodColour.
    pub limb: Option<usize>,
    pub bloodColour: [f32; 4],
    /// AppliedBandageBehaviour.SoakAmount
    pub SoakAmount: f32,
    /// AppliedBandageBehaviour.SoakColour (1.28.45; white at first).
    pub SoakColour: [f32; 4],
    /// The component's instance id.
    pub id: i32,
    /// physicalBehaviour: the PhysicalBehaviour on its GameObject (GetComponent in Awake).
    pub physical: usize,
    /// otherPhysicalBehaviour: that of the joint's connectedBody, set in Start.
    pub other: Option<usize>,
    /// typedJoint / untypedJoint: the `Game::fixed_joints` entry of the DistanceJoint2D.
    pub joint: usize,
    /// WireWidth (the tool's 0.05).
    pub WireWidth: f32,
    /// lineRenderer.widthMultiplier when a recording's wire record holds it (`lineWidth`); None: WireWidth, what
    /// Initialise set it to (only Initialise and SetThickness write it, so a WireWidth changed later does not reach it).
    pub line_width: Option<f32>,
    /// Hover.Invisible
    pub Invisible: bool,
    /// Start ran (Update and FixedUpdate wait for it).
    pub started: bool,
    pub usedToHaveConnectedBody: bool,
    /// DistanceJointWireBehaviour.distanceBeforeFlip
    pub distanceBeforeFlip: f32,
    pub shouldIgnoreUpdate: bool,
    pub IsMouseInsideBounds: bool,
    pub IsMouseOverlapping: bool,
    /// WireBehaviour.InvisibilityTracker
    pub tracker: super::tool_invisibility::InvisibilityTracker,
    /// The wire's vertices; the LineRenderer holds the same positions.
    pub vertices: Vec<[f32; 3]>,
    // BloodWireBehaviour.OnWillRenderObject: drawn in wires.rs `wires` (_Flow = clamp(flow, -1, 1) and _BloodColor = color on the line's BloodWire material).
    /// flow and color: only OnWillRenderObject reads them, for the shader.
    pub flow: f32,
    pub color: [f32; 4],
    pub limbA: Option<usize>,
    pub limbB: Option<usize>,
    pub containerA: Option<Owner>,
    pub containerB: Option<Owner>,
    pub heartMonitor: Option<usize>,
    pub cardiopulmonaryBypass: Option<usize>,
    pub heartLimb: Option<usize>,
    pub AreBothLimbs: bool,
    pub IsHeartRateWire: bool,
    pub IsCardiopulmonaryBypassWire: bool,
    /// EnergyWireBehaviour.antenna and receiver while hasAntenna: the AntennaBehaviour's physical with its CorrectUp
    /// and Fuzziness, and the AntennaReceiverBehaviour's physical (`Game::antenna_receivers` holds its state).
    pub antenna: Option<Antenna>,
    /// EnergyWireBehaviour.eggs.b: the LaserBehaviour (`Game::lasers`) of a Hoverbox and Laser pair this wire joins.
    pub eggs: Option<usize>,
    /// EnergyWireBehaviour.hasEggs: SetEggs' last TryGetComponent<LaserBehaviour> (kept when neither end is a Hoverbox).
    pub hasEggs: bool,
    /// UseWireBehaviour: Channel, Color (the glow), CanBreak, the toSendOut queue and target (the connected body's
    /// PhysicalBehaviour, its IUseEmitter, found in Start).
    pub Channel: u16,
    pub Color: [f32; 4],
    pub CanBreak: bool,
    pub toSendOut: std::collections::VecDeque<super::signal::Act>,
    /// SendGlow's _Timestamp (Time.timeSinceLevelLoad) in the line's property block.
    pub glow_timestamp: Option<f32>,
    pub target: Option<usize>,
    /// NewUseWireBehaviour.from and to: `Game::activation_nodes` indices.
    pub from: Option<usize>,
    pub to: Option<usize>,
    /// A wire ObjectState.Reconstruct added (AddComponent and InjectIntoMonoBehaviour): no typedJoint until Start makes it from these.
    pub saved: Option<SavedWire>,
    /// The current* members (and targetDistance, SpringStrength, WireMaterialName) as Start left them: read from the tool's joint, or
    /// the stored ones of a loaded wire.
    pub current: SavedWire,
}

impl Default for BloodWire {
    fn default() -> Self {
        Self {
            kind: Default::default(),
            WireColor: [1.0; 4],
            start_color: Default::default(),
            end_color: Default::default(),
            localRenderEndpoint: Default::default(),
            originalRotation: Default::default(),
            hasMovedInPauseMode: Default::default(),
            otherEnd: Default::default(),
            breakForceBuffer: Default::default(),
            fakeCableTemperature: Default::default(),
            vertex_count: Default::default(),
            limb: Default::default(),
            bloodColour: Default::default(),
            SoakAmount: Default::default(),
            SoakColour: [1.0; 4],
            id: Default::default(),
            physical: Default::default(),
            other: Default::default(),
            joint: Default::default(),
            WireWidth: Default::default(),
            line_width: None,
            Invisible: Default::default(),
            started: Default::default(),
            usedToHaveConnectedBody: Default::default(),
            distanceBeforeFlip: Default::default(),
            shouldIgnoreUpdate: Default::default(),
            IsMouseInsideBounds: Default::default(),
            IsMouseOverlapping: Default::default(),
            tracker: Default::default(),
            vertices: Default::default(),
            flow: Default::default(),
            color: Default::default(),
            limbA: Default::default(),
            limbB: Default::default(),
            containerA: Default::default(),
            containerB: Default::default(),
            heartMonitor: Default::default(),
            cardiopulmonaryBypass: Default::default(),
            heartLimb: Default::default(),
            AreBothLimbs: Default::default(),
            IsHeartRateWire: Default::default(),
            IsCardiopulmonaryBypassWire: Default::default(),
            antenna: Default::default(),
            eggs: Default::default(),
            hasEggs: Default::default(),
            Channel: Default::default(),
            Color: Default::default(),
            CanBreak: Default::default(),
            toSendOut: Default::default(),
            glow_timestamp: Default::default(),
            target: Default::default(),
            from: Default::default(),
            to: Default::default(),
            saved: Default::default(),
            current: Default::default(),
        }
    }
}

impl BloodWire {
    /// The kind's behaviour class constructed: its own field initialisers over WireBehaviour's.
    pub fn new(kind: WireKind) -> Self {
        use WireKind::*;
        Self {
            kind,
            // RopeBehaviour, SteelBindingBehaviour, WoodStrutBehaviour, CopperWireBehaviour, UseWireBehaviour and
            // NewUseWireBehaviour: breakForceBuffer = 1; SpringCableBehaviour: initialBreakForce = infinity (kept here).
            breakForceBuffer: match kind {
                // WoodenBindingBehaviour is a SteelBindingBehaviour (fnrec r1: WoodenBindingTool.OnJointCreate).
                Rope | SteelBinding | WoodenBinding | WoodStrut | Copper | Use(..) | NewUse => 1.0,
                // AppliedBandageBehaviour is a SpringCableBehaviour too (fnrec r1: BandageWireTool.OnJointCreate's initialBreakForce).
                Spring | StrongSpring | DestructibleSpring | Bandage => f32::INFINITY,
                _ => 0.0,
            },
            // UseWireBehaviour.Color (1, 11, 0) and NewUseWireBehaviour.Color (11, 1, 11).
            Color: match kind {
                Use(..) => [1.0, 11.0, 0.0, 1.0],
                NewUse => [11.0, 1.0, 11.0, 1.0],
                _ => [0.0; 4],
            },
            ..Default::default()
        }
    }
}

/// The stored members of a loaded wire that its Start (the `!typedJoint` branch) reads.
#[derive(Clone, Debug)]
pub struct SavedWire {
    pub currentAnchor: [f32; 2],
    pub currentConnectedAnchor: [f32; 2],
    pub currentDistance: f32,
    pub currentMaxDistanceOnly: bool,
    pub currentBreakingForce: f32,
    /// SpringCableBehaviour.targetDistance and SpringStrength
    pub targetDistance: f32,
    pub SpringStrength: f32,
    pub WireMaterialName: String,
}

impl Default for SavedWire {
    /// The field initialisers: currentBreakingForce +inf, SpringStrength 2.
    fn default() -> Self {
        Self { currentAnchor: [0.0; 2], currentConnectedAnchor: [0.0; 2], currentDistance: 0.0, currentMaxDistanceOnly: false, currentBreakingForce: f32::INFINITY, targetDistance: 0.0, SpringStrength: 2.0, WireMaterialName: String::new() }
    }
}

impl WireKind {
    /// The behaviour class the kind's tool adds.
    pub fn class(self) -> &'static str {
        match self {
            Self::Blood => "BloodWireBehaviour",
            Self::Cable => "CableBehaviour",
            Self::Rope => "RopeBehaviour",
            Self::Chain => "ChainBehaviour",
            Self::Copper => "CopperWireBehaviour",
            Self::Energy => "EnergyWireBehaviour",
            Self::HeatPipe => "HeatPipeBehaviour",
            Self::Rigid => "RigidWireBehaviour",
            Self::Spring | Self::StrongSpring | Self::DestructibleSpring => "SpringCableBehaviour",
            Self::Bandage => "AppliedBandageBehaviour",
            Self::WoodStrut => "WoodStrutBehaviour",
            Self::FixedCable => "FixedCableBehaviour",
            Self::SteelBinding => "SteelBindingBehaviour",
            Self::WoodenBinding => "WoodenBindingBehaviour",
            Self::Slider => "SliderJointWireBehaviour",
            Self::Use(..) => "UseWireBehaviour",
            Self::NewUse => "NewUseWireBehaviour",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Antenna {
    /// antenna.a: the AntennaBehaviour's instance id (`!antenna.a` once it is destroyed) and antenna.phys.
    pub a: i32,
    pub antenna: usize,
    pub CorrectUp: [f32; 2],
    pub Fuzziness: f32,
    pub receiver: usize,
}

impl Default for Antenna {
    fn default() -> Self {
        Self {
            a: Default::default(),
            antenna: Default::default(),
            CorrectUp: [0.0, 1.0],
            Fuzziness: 0.15,
            receiver: Default::default(),
        }
    }
}

/// DistanceWireTool.WireWidth's field initialiser (0.05): the width of every kind whose Awake leaves it.
pub const DISTANCE_WIRE_WIDTH: f32 = 0.05;

impl WireTool {
    /// DistanceWireTool..ctor: the field initialisers WireWidth 0.05 and AllowNothingConnection true, before any Awake
    /// (the play state's None for both means the chosen kind's Awake value: WireKind::width, the Blood/Use false).
    pub fn new() -> Self {
        WireTool { WireWidth: Some(DISTANCE_WIRE_WIDTH), AllowNothingConnection: Some(true), ..Default::default() }
    }
}

/// The BloodWireTool (a DistanceWireTool) on the ToolController: whether it is the CurrentTool, and its drag.
#[derive(Clone, Debug, Default)]
pub struct WireTool {
    /// CurrentTool is a DistanceWireTool (else the DragTool).
    pub chosen: bool,
    /// Which one.
    pub kind: WireKind,
    /// BandageWireTool.startBody, which stays until the next OnHold with a selection.
    pub startBody: Option<usize>,
    /// The tool components added so far.
    pub created: Vec<WireKind>,
    /// The one LineRenderer on the ToolController, that the first OnToolChosen added.
    pub has_line: bool,
    pub startPos: [f32; 2],
    pub startOffset: [f32; 2],
    pub hasDragged: bool,
    /// The chosen tool's WireWidth when a recording's tool record holds one other than its Awake's (fnb2g's fuzzed 0.02).
    pub WireWidth: Option<f32>,
    /// The chosen tool's AllowNothingConnection when a recording's tool record holds it (fnb2j's fuzzed 0 on the FixedCableTool);
    /// None: its constructor's/Awake's.
    pub AllowNothingConnection: Option<bool>,
    /// SteelBindingBehaviour.stress, a static every WoodenBindingBehaviour.Created replaces: its folder and clip count.
    pub stress_clips: Option<(&'static str, usize)>,
    /// WoodStrutBehaviour.lastStressSoundTime, a static (float.MinValue at first).
    pub lastStressSoundTime: Option<f32>,
}

/// Utils.SqrdDistanceFromPointToLineSegment (a degenerate segment returns the distance itself).
pub(crate) fn sqrd_distance_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let dot = |u: [f32; 2], v: [f32; 2]| (super::d(u[0]) * v[0] + super::d(u[1]) * v[1]).f();
    let sqr = mathf::sqr_magnitude([a[0] - b[0], a[1] - b[1]]);
    if sqr as f64 == 0.0 {
        return mathf::magnitude2([p[0] - a[0], p[1] - a[1]]);
    }
    let num = mathf::clamp01(dot([p[0] - a[0], p[1] - a[1]], [b[0] - a[0], b[1] - a[1]]) / sqr);
    let v = [a[0] + num * (b[0] - a[0]), a[1] + num * (b[1] - a[1])];
    mathf::sqr_magnitude([p[0] - v[0], p[1] - v[1]])
}

/// Utils.DistanceFromPointToLineSegment: Mathf.Sqrt of Utils.SqrdDistanceFromPointToLineSegment, so a degenerate
/// segment gives the square root of its magnitude, as the C# does.
pub(crate) fn distance_to_line_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    mathf::sqrt(sqrd_distance_to_segment(p, a, b))
}

/// Utils.CalculateBreakForceForCable's formula on the joint's attachedRigidbody mass and its connectedBody's (None: no
/// connectedBody, the attached mass counts twice). A Joint2D always has an attachedRigidbody (RequireComponent), so the
/// referenceMass fallback for none is never taken.
pub(crate) fn cable_break_force(base: f32, attached: f32, connected: Option<f32>, reference_mass: f32, mass_influence: f32) -> f32 {
    if base == f32::INFINITY {
        return base;
    }
    let heavier = mathf::max(attached, connected.unwrap_or(attached));
    base * mathf::lerp(heavier / reference_mass, 1.0, 1.0 - mathf::clamp01(mass_influence))
}

/// Utils.Cosh
pub(crate) fn cosh(x: f32) -> f32 {
    (mathf::pow(std::f64::consts::E as f32, x) + mathf::pow(std::f64::consts::E as f32, 0.0 - x)) / 2.0
}

impl Game {
    /// WireSnapController.GetTransformedWorldEndPoint
    pub(crate) fn wire_end_point(&self, end: [f32; 2], start: [f32; 2]) -> [f32; 2] {
        if !self.input.held(crate::il!(str "WireSnapController::GetTransformedWorldEndPoint" 0)) {
            return end;
        }
        let v = [end[0] - start[0], end[1] - start[1]];
        let magnitude = mathf::magnitude2(v);
        if magnitude < 0.001 {
            return end;
        }
        let dir = [v[0] / magnitude, v[1] / magnitude];
        // Utils.Snap(Mathf.Atan2(y, x), (float)Math.PI / 4f): Mathf.Round is Math.Round, to even.
        let atan = crate::game::mathf::atan2(dir[1], dir[0]);
        let step = std::f64::consts::PI as f32 / 4.0;
        let f = mathf::snap(atan, step);
        [(super::d(start[0]) + super::d(mathf::cos(f)) * magnitude).f(), (super::d(start[1]) + super::d(mathf::sin(f)) * magnitude).f()]
    }

    /// `(bool)ActiveSingleSelected`: the tool's selected object, unless it was destroyed.
    pub(super) fn wire_selected(&self) -> Option<usize> {
        self.tool.as_ref().and_then(|t| t.ActiveSingleSelected).filter(|&p| !self.physicals[p].destroyed)
    }

    /// DistanceWireTool.OnSelect, FixedWireTool.OnSelect, SpringJointWireTool.OnSelect (the same body; the LineRenderer's two positions and its enabling are drawn by wires.rs `wires` through `wire_preview`).
    pub(crate) fn wire_on_select(&mut self) {
        let m = self.MousePosition;
        self.wire_tool.startPos = [m[0], m[1]];
        if let Some(sel) = self.wire_selected() {
            let o = self.inverse_transform_point(sel, [m[0], m[1], 0.0]);
            self.wire_tool.startOffset = [o[0], o[1]];
        }
        self.tool_line_or_throw(self.wire_tool.chosen || self.wire_tool.created.contains(&self.wire_tool.kind));
        self.wire_tool.hasDragged = true;
    }

    /// gapA r9: `lineRenderer.SetPositions` in a line tool's OnSelect/OnHold (DistanceWireTool and its kin, LinkDeviceTool):
    /// the tool's own field is null until its first OnToolChosen (a tool never chosen, as the recorder adds it: fngb10's
    /// seed), a NullReferenceException, even while another tool's OnToolChosen left the shared LineRenderer (gapa8_default's
    /// SliderTool beside a LogicWire line). `chosen`: the tool is the CurrentTool or its OnToolChosen ran (`created`).
    pub(crate) fn tool_line_or_throw(&self, chosen: bool) {
        if !chosen {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
    }

    /// The tool's LineRenderer while it is enabled: the start and WireSnapController's end.
    pub fn wire_preview(&self) -> Option<[[f32; 2]; 2]> {
        let t = &self.wire_tool;
        (t.chosen && t.hasDragged).then(|| [t.startPos, if t.kind.is_slider() { [self.MousePosition[0], self.MousePosition[1]] } else { self.wire_end_point([self.MousePosition[0], self.MousePosition[1]], t.startPos) }])
    }

    /// DistanceWireTool.OnHold, FixedWireTool.OnHold, SpringJointWireTool.OnHold (the same body; startBody is kept for the BandageWireTool only: the spring tool never prioritises it).
    pub(crate) fn wire_on_hold(&mut self) {
        if let Some(sel) = self.wire_selected() {
            if self.wire_tool.kind == WireKind::Bandage {
                self.wire_tool.startBody = Some(sel);
            }
            let o = self.wire_tool.startOffset;
            let p = self.transform_point3(sel, [o[0], o[1], 0.0]);
            self.wire_tool.startPos = [p[0], p[1]];
        }
        self.tool_line_or_throw(self.wire_tool.chosen || self.wire_tool.created.contains(&self.wire_tool.kind));
    }

    /// The layer of the PhysicalBehaviour's GameObject (that of its Rigidbody2D).
    pub(super) fn physical_game_object_layer(&self, p: usize) -> i32 {
        let go = self.world.body(self.physicals[p].rigidbody.body).user.expect("a physical's Rigidbody2D").game_object;
        self.world.layers[&go]
    }

    /// DistanceWireTool.OnDeselect, SpringJointWireTool.OnDeselect (the same body): AllowNothingConnection is false for the BloodWireTool and the use wire tools, whose wire needs both ends.
    pub(crate) fn wire_on_deselect(&mut self) {
        if !self.wire_tool.hasDragged {
            return;
        }
        if self.wire_tool.kind.is_fixed() {
            return self.fixed_wire_on_deselect();
        }
        if self.wire_tool.kind.is_slider() {
            return self.slider_wire_on_deselect();
        }
        // GetEndObject: SelectionController.Main.CurrentlyUnderMouse.
        let m = self.MousePosition;
        let start = self.wire_tool.startPos;
        let end = self.wire_end_point([m[0], m[1]], start);
        let end_body = self.wire_end_body();
        let kind = self.wire_tool.kind;
        let spring = kind.is_spring();
        let selected = self.wire_selected();
        // BloodWireTool.Awake, UseWireTool.Awake and DestructibleUseWireTool.Awake: AllowNothingConnection = false.
        let allow_nothing = self.wire_tool.AllowNothingConnection.unwrap_or(!matches!(kind, WireKind::Blood | WireKind::Use(..)));
        let joint = if (selected.is_some() && end_body.is_some()) || allow_nothing {
            match (selected, end_body) {
                (Some(sel), _) => {
                    self.allocate_instance_id();
                    let f = self.wire_add_joint(sel, spring);
                    let anchor = self.inverse_transform_point(sel, [start[0], start[1], 0.0]);
                    self.fixed_joint_set_anchor(f, [anchor[0], anchor[1]]);
                    self.fixed_joint_set_auto_configure(f, false);
                    if spring {
                        self.fixed_joint_set_enable_collision(f, true);
                    }
                    if let Some(eb) = end_body {
                        self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(eb)));
                        let connected = self.inverse_transform_point(eb, [end[0], end[1], 0.0]);
                        self.fixed_joint_set_connected_anchor(f, [connected[0], connected[1]]);
                    } else {
                        self.fixed_joint_set_connected_anchor(f, end);
                    }
                    Some((sel, f))
                }
                (None, Some(eb)) => {
                    self.allocate_instance_id();
                    let f = self.wire_add_joint(eb, spring);
                    let anchor = self.inverse_transform_point(eb, [end[0], end[1], 0.0]);
                    self.fixed_joint_set_anchor(f, [anchor[0], anchor[1]]);
                    self.fixed_joint_set_auto_configure(f, false);
                    self.fixed_joint_set_connected_anchor(f, start);
                    if spring {
                        self.fixed_joint_set_enable_collision(f, true);
                    }
                    Some((eb, f))
                }
                (None, None) => None,
            }
        } else {
            None
        };
        if let Some((owner, f)) = joint {
            if !spring {
                self.fixed_joint_set_enable_collision(f, true);
                self.distance_joint_set_max_distance_only(f, true);
            }
            super::stats::with_stats(|s| s.increment("WIRES_LAID", 1.0));
            self.blood_wire_tool_on_joint_create(owner, f);
        }
        self.wire_tool.hasDragged = false;
    }

    /// SliderTool.GetEndObject: endPos = the unsnapped mouse; endBody the object slider_end_body finds there.
    pub(crate) fn slider_tool_get_end_object(&mut self) -> Option<usize> {
        let m = self.MousePosition;
        self.slider_end_body([m[0], m[1]])
    }

    /// GetEndObject's pick: Physics2D.OverlapPoint(end, LayerMask.GetMask("Debris", "Objects")), a collider whose GameObject is not
    /// on layer 11 (Bounds) and has a Rigidbody2D. A collider with no PhysicalBehaviour on its own GameObject (a scene
    /// collider) has no Rigidbody2D to connect.
    pub(crate) fn slider_end_body(&mut self, end: [f32; 2]) -> Option<usize> {
        let (physical, _) = self.overlap_point_first(end, (1 << 9) | (1 << 10))?;
        physical.filter(|&p| self.physical_game_object_layer(p) != 11)
    }

    /// SliderTool.OnDeselect: a SliderJoint2D on the selected object (or on the object under the mouse), then OnJointCreate. The
    /// `ActiveSingleSelected == endBody.gameObject` test compares a PhysicalBehaviour with a GameObject, which is never equal, so a
    /// slider to its own body is still created (Create refuses it).
    pub(crate) fn slider_wire_on_deselect(&mut self) {
        // SliderTool.OnSelect: DistanceWireTool's (wire_on_select); its line's SetPositions take the unsnapped end (`wire_preview`, drawn by wires.rs `WireLine::Tool`).
        // SliderTool.OnHold: DistanceWireTool's (wire_on_hold).
        // SliderTool.OnFixedHold: empty (as DistanceWireTool's).
        // SliderTool.OnToolUnchosen: empty.
        // SliderTool.OnToolChosen: wire_tool_choose; the LineRenderer's default WireColor, WireWidth, numCapVertices 0, Top order 2, SlidePiston and Stretch are wires.rs `WireLine::Tool`.
        // SliderTool.Awake: the material "Materials/SlidePiston" (`WireKind::material`) and WireWidth 0.1 (`WireKind::width`).
        // SliderTool.constructor: AllowNothingConnection = true, so OnDeselect's test always passes.
        let m = self.MousePosition;
        let (start, end) = (self.wire_tool.startPos, [m[0], m[1]]);
        let end_body = self.slider_tool_get_end_object();
        let joint = match (self.wire_selected(), end_body) {
            (Some(sel), _) => {
                self.allocate_instance_id();
                let f = self.add_slider_joint_component(sel);
                self.fixed_joint_set_auto_configure(f, false);
                let anchor = self.inverse_transform_point(sel, [start[0], start[1], 0.0]);
                self.fixed_joint_set_anchor(f, [anchor[0], anchor[1]]);
                if let Some(eb) = end_body {
                    self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(eb)));
                    let connected = self.inverse_transform_point(eb, [end[0], end[1], 0.0]);
                    self.fixed_joint_set_connected_anchor(f, [connected[0], connected[1]]);
                } else {
                    self.fixed_joint_set_connected_anchor(f, end);
                }
                Some((sel, f))
            }
            (None, Some(eb)) => {
                self.allocate_instance_id();
                let f = self.add_slider_joint_component(eb);
                self.fixed_joint_set_auto_configure(f, false);
                let anchor = self.inverse_transform_point(eb, [end[0], end[1], 0.0]);
                self.fixed_joint_set_anchor(f, [anchor[0], anchor[1]]);
                self.fixed_joint_set_connected_anchor(f, start);
                Some((eb, f))
            }
            (None, None) => None,
        };
        if let Some((owner, f)) = joint {
            self.fixed_joint_set_enable_collision(f, true);
            self.slider_joint_set_auto_configure_angle(f, true);
            // SliderTool.OnJointCreate: the SliderJointWireBehaviour, the undo action "slider cable", WireColor, WireMaterial and WireWidth
            // are the tool's (blood_wire_tool_on_joint_create), typedJoint is the joint.
            self.blood_wire_tool_on_joint_create(owner, f);
        }
        self.wire_tool.hasDragged = false;
    }

    /// FixedWireTool.OnDeselect
    pub(crate) fn fixed_wire_on_deselect(&mut self) {
        // FixedWireTool.OnDeselect's own `if (!hasDragged) return;` (wire_on_deselect checks it too; fn-check calls this directly).
        if !self.wire_tool.hasDragged {
            return;
        }
        let m = self.MousePosition;
        let start = self.wire_tool.startPos;
        let end = self.wire_end_point([m[0], m[1]], start);
        let end_body = self.wire_end_body();
        let selected = self.wire_selected();
        // `(ActiveSingleSelected && endBody) || AllowNothingConnection` (true from FixedWireTool's subclasses' constructors).
        let allow_nothing = self.wire_tool.AllowNothingConnection.unwrap_or(true);
        let joint = match (selected, end_body) {
            _ if !(selected.is_some() && end_body.is_some()) && !allow_nothing => None,
            (Some(sel), _) => {
                self.allocate_instance_id();
                let f = self.add_fixed_joint_component(sel);
                let anchor = self.inverse_transform_point(sel, [start[0], start[1], 0.0]);
                self.fixed_joint_set_anchor(f, [anchor[0], anchor[1]]);
                self.fixed_joint_set_auto_configure(f, true);
                self.fixed_joint_set_enable_collision(f, true);
                if let Some(eb) = end_body {
                    self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(eb)));
                    let connected = self.inverse_transform_point(eb, [end[0], end[1], 0.0]);
                    self.fixed_joint_set_connected_anchor(f, [connected[0], connected[1]]);
                } else {
                    self.fixed_joint_set_connected_anchor(f, end);
                }
                Some((sel, f, end_body))
            }
            (None, Some(eb)) => {
                self.allocate_instance_id();
                let f = self.add_fixed_joint_component(eb);
                let anchor = self.inverse_transform_point(eb, [end[0], end[1], 0.0]);
                self.fixed_joint_set_anchor(f, [anchor[0], anchor[1]]);
                self.fixed_joint_set_auto_configure(f, true);
                self.fixed_joint_set_enable_collision(f, true);
                Some((eb, f, None))
            }
            (None, None) => None,
        };
        if let Some((owner, f, connected)) = joint {
            super::stats::with_stats(|s| s.increment("WIRES_LAID", 1.0));
            let endpoint = if end_body.is_some() && connected.is_none() { start } else { end };
            self.fixed_wire_tool_on_joint_create(owner, f, endpoint);
        }
        self.wire_tool.hasDragged = false;
    }

    /// FixedCableTool/SteelBindingTool/WoodenBindingTool.OnJointCreate(joint, worldSpaceEndpos): the shared wire-tool body, then
    /// localRenderEndpoint = (connectedBody ? connectedBody.transform : joint.transform).InverseTransformPoint(worldSpaceEndpos).
    pub(crate) fn fixed_wire_tool_on_joint_create(&mut self, owner: usize, f: usize, world_end: [f32; 2]) {
        self.blood_wire_tool_on_joint_create(owner, f);
        let frame = match self.fixed_joints[f].connected { Some(BodyOwner::Physical(c)) => c, Some(_) => unimplemented!("mod-only: a fixed wire connected to a bare Rigidbody2D"), None => owner };
        let local = self.inverse_transform_point(frame, [world_end[0], world_end[1], 0.0]);
        self.blood_wires.last_mut().unwrap().localRenderEndpoint = [local[0], local[1]];
    }

    fn wire_add_joint(&mut self, owner: usize, spring: bool) -> usize {
        if spring { self.add_spring_joint_component(owner) } else { self.add_distance_joint_component(owner) }
    }

    /// BandageWireTool.get_ShouldPrioritiseInitialGameObject
    pub(crate) fn bandage_wire_tool_should_prioritise_initial_game_object(&self) -> bool {
        true
    }

    /// SpringJointWireTool.get_ShouldPrioritiseInitialGameObject (the other wire tools' too: none overrides it).
    pub(crate) fn spring_joint_wire_tool_should_prioritise_initial_game_object(&self) -> bool {
        false
    }

    /// DistanceWireTool.GetEndObject, FixedWireTool.GetEndObject, SpringJointWireTool.GetEndObject: the object under the mouse. BandageWireTool.ShouldPrioritiseInitialGameObject prefers the start
    /// body: under the mouse, or among the colliders at the point, whose mask is the body's layer number as the game
    /// passes it. OverlapPointNonAlloc's buffer holds 8.
    pub(crate) fn wire_end_body(&mut self) -> Option<usize> {
        let under = self.tool.as_ref().and_then(|t| t.hover).filter(|&p| !self.physicals[p].destroyed);
        let prioritise = match self.wire_tool.kind {
            WireKind::Bandage => self.bandage_wire_tool_should_prioritise_initial_game_object(),
            _ => self.spring_joint_wire_tool_should_prioritise_initial_game_object(),
        };
        if prioritise {
            if let Some(s) = self.wire_tool.startBody.filter(|&s| self.wire_physical_alive(s)) {
                if under == Some(s) {
                    return Some(s);
                }
                let m = self.MousePosition;
                let layer = self.physical_game_object_layer(s) as u32;
                // SpringJointWireTool.overlapBuffer: an instance Collider2D[8]; NonAlloc fills it before the attachedRigidbody test.
                let hits = self.overlap_point_buffer([m[0], m[1], 0.0], layer, f32::NEG_INFINITY, f32::INFINITY, 8);
                if hits.contains(&s) {
                    return Some(s);
                }
            }
        }
        under.filter(|&p| self.physical_game_object_layer(p) != 11)
    }

    /// BloodWireTool.OnJointCreate, SpringJointWireTool.OnJointCreate, SpringWireTool.OnJointCreate (and the other wire tools')
    /// The tools' OnJointCreate: AddComponent (Awake: GetComponent<PhysicalBehaviour>), the undo action, the wire's look,
    /// the rope's and copper wire's break force, maxDistanceOnly off for the heat pipe and the rigid wire, and
    /// Invisible = (ToolVisibility == Invisible).
    pub(crate) fn blood_wire_tool_on_joint_create(&mut self, sel: usize, f: usize) {
        // Each tool's Awake sets WireColor (`color`) and WireWidth (`width`; DistanceWireTool's 0.05 when it sets none), and
        // the material, which is presentation. Each OnJointCreate is its kind's row below: the behaviour's AddComponent, the
        // undo action named by `display_name`, WireColor, WireWidth, typedJoint, the anchors (Joint_Anchor and
        // Joint_ConnectedAnchor are the joint's own), `break_strength`, and Invisible = ToolVisibility == Invisible.
        // RopeTool.OnJointCreate: Rope.
        // RopeTool.Awake: white.
        // CableWireTool.OnJointCreate: Cable, no break force.
        // CableWireTool.Awake: white.
        // ChainWireTool.OnJointCreate: Chain.
        // ChainWireTool.Awake: width 0.1.
        // CopperWireTool.OnJointCreate: Copper, break force (250, 1, 0.5).
        // CopperWireTool.Awake: white.
        // EnergyWireTool.OnJointCreate: Energy.
        // EnergyWireTool.Awake: WireColor (0, 0, 0.03).
        // HeatPipeTool.OnJointCreate: HeatPipe, maxDistanceOnly = false.
        // HeatPipeTool.Awake: width 0.1.
        // RigidWireTool.OnJointCreate: Rigid, maxDistanceOnly = false.
        // RigidWireTool.Awake: width 0.08.
        // WoodStrutTool.OnJointCreate: WoodStrut, maxDistanceOnly = false and break force (700, 1, 1).
        // WoodStrutTool.Awake: width 0.08.
        // FixedCableTool.OnJointCreate: FixedCable (localRenderEndpoint is set by fixed_wire_on_deselect).
        // FixedCableTool.Awake: width 0.06.
        // SteelBindingTool.OnJointCreate: SteelBinding, break force and torque 6000.
        // SteelBindingTool.Awake: width 0.1.
        // WoodenBindingTool.OnJointCreate: WoodenBinding, break force and torque 4000.
        // WoodenBindingTool.Awake: width 0.1.
        // BandageWireTool.OnJointCreate: Bandage (an AppliedBandageBehaviour).
        // BandageWireTool.Awake: width 0.07.
        // StrongSpringWireTool.OnJointCreate: StrongSpring, SpringStrength 8 (spring_strength).
        // StrongSpringWireTool.Awake: material only.
        // DestructibleSpringWireTool.OnJointCreate: DestructibleSpring, break force (850, 1, 0.5).
        // DestructibleSpringWireTool.Awake: material only.
        let kind = self.wire_tool.kind;
        let w = self.wire_tool_create_wire(sel, f, kind);
        // SliderTool.OnJointCreate leaves Hover.Invisible false (WireBehaviour.Initialise turns it on for the tool's clear colour).
        self.blood_wires[w].Invisible = !kind.is_slider() && self.tool.as_ref().unwrap().invisible;
        if matches!(kind, WireKind::HeatPipe | WireKind::Rigid | WireKind::WoodStrut) {
            self.distance_joint_set_max_distance_only(f, false);
        }
        if let WireKind::Use(channel, can_break) = kind {
            // UseWireTool.OnJointCreate: Channel and Color (GetWireChannel, GetWireColor), then CanBreak.
            // UseWireTool.CreateWire: the AddComponent, the "propagation wire" undo action and the shared fields above.
            // UseWireTool.GetWireChannel: 0 (Use's first field).
            // RedUseWireTool.GetWireChannel: 1.
            // BlueUseWireTool.GetWireChannel: 2.
            // DestructibleRedUseWireTool.GetWireChannel: 1.
            // DestructibleBlueUseWireTool.GetWireChannel: 2.
            // UseWireTool.GetBreakForce: infinity, and DestructibleUseWireTool's (850, 1, 0.5) is `break_strength`.
            // UseWireTool.Awake: width 0.05 and AllowNothingConnection = false.
            // DestructibleUseWireTool.Awake: the same.
            self.blood_wires[w].Channel = channel;
            self.blood_wires[w].Color = super::use_wire::wire_color(channel);
            self.blood_wires[w].CanBreak = can_break;
        }
        if let Some((strength, reference_mass, mass_influence)) = kind.break_strength() {
            let force = self.calculate_break_force_for_cable(f, strength, reference_mass, mass_influence);
            self.fixed_joint_set_break_force(f, force);
            if kind.is_fixed() {
                self.fixed_joint_set_break_torque(f, force);
            }
        }
    }

    /// The shared head of every wire tool's OnJointCreate, and UseWireTool.CreateWire: AddComponent of `kind`'s behaviour on
    /// `sel` (the joint's object), the ObjectCreationAction, WireColor, WireWidth and typedJoint (the anchors are the joint's own).
    pub(crate) fn wire_tool_create_wire(&mut self, sel: usize, f: usize, kind: WireKind) -> usize {
        let w = self.wire_tool_add_wire(sel, f, kind);
        self.undo_register_action(ObjectCreationAction { displayName: kind.display_name().into(), RelevantObjects: vec![self.blood_wires[w].id] });
        w
    }

    /// `wire_tool_create_wire` without the undo action: the AddComponent, WireColor, WireWidth and typedJoint.
    fn wire_tool_add_wire(&mut self, sel: usize, f: usize, kind: WireKind) -> usize {
        let id = self.allocate_instance_id();
        self.add_script_to_clone(self.physicals[sel].id, kind.class(), id);
        let color = kind.color();
        let width = self.wire_tool.WireWidth.filter(|_| kind == self.wire_tool.kind).unwrap_or(kind.width());
        self.blood_wires.push(BloodWire { kind, id, physical: sel, joint: f, WireWidth: width, WireColor: color, start_color: color, end_color: color, ..BloodWire::new(kind) });
        let w = self.blood_wires.len() - 1;
        // The OnJointCreate's SpringStrength (StrongSpringWireTool's 8; else the field's 2), before Start reads it.
        self.blood_wires[w].current.SpringStrength = kind.spring_strength();
        let b = Behaviour::BloodWire(w);
        // BloodWireBehaviour's MonoScript execution order is 0.
        self.behaviour_of.insert(id, (0, b));
        self.behaviours.add(0, b);
        self.pending_starts.push(super::misc::PendingStart::BloodWire(w));
        w
    }

    /// SpringJointWireTool.OnJointCreate, the base body (every tool overrides it, none calls base) on the chosen SpringWireTool:
    /// AddComponent<SpringCableBehaviour>, the tool's WireColor, WireMaterial (presentation), WireWidth, typedJoint, Invisible =
    /// ToolVisibility == Invisible. Unlike SpringWireTool's: no Joint_Anchor/Joint_ConnectedAnchor (obsolete, not kept) and no undo action.
    pub(crate) fn spring_joint_wire_tool_on_joint_create(&mut self, f: usize) {
        let BodyOwner::Physical(sel) = self.fixed_joints[f].attached else { unimplemented!("mod-only: OnJointCreate on a joint of a bare Rigidbody2D") };
        let kind = WireKind::of_tool("SpringWireTool").unwrap();
        let saved = std::mem::replace(&mut self.wire_tool.kind, kind);
        let w = self.wire_tool_add_wire(sel, f, kind);
        self.wire_tool.kind = saved;
        self.blood_wires[w].Invisible = self.tool.as_ref().unwrap().invisible;
    }

    /// UseWireTool.CreateWire(joint): a UseWireBehaviour (Channel 0, CanBreak false: the field defaults) on the joint's object.
    pub(crate) fn use_wire_tool_create_wire(&mut self, f: usize) -> usize {
        let BodyOwner::Physical(sel) = self.fixed_joints[f].attached else { unimplemented!("mod-only: UseWireTool.CreateWire on a joint of a bare Rigidbody2D") };
        self.wire_tool_create_wire(sel, f, WireKind::Use(0, false))
    }

    /// UseWireTool.OnJointCreate(joint) of the chosen Use wire tool `kind` on an existing joint (its object's).
    pub(crate) fn use_wire_tool_on_joint_create(&mut self, f: usize, kind: WireKind) {
        let BodyOwner::Physical(sel) = self.fixed_joints[f].attached else { unimplemented!("mod-only: UseWireTool.OnJointCreate on a joint of a bare Rigidbody2D") };
        let saved = std::mem::replace(&mut self.wire_tool.kind, kind);
        self.blood_wire_tool_on_joint_create(sel, f);
        self.wire_tool.kind = saved;
    }

    /// Utils.CalculateBreakForceForCable on fixed joint `f` (its two bodies' masses: cable_break_force).
    pub(super) fn calculate_break_force_for_cable(&self, f: usize, base: f32, reference_mass: f32, mass_influence: f32) -> f32 {
        let j = &self.fixed_joints[f];
        let mass = |o: BodyOwner| match o {
            BodyOwner::Physical(p) => self.physicals[p].rigidbody.mass(&self.world),
            BodyOwner::Node(k) => self.node_rigidbodies[k].rigidbody.mass(&self.world),
        };
        cable_break_force(base, mass(j.attached), j.connected.map(mass), reference_mass, mass_influence)
    }

    /// DistanceWireTool.OnToolChosen, FixedWireTool.OnToolChosen, SpringJointWireTool.Awake, SpringJointWireTool.OnToolChosen, BloodWireTool.Awake, BloodWireTool.OnToolChosen (the DistanceWireTool half of ToolControllerBehaviour.SetTool; the LineRenderer's look and the _Flow 0 are drawn by wires.rs `wires` for WireLine::Tool): AddComponent the first time (Awake loads the material), then
    /// OnToolChosen (GetComponent<LineRenderer> or AddComponent, and its properties).
    pub(super) fn wire_tool_choose(&mut self, kind: WireKind) {
        // BandageWireTool.OnToolChosen: DistanceWireTool's (base.OnToolChosen), the kind's line below.
        // WoodStrutTool.OnToolChosen: the same.
        // DistanceWireTool.OnToolUnchosen: empty
        // FixedWireTool.OnToolUnchosen: empty
        // FixedWireTool.OnFixedHold: empty
        if !self.wire_tool.created.contains(&kind) {
            self.allocate_instance_id();
            self.wire_tool.created.push(kind);
        }
        if !self.wire_tool.has_line {
            self.allocate_instance_id();
            self.wire_tool.has_line = true;
        }
        self.wire_tool.kind = kind;
        self.wire_tool.chosen = true;
    }

    /// `(bool)joint.connectedBody`: set, and its Rigidbody2D not destroyed.
    pub(super) fn wire_connected_alive(&self, f: usize) -> bool {
        match self.fixed_joints[f].connected {
            None => false,
            Some(BodyOwner::Physical(p)) => !self.destroyed_components.contains(&self.physicals[p].rigidbody.component),
            Some(BodyOwner::Node(k)) => !self.destroyed_components.contains(&self.node_rigidbodies[k].rigidbody.component),
        }
    }

    /// `(bool)physical`: the component was not destroyed.
    pub(super) fn wire_physical_alive(&self, p: usize) -> bool {
        !self.physicals[p].destroyed
    }

    /// physical.GetComponent<BloodContainer>(): a limb, flask, syringe, blood tank or valve.
    pub(super) fn wire_container(&mut self, q: usize) -> Option<Owner> {
        if let Some(v) = self.valves.iter().position(|x| x.physical == q) {
            return Some(Owner::Valve(v));
        }
        self.physical_container(q)
    }

    /// `(bool)container`: its GameObject's PhysicalBehaviour was not destroyed.
    fn wire_container_alive(&self, o: Owner) -> bool {
        self.wire_physical_alive(self.container_physical(o))
    }

    /// BloodContainer.AllowsTransfer: a ValveBehaviour's is ValvePosition > 0.5, a LiquidPressureValveBehaviour's and LiquidPumpBehaviour's
    /// false, every other container rewritten true.
    pub(crate) fn wire_allows(&self, o: Owner) -> bool {
        match o {
            Owner::Valve(v) => !self.valves[v].pressure_only && self.valve_allows_transfer(v),
            Owner::LiquidPump(_) => false,
            _ => true,
        }
    }

    /// BloodWireBehaviour.get_CanTransferBlood, once both containers are known alive: both AllowsTransfer.
    pub(crate) fn blood_wire_can_transfer(&self, a: Owner, b: Owner) -> bool {
        self.wire_allows(a) && self.wire_allows(b)
    }

    /// physical.GetComponent<CirculationBehaviour>(): a limb.
    fn wire_limb(&self, q: usize) -> Option<usize> {
        self.limbs.iter().position(|x| x.PhysicalBehaviour == q)
    }

    /// The wires added to physical `p`'s GameObject, as it becomes active again: MonoBehaviour::AddToManager (1808b62b0)
    /// queues DelayedStartCall for every script with a Start, so a wire whose Start was skipped while inactive starts.
    pub(super) fn blood_wires_requeue_start(&mut self, p: usize) {
        for w in 0..self.blood_wires.len() {
            if self.blood_wires[w].physical == p && !self.blood_wires[w].started && !self.destroyed_components.contains(&self.blood_wires[w].id) {
                self.pending_starts.push(super::misc::PendingStart::BloodWire(w));
            }
        }
    }

    /// DistanceJointWireBehaviour.Start (with a typedJoint) and WireBehaviour.Initialise: the Wire child object (a
    /// GameObject, its Transform, Optout and the LineRenderer take 4 instance ids), the other end's PhysicalBehaviour,
    /// then Created.
    pub fn blood_wire_start(&mut self, w: usize) {
        // MonoBehaviour::DelayedStartCall (1808b75f0) calls Start only while the behaviour is in the manager (+0x128, set by
        // AddToManager 1808b62b0, cleared by RemoveFromManager 1808bb550): a wire whose GameObject went inactive before its
        // Start (a disintegrated limb) does not start; a reactivation queues it again (`blood_wires_requeue_start`), and
        // DelayedStartCall skips a Start that already ran (+0x125).
        if self.blood_wires[w].started || !self.physicals[self.blood_wires[w].physical].active || self.destroyed_components.contains(&self.blood_wires[w].id) {
            return;
        }
        let kind = self.blood_wires[w].kind;
        if !(kind.is_slider() || kind.is_spring() || kind.is_fixed()) {
            self.distance_joint_wire_start(w);
        } else if kind.is_spring() {
            self.spring_cable_start(w);
        } else {
            self.wire_start_current(w);
            // A loaded wire's joint is made in wire_start_current: its index only now.
            let f = self.blood_wires[w].joint;
            // SliderJointWireBehaviour.Start (a typedJoint): autoConfigureAngle on and useLimits off, before Initialise (the Wire child, the
            // line's cap vertices and texture are as the other wires'). The branch without a typedJoint is a saved wire, not reachable here.
            if kind.is_slider() {
                self.slider_joint_set_auto_configure_angle(f, true);
                self.slider_joint_set_use_limits(f, false);
            }
            if kind.is_fixed() {
                self.fixed_joint_set_enable_collision(f, false);
            }
            self.wire_initialise(w);
        }
        // FixedJointWireBehaviour.Start: originalRotation = GetReferenceAngle(), AddComponent<CableOtherEndBehaviour> with
        // Connected = this (its OnPlayerDragEnd is in `drag_broadcast_drag_message`).
        if kind.is_fixed() && self.blood_wires[w].other.is_some_and(|o| self.wire_physical_alive(o)) {
            self.allocate_instance_id();
            self.blood_wires[w].otherEnd = true;
            let (me, o) = (self.blood_wires[w].physical, self.blood_wires[w].other.unwrap());
            self.blood_wires[w].originalRotation = self.fixed_wire_get_reference_angle(me, o);
        }
        // WoodStrutBehaviour.Start: base.Start(), breakForceBuffer = currentBreakingForce below (as SteelBindingBehaviour.Start); the
        // SteelBindingBehaviour.Start and WoodStrutBehaviour.Start: base.Start(), breakForceBuffer = currentBreakingForce below; the
        // startColor test against LegacyColour is false here (Created set white; only a loaded save has the legacy colour) so SetColor does not run.
        // RigidWireBehaviour.Start, FixedCableBehaviour.Start, SteelBindingBehaviour.Start (the wooden binding's too) and WoodStrutBehaviour.Start:
        // `lineRenderer.startColor == LegacyColour` (Color == is Vector4 ==: the squared distance under 9.99999944e-11) sets white.
        if matches!(kind, WireKind::Rigid | WireKind::FixedCable | WireKind::SteelBinding | WireKind::WoodenBinding | WireKind::WoodStrut) {
            let legacy = [4.0 / 255.0, 1.0 / 51.0, 1.0 / 15.0, 1.0f32];
            let c = self.blood_wires[w].WireColor;
            if (0..4).map(|i| (c[i] - legacy[i]) * (c[i] - legacy[i])).sum::<f32>() < 9.99999944e-11 {
                self.wire_set_color(w, [1.0; 4]);
            }
        }
        // RopeBehaviour.Start, CopperWireBehaviour.Start and SpringCableBehaviour.Start's initialBreakForce:
        // breakForceBuffer = currentBreakingForce.
        let f = self.blood_wires[w].joint;
        self.blood_wires[w].breakForceBuffer = self.fixed_joints[f].state.break_force;
        if kind == WireKind::Bandage {
            self.applied_bandage_start(w);
        }
        if let WireKind::Use(..) = kind {
            self.use_wire_start(w);
        }
        if kind == WireKind::NewUse {
            self.new_use_wire_start(w);
        }
        // WireBehaviour.Start: InvisibilityTracker = new ToolInvisibilityTracker(this, lineRenderer).
        self.tracker_new(super::tool_invisibility::Owner::Wire(w));
        self.blood_wires[w].started = true;
    }

    /// The current* members a wire's Start takes: a loaded wire's stored ones (its joint made from them), else the
    /// tool's joint's.
    fn wire_start_current(&mut self, w: usize) {
        self.blood_wires[w].current = match self.blood_wires[w].saved.clone() {
            Some(s) => {
                self.wire_saved_joint(w);
                s
            }
            None => self.wire_current_from_joint(w),
        };
    }

    /// SpringCableBehaviour.Start: the current* members from the typedJoint (or a loaded wire's), autoConfigureDistance off,
    /// initialBreakForce = currentBreakingForce, Initialise (vertexCount from the distance), then frequency = SpringStrength.
    pub(crate) fn spring_cable_start(&mut self, w: usize) {
        self.wire_start_current(w);
        // A loaded wire's joint is made by wire_start_current.
        let f = self.blood_wires[w].joint;
        self.fixed_joints[f].spring.as_mut().unwrap().auto_configure_distance = false;
        // initialBreakForce = currentBreakingForce
        self.blood_wires[w].breakForceBuffer = self.blood_wires[w].current.currentBreakingForce;
        self.wire_initialise(w);
        // typedJoint.frequency = SpringStrength (the field: the tool's, a loaded wire's stored one, or its current value).
        let strength = self.blood_wires[w].current.SpringStrength;
        self.fixed_joint_set_frequency(f, strength);
    }

    /// DistanceJointWireBehaviour.Start: the current* members from the typedJoint (or a loaded wire's joint made from
    /// them), then WireBehaviour.Initialise.
    pub(crate) fn distance_joint_wire_start(&mut self, w: usize) {
        self.wire_start_current(w);
        self.wire_initialise(w);
    }

    /// WireBehaviour.Initialise (from the wires' Start): Invisible, the vertices (GetVertexCount() + 1, zero), the Wire child
    /// (a GameObject, its Transform, Optout and the LineRenderer take 4 instance ids) with the line's colours WireColor, the
    /// other end's PhysicalBehaviour, Created, OnWireCreated. The InvisibilityTracker is made at the end of `blood_wire_start`.
    pub(crate) fn wire_initialise(&mut self, w: usize) {
        let f = self.blood_wires[w].joint;
        let kind = self.blood_wires[w].kind;
        // WireBehaviour.Initialise: a clear colour (or the DeleteWire material name, which only a loaded wire can have) hides the wire.
        if self.blood_wires[w].WireColor[3] < 0.01 || self.blood_wires[w].saved.as_ref().is_some_and(|s| s.WireMaterialName == "DeleteWire") {
            self.blood_wires[w].Invisible = true;
        }
        // SpringCableBehaviour.GetVertexCount: vertexCount - 1 (vertexCount = Max(2, CeilToInt(distance))).
        self.blood_wires[w].vertex_count = if kind.is_spring() {
            let distance = self.spring_joint_distance(f);
            (distance as f64).ceil().max(2.0) as usize - 1
        } else {
            kind.vertex_count()
        };
        (self.blood_wires[w].start_color, self.blood_wires[w].end_color) = (self.blood_wires[w].WireColor, self.blood_wires[w].WireColor);
        self.blood_wires[w].vertices = vec![[0.0; 3]; self.blood_wires[w].vertex_count + 1];
        // The game has no started field: what the rewrite's `started` stands for in a dump (vertices non-null) and in OnDestroy
        // (InvisibilityTracker non-null) is set only here, in Initialise (from every wire's Start). A direct Start call on a wire
        // that never started (fn-check: a destroyed typedJoint's untyped branch) sets it too.
        self.blood_wires[w].started = true;
        for _ in 0..4 {
            self.allocate_instance_id();
        }
        if self.wire_connected_alive(f) {
            self.blood_wires[w].usedToHaveConnectedBody = true;
            self.blood_wires[w].other = match self.fixed_joints[f].connected {
                Some(BodyOwner::Physical(p)) => Some(p),
                // GetComponent<PhysicalBehaviour>() of a bare Rigidbody2D's object is null.
                _ => None,
            };
        }
        match self.blood_wires[w].kind {
            WireKind::Blood => self.blood_wire_created(w),
            WireKind::Energy | WireKind::Copper => self.energy_wire_created(w),
            // SteelBindingBehaviour.Created: the static stress and break clips load once; WireColor, startColor and endColor white, numCapVertices 0
            // (`cap_vertices`), the sorting layer and texture mode are the line's look.
            WireKind::SteelBinding => {
                self.wire_tool.stress_clips.get_or_insert((crate::il!(str "SteelBindingBehaviour::Created" 0), METAL_STRESS_CLIPS));
                self.wire_white(w);
            }
            // WoodenBindingBehaviour.Created: SteelBindingBehaviour.stress is replaced by the wooden binding stress clips; the look is the steel one's.
            WireKind::WoodenBinding => {
                self.wire_tool.stress_clips = Some((crate::il!(str "WoodenBindingBehaviour::Created" 0), WOODEN_STRESS_CLIPS));
                self.wire_white(w);
            }
            // WoodStrutBehaviour.Created: WireColor, startColor and endColor white, numCapVertices 0 (`cap_vertices`); nothing is loaded.
            WireKind::WoodStrut => self.wire_white(w),
            // RigidWireBehaviour.Start: base.Start(), then SetColor(white) only for the legacy colour (a loaded save; the tool's is white already).
            // WireBehaviour.Created: empty (the base).
            _ => {}
        }
        // WireBehaviour.Initialise: ModdingApi.Events.OnWireCreated.Invoke(this) after Created.
        self.mod_event(&super::mods::mod_api::OnWireCreated, self.blood_wires[w].id);
    }

    /// WireColor, startColor and endColor white (the bindings' and the wood strut's Created).
    fn wire_white(&mut self, w: usize) {
        let x = &mut self.blood_wires[w];
        (x.WireColor, x.start_color, x.end_color) = ([1.0; 4], [1.0; 4], [1.0; 4]);
    }

    /// The `!typedJoint` branch of DistanceJointWireBehaviour.Start, SpringCableBehaviour.Start, FixedJointWireBehaviour.Start and
    /// SliderJointWireBehaviour.Start (a wire ObjectState.Reconstruct added): AddComponent of the joint, set up from the stored members
    /// (WireMaterial is Resources.Load of WireMaterialName: rendering). The common steps after the branch are `blood_wire_start`'s.
    fn wire_saved_joint(&mut self, w: usize) {
        let s = self.blood_wires[w].saved.clone().unwrap();
        let (p, kind) = (self.blood_wires[w].physical, self.blood_wires[w].kind);
        self.allocate_instance_id();
        let f = if kind.is_spring() {
            self.add_spring_joint_component(p)
        } else if kind.is_fixed() {
            self.add_fixed_joint_component(p)
        } else if kind.is_slider() {
            self.add_slider_joint_component(p)
        } else {
            self.add_distance_joint_component(p)
        };
        self.blood_wires[w].joint = f;
        let distance = !kind.is_spring() && !kind.is_fixed() && !kind.is_slider();
        if kind.is_fixed() {
            self.fixed_joint_set_auto_configure(f, true);
            self.fixed_joint_set_frequency(f, 0.0);
            self.fixed_joint_set_damping_ratio(f, 1.0);
        } else {
            self.fixed_joint_set_auto_configure(f, false);
            self.fixed_joint_set_enable_collision(f, true);
            if distance {
                self.distance_joint_set_auto_configure_distance(f, false);
            }
        }
        // `if (otherPhysicalBehaviour) typedJoint.connectedBody = otherPhysicalBehaviour.rigidbody`
        let other = self.blood_wires[w].other.filter(|&o| self.wire_physical_alive(o));
        if let Some(o) = other {
            self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(o)));
            self.fixed_joint_set_connected_anchor(f, s.currentConnectedAnchor);
        } else {
            let at = self.root_transform_point(p, s.currentConnectedAnchor);
            self.fixed_joint_set_connected_anchor(f, at);
        }
        self.fixed_joint_set_anchor(f, s.currentAnchor);
        if distance {
            self.distance_joint_set_max_distance_only(f, s.currentMaxDistanceOnly);
            self.distance_joint_set_distance(f, s.currentDistance);
        }
        if !s.currentBreakingForce.is_infinite() {
            self.fixed_joint_set_break_force(f, s.currentBreakingForce);
        }
        if distance {
            self.distance_joint_set_auto_configure_distance(f, true);
        }
        if kind.is_spring() {
            self.spring_joint_set_distance(f, s.targetDistance);
        }
    }

    /// The `typedJoint` branch of DistanceJointWireBehaviour.Start, FixedJointWireBehaviour.Start, SpringCableBehaviour.Start and
    /// SliderJointWireBehaviour.Start: the current* members (targetDistance for a spring) from the tool's joint, the connected anchor
    /// in the root's space when there is no connected body; WireMaterialName = WireMaterial.name.
    fn wire_current_from_joint(&self, w: usize) -> SavedWire {
        let (f, p, kind) = (self.blood_wires[w].joint, self.blood_wires[w].physical, self.blood_wires[w].kind);
        let st = &self.fixed_joints[f].state;
        let ca = [st.connected_anchor.x, st.connected_anchor.y];
        let distance = self.fixed_joints[f].distance;
        SavedWire {
            currentAnchor: [st.anchor.x, st.anchor.y],
            currentConnectedAnchor: if self.wire_connected_alive(f) { ca } else { self.root_inverse_transform_point(p, ca) },
            currentDistance: distance.map_or(0.0, |d| d.distance),
            currentMaxDistanceOnly: distance.is_some_and(|d| d.max_distance_only),
            currentBreakingForce: st.break_force,
            targetDistance: if kind.is_spring() { self.spring_joint_distance(f) } else { 0.0 },
            // A field Start never writes (the tool's, or a re-run Start's current value).
            SpringStrength: self.blood_wires[w].current.SpringStrength,
            WireMaterialName: if kind == WireKind::NewUse { String::new() } else { kind.material().trim_start_matches("Materials/").into() },
        }
    }

    /// `transform.root.InverseTransformPoint(point)` of a physical's object (z 0).
    fn root_inverse_transform_point(&self, p: usize, point: [f32; 2]) -> [f32; 2] {
        match self.root_of(p) {
            (false, r) => {
                let v = self.inverse_transform_point(r, [point[0], point[1], 0.0]);
                [v[0], v[1]]
            }
            (true, n) => {
                let (t, q) = self.node_world(n);
                let s = self.transform_nodes[n].trs.s;
                let root = crate::unity::transform::Trs { t: [t[0], t[1], t[2], 0.0], q, s };
                let v = crate::unity::transform::inverse(&root, [point[0], point[1], 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]).0;
                [v[0], v[1]]
            }
        }
    }

    /// `transform.root.TransformPoint(point)` of a physical's object (z 0).
    fn root_transform_point(&self, p: usize, point: [f32; 2]) -> [f32; 2] {
        match self.root_of(p) {
            (false, r) => self.transform_point(r, point),
            (true, n) => {
                let (t, q) = self.node_world(n);
                let s = self.transform_nodes[n].trs.s;
                let root = crate::unity::transform::Trs { t: [t[0], t[1], t[2], 0.0], q, s };
                let v = crate::unity::transform::global_position(&root, [point[0], point[1], 0.0, 0.0]);
                [v[0], v[1]]
            }
        }
    }

    /// AppliedBandageBehaviour.Start after base.Start (the sorting layer and order are `wires::shown`'s).
    pub(crate) fn applied_bandage_start(&mut self, w: usize) {
        let f = self.blood_wires[w].joint;
        self.fixed_joint_set_frequency(f, 3.0);
        self.fixed_joint_set_damping_ratio(f, 0.2);
        if let Some(l) = self.wire_limb(self.blood_wires[w].physical) {
            if self.limbs[l].IsAndroid {
                return;
            }
            self.blood_wires[w].limb = Some(l);
            let person = self.limbs[l].Person;
            self.people[person].AdrenalineLevel = (super::d(self.people[person].AdrenalineLevel) + 0.25).f();
            self.blood_wires[w].bloodColour = self.original_blood_color(l);
        }
        self.bandage_soak_colour(w);
    }

    /// AppliedBandageBehaviour.SetSoakColour
    pub(crate) fn bandage_soak_colour(&mut self, w: usize) {
        let wire = &mut self.blood_wires[w];
        if crate::assets::il::is_1_28_45() {
            // 1.28.45: the line takes SoakColour.
            if wire.limb.is_some() {
                wire.start_color = wire.SoakColour;
                wire.end_color = wire.SoakColour;
            }
            return;
        }
        if wire.limb.is_some() {
            let t = mathf::clamp01((super::d(wire.SoakAmount) * 0.7).f());
            let color: [f32; 4] = std::array::from_fn(|i| 1.0 + (wire.bloodColour[i] - 1.0) * t);
            wire.start_color = color;
            wire.end_color = color;
        }
    }

    /// AppliedBandageBehaviour.FixedUpdate, which hides WireBehaviour's: a bandage never Ticks, so fire and Slice leave
    /// it on. (`limb` is a limb that is not destroyed.)
    pub(crate) fn applied_bandage_fixed_update(&mut self, w: usize) {
        let Some(l) = self.blood_wires[w].limb.filter(|&l| self.wire_physical_alive(self.limbs[l].PhysicalBehaviour)) else { return };
        if crate::assets::il::is_1_28_45() {
            // 1.28.45: SoakColour moves toward the blood's computed colour, then SetSoakColour when the line is visible (only the
            // drawn colour; set every time here), then the bleeding and healing as before.
            let fallback = self.bc(super::container::Owner::Circulation(l)).cachedComputedColour;
            let computed = self.get_computed_color(super::container::Owner::Circulation(l), fallback);
            let t = mathf::clamp01((super::d(self.time.fixedDeltaTime) * self.circulations[l].BleedingRate * 0.5f32).f());
            self.blood_wires[w].SoakColour = super::visual::color_lerp(self.blood_wires[w].SoakColour, computed, t);
            self.bandage_soak_colour(w);
            self.circulations[l].BleedingRate = (super::d(self.circulations[l].BleedingRate) * 0.9).f();
            self.limbs[l].Health = mathf::lerp(self.limbs[l].Health, self.limbs[l].InitialHealth, 0.001);
            return;
        }
        self.circulations[l].BleedingRate = (super::d(self.circulations[l].BleedingRate) * 0.9).f();
        self.limbs[l].Health = mathf::lerp(self.limbs[l].Health, self.limbs[l].InitialHealth, 0.001);
        let soak = self.blood_wires[w].SoakAmount;
        let more = (super::d(soak) + super::d(self.time.fixedDeltaTime) * self.circulations[l].BleedingRate * 0.5).f();
        self.blood_wires[w].SoakAmount = mathf::max(soak, mathf::clamp01(more));
        self.bandage_soak_colour(w);
    }

    /// EnergyWireBehaviour.Created: the antenna and the hoverbox with a laser are the pairs it joins.
    pub(crate) fn energy_wire_created(&mut self, w: usize) {
        self.energy_wire_set_antenna_objects(w);
        self.energy_wire_set_eggs(w);
    }

    /// Whether a PhysicalBehaviour's GameObject holds a script of that class (TryGetComponent).
    fn physical_has_script(&mut self, q: usize, class: &str) -> bool {
        let id = self.physicals[q].id;
        self.with_component_instance(id, |_, inst, start| {
            inst.prefab.objects[start].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Script { class: n, .. } if n == class))
        })
    }

    /// Physical `a`'s AntennaBehaviour: its CorrectUp, Fuzziness and instance id.
    pub(crate) fn antenna_behaviour(&mut self, a: usize) -> ([f32; 2], f32, i32) {
        self.with_component_instance(self.physicals[a].id, |_, inst, x| {
            let fields = inst.prefab.objects[x].components.iter().enumerate().find_map(|(k, (_, c))| match c {
                crate::assets::db::Component::Script { class, fields, .. } if class == "AntennaBehaviour" => Some((fields.clone(), inst.objects[x].components[k])),
                _ => None,
            });
            let (v, id) = fields.expect("the AntennaBehaviour");
            let up = v.get("CorrectUp").expect("AntennaBehaviour.CorrectUp");
            let f = |v: &crate::assets::mono::Value, n: &str| v.get(n).and_then(crate::assets::mono::Value::f32).expect("an AntennaBehaviour float");
            ([f(up, "x"), f(up, "y")], f(&v, "Fuzziness"), id)
        })
    }

    /// EnergyWireBehaviour.SetAntennaObjects: physicalBehaviour's AntennaBehaviour first, else otherPhysicalBehaviour's; with
    /// the other end's AntennaReceiverBehaviour, receiver.j.ConnectedAntennae++ and Connect().
    pub(crate) fn energy_wire_set_antenna_objects(&mut self, w: usize) {
        let p = self.blood_wires[w].physical;
        let Some(o) = self.blood_wires[w].other.filter(|&o| self.wire_physical_alive(o)) else { return };
        let (a, r) = if self.physical_has_script(p, "AntennaBehaviour") { (p, o) } else { (o, p) };
        if self.physical_has_script(a, "AntennaBehaviour") && self.physical_has_script(r, "AntennaReceiverBehaviour") {
            let (CorrectUp, Fuzziness, id) = self.antenna_behaviour(a);
            self.blood_wires[w].antenna = Some(Antenna { a: id, antenna: a, CorrectUp, Fuzziness, receiver: r });
            // receiver.j.ConnectedAntennae++, then receiver.j.Connect().
            // AntennaReceiverBehaviour..ctor: the field defaults and the radio's Random (antenna.rs).
            let j = self.antenna_receiver_of(r).expect("an AntennaReceiverBehaviour in the arena");
            self.antenna_receivers[j].ConnectedAntennae += 1;
            self.antenna_receiver_connect(j);
        }
    }

    /// EnergyWireBehaviour.SetEggs: physicalBehaviour's HoverboxBehaviour first (then otherPhysicalBehaviour's LaserBehaviour
    /// decides hasEggs, with no fallback to the other order), else otherPhysicalBehaviour's HoverboxBehaviour and
    /// physicalBehaviour's LaserBehaviour. Then eggs.b.ConnectedEggs++ and eggs.b.ConnectEgg(eggs.a).
    pub(crate) fn energy_wire_set_eggs(&mut self, w: usize) {
        let p = self.blood_wires[w].physical;
        let Some(o) = self.blood_wires[w].other.filter(|&o| self.wire_physical_alive(o)) else { return };
        let mut hover = None;
        for (h, laser) in [(p, o), (o, p)] {
            if self.physical_has_script(h, "HoverboxBehaviour") {
                hover = Some(h);
                // hasEggs = TryGetComponent<LaserBehaviour>(out eggs.b)
                let b = self.physical_has_script(laser, "LaserBehaviour").then(|| self.lasers.iter().position(|l| l.physical == laser).expect("a LaserBehaviour in the arena"));
                self.blood_wires[w].eggs = b;
                self.blood_wires[w].hasEggs = b.is_some();
                break;
            }
        }
        if !self.blood_wires[w].hasEggs {
            return;
        }
        // With no Hoverbox found, hasEggs and eggs are the earlier call's (GenRun's flipped hasEggs: eggs.b null).
        let Some(b) = self.blood_wires[w].eggs else { crate::game::menu::npe() };
        self.lasers[b].ConnectedEggs += 1;
        // LaserBehaviour.ConnectEgg: hv = a (the earlier call's a, already there, when no Hoverbox was found)
        if hover.is_some() {
            self.lasers[b].hv = hover;
        }
    }

    /// EnergyWireBehaviour.OnDestroy, after WireBehaviour's: `receiver.j.ConnectedAntennae--` while the receiver is not
    /// destroyed, then `eggs.b.ConnectedEggs--` while the laser is not destroyed.
    pub(crate) fn energy_wire_on_destroy(&mut self, w: usize) {
        if let Some(x) = self.blood_wires[w].antenna.filter(|x| !self.physicals[x.receiver].destroyed) {
            let j = self.antenna_receiver_of(x.receiver).expect("an AntennaReceiverBehaviour in the arena");
            self.antenna_receivers[j].ConnectedAntennae -= 1;
        }
        if let Some(b) = self.blood_wires[w].eggs.filter(|&b| self.wire_physical_alive(self.lasers[b].physical)) {
            self.lasers[b].ConnectedEggs -= 1;
        }
    }

    /// BloodWireBehaviour.Created
    pub(crate) fn blood_wire_created(&mut self, w: usize) {
        // BloodWireBehaviour.Created: color = Color.clear before anything else.
        self.blood_wires[w].color = [0.0; 4];
        let (p, other) = (self.blood_wires[w].physical, self.blood_wires[w].other);
        let Some(o) = other.filter(|&o| o != p && self.wire_physical_alive(o)) else {
            self.blood_wires[w].AreBothLimbs = false;
            return;
        };
        let container_a = self.wire_container(p);
        let container_b = self.wire_container(o);
        let (limb_a, limb_b) = (self.wire_limb(p), self.wire_limb(o));
        let monitor = self.heart_monitors.iter().position(|x| x.physical == p).or_else(|| self.heart_monitors.iter().position(|x| x.physical == o));
        let cpb = self.cardiopulmonary_bypasses.iter().position(|x| x.physical == p).or_else(|| self.cardiopulmonary_bypasses.iter().position(|x| x.physical == o));
        let wire = &mut self.blood_wires[w];
        wire.containerA = container_a;
        wire.containerB = container_b;
        wire.limbA = limb_a;
        wire.limbB = limb_b;
        wire.heartMonitor = monitor;
        wire.cardiopulmonaryBypass = cpb;
        wire.IsHeartRateWire = monitor.is_some();
        let first = limb_a.or(limb_b);
        let cpb_wire = cpb.is_some()
            && first.is_some_and(|l| self.circulations[l].WasInitiallyPumping && !self.limbs[l].IsAndroid);
        self.blood_wires[w].IsCardiopulmonaryBypassWire = cpb_wire;
        if cpb_wire {
            let c = cpb.unwrap();
            self.cardiopulmonary_bypass_play_valid_sound(c);
            self.cardiopulmonary_bypasses[c].ValidConnectionCount += 1;
            self.cardiopulmonary_bypass_update_status_text(c);
        } else if let Some(c) = cpb {
            self.cardiopulmonary_bypass_play_invalid_sound(c);
            let s = match first {
                None => "<color=red>INORGANIC</color>",
                Some(l) if self.limbs[l].IsAndroid => "<color=red>ARTIFICIAL</color>",
                Some(l) if !self.circulations[l].WasInitiallyPumping => crate::il!(str "BloodWireBehaviour::Created" 4),
                Some(_) => "",
            };
            if !s.is_empty() {
                // BloodWireBehaviour.MoveNext: the flash() coroutine: StatusText.text = s, 3 s, then UpdateStatusText while the machine exists.
                // BloodWireBehaviour.System.Collections.IEnumerator.get_Current: the flash iterator's plumbing.
                // BloodWireBehaviour.System.Collections.Generic.IEnumerator<System.Object>.get_Current: likewise.
                // BloodWireBehaviour.System.Collections.IEnumerator.Reset: likewise (never called).
                // BloodWireBehaviour.System.IDisposable.Dispose: likewise.
                let id = self.cardiopulmonary_bypasses[c].id;
                let mut stage = 0;
                self.start_coroutine(id, id, Box::new(move |g| {
                    stage += 1;
                    if stage == 1 {
                        let text = g.cardiopulmonary_bypasses[c].StatusText;
                        g.text_write(text, s);
                        return Yield::WaitForSeconds(3.0);
                    }
                    if !g.physicals[g.cardiopulmonary_bypasses[c].physical].destroyed {
                        g.cardiopulmonary_bypass_update_status_text(c);
                    }
                    Yield::Done
                }));
            } else {
                self.cardiopulmonary_bypass_update_status_text(c);
            }
        }
        if self.blood_wires[w].IsHeartRateWire {
            let heart = [limb_a, limb_b].into_iter().flatten().find(|&l| !self.limbs[l].IsAndroid);
            match heart {
                Some(l) => self.blood_wires[w].heartLimb = Some(l),
                None => self.blood_wires[w].IsHeartRateWire = false,
            }
        }
        self.blood_wires[w].AreBothLimbs = matches!((limb_a, limb_b), (Some(a), Some(b)) if !self.limbs[a].IsAndroid && !self.limbs[b].IsAndroid);
    }

    /// Hover.CheckMouseInput of a WireBehaviour (no dialog, UI block or pause menu is ever open): deletes the wire while
    /// "delete" is held over it.
    pub(crate) fn blood_wire_check_mouse_input(&mut self, w: usize) {
        let mut m = self.MousePosition;
        let count = self.blood_wires[w].vertex_count;
        let all = self.blood_wires[w].vertices.clone();
        let p = self.blood_wires[w].physical;
        let fixed = self.blood_wires[w].kind.is_fixed();
        let v = &all[..=count];
        // WireBehaviour.GetVisualBounds: lineRenderer.bounds (LineRenderer::CalculateWorldMatrixAndBoundsJob, a world-space line): centre (max + min) * 0.5
        // and extents (max - min) * 0.5 + widthMultiplier * 0.5 on each axis; IsPointInsideBounds is strict. A local line's
        // box is the local one's world AABB, its extents through the absolute matrix (probed on a real fixed cable).
        let width = self.blood_wires[w].WireWidth;
        // The bounds are the LineRenderer's: its widthMultiplier (fnb2j k=2's fuzzed WireWidth -10.03 left it at 0.06).
        let line_width = self.blood_wires[w].line_width.unwrap_or(width);
        let inside_bounds = {
            let (mut centre, mut extent) = ([0.0f32; 2], [0.0f32; 2]);
            for i in 0..2 {
                let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[i]), hi.max(p[i])));
                (centre[i], extent[i]) = ((hi + lo) * 0.5, (hi - lo) * 0.5 + line_width * 0.5);
            }
            if fixed {
                let o = self.transform_point3(p, [0.0; 3]);
                let (x, y) = (self.transform_point3(p, [1.0, 0.0, 0.0]), self.transform_point3(p, [0.0, 1.0, 0.0]));
                let c = self.transform_point3(p, [centre[0], centre[1], 0.0]);
                let e = extent;
                centre = [c[0], c[1]];
                extent = std::array::from_fn(|i| (x[i] - o[i]).abs() * e[0] + (y[i] - o[i]).abs() * e[1]);
            }
            (0..2).all(|i| m[i] > centre[i] - extent[i] && m[i] < centre[i] + extent[i])
        };
        self.blood_wires[w].IsMouseInsideBounds = inside_bounds;
        // WireBehaviour.IsMouseInsideCollider: the mouse within WireWidth of one of the segments, in the line's space.
        let limit = width * width;
        let v = if fixed {
            m = self.inverse_transform_point(p, m);
            &all[..=count]
        } else {
            v
        };
        let on_wire = inside_bounds && (0..count).any(|i| sqrd_distance_to_segment([m[0], m[1]], [v[i][0], v[i][1]], [v[i + 1][0], v[i + 1][1]]) <= limit);
        let flag = on_wire;
        let id = self.blood_wires[w].id;
        self.hover_track(id, inside_bounds, flag);
        if flag && self.input.held(crate::il!(str "Hover::CheckMouseInput" 0)) && self.tool_should_be_visible(self.blood_wires[w].Invisible) {
            // SliderJointWireBehaviour.OnUserDelete: empty (WireBehaviour's Destroy follows).
            // FixedJointWireBehaviour.OnUserDelete: empty (WireBehaviour's Destroy follows).
            // SpringCableBehaviour.OnUserDelete: empty; Object.Destroy(this).
            // DistanceJointWireBehaviour.OnUserDelete: empty (WireBehaviour's Destroy follows).
            let id = self.blood_wires[w].id;
            self.tr_destroy_component(id, 0.0);
        }
        // OnMouseEnter or OnMouseExit (the ToolInvisibilityTracker's) when IsMouseOverlapping changes.
        if self.blood_wires[w].IsMouseOverlapping != flag {
            self.blood_wires[w].IsMouseOverlapping = flag;
            let o = super::tool_invisibility::Owner::Wire(w);
            if flag { self.tracker_on_mouse_enter(o) } else { self.tracker_on_mouse_exit(o) }
        }
    }

    /// DistanceJointWireBehaviour.CalculateParabola into `vertices`.
    pub(crate) fn blood_wire_calculate_parabola(&mut self, w: usize) {
        let (f, count) = (self.blood_wires[w].joint, self.blood_wires[w].vertex_count);
        let v = self.wire_parabola(f, count);
        self.blood_wires[w].vertices[..=count].copy_from_slice(&v);
    }

    /// DistanceJointWireBehaviour.CalculateParabola between joint `f`'s ends: `count + 1` vertices.
    fn wire_parabola(&self, f: usize, count: usize) -> Vec<[f32; 3]> {
        let (a, b) = self.joint_ends(f);
        let distance = self.distance_joint_distance(f);
        let num = (super::d(distance) * distance).f();
        let diff = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let sqr = (super::d(diff[0]) * diff[0] + super::d(diff[1]) * diff[1] + super::d(diff[2]) * diff[2]).f();
        // Mono: `1f - sqrMagnitude / num` is one double expression, rounded at Pow's float argument.
        let num2 = (super::d(mathf::pow((super::d(1.0) - super::d(sqr) / num).f(), 0.65)) * distance / 2.0).f();
        let mut vertices = [[0.0f32; 3]; 13];
        for i in 0..count {
            let t = i as f32 / count as f32;
            let mut v = [mathf::lerp(a[0], b[0], t), mathf::lerp(a[1], b[1], t), mathf::lerp(a[2], b[2], t)];
            if num2 > mathf::EPSILON {
                // DistanceJointWireBehaviour.CatenaryFunction
                let catenary = cosh((super::d(2.0 * t) * 1.316958 - 1.316958).f()) - 2.0;
                v[1] = (super::d(v[1]) + super::d(catenary) * num2).f();
            }
            vertices[i] = v;
        }
        vertices[0] = a;
        vertices[count] = b;
        vertices[..=count].to_vec()
    }

    /// The wire's two world ends: the joint's anchors.
    fn wire_ends(&self, w: usize) -> ([f32; 3], [f32; 3]) {
        self.joint_ends(self.blood_wires[w].joint)
    }

    /// Joint `f`'s two world ends: `transform.TransformPoint(anchor)`, and the connected body's
    /// `transform.TransformPoint(connectedAnchor)` (the world connectedAnchor without a body).
    pub(super) fn joint_ends(&self, f: usize) -> ([f32; 3], [f32; 3]) {
        let j = &self.fixed_joints[f];
        let (anchor, connected_anchor) = (j.state.anchor, j.state.connected_anchor);
        let a = self.owner_transform_point3(j.attached, [anchor.x, anchor.y, 0.0]);
        let b = match (self.wire_connected_alive(f), j.connected) {
            (true, Some(o)) => self.owner_transform_point3(o, [connected_anchor.x, connected_anchor.y, 0.0]),
            (true, None) => unreachable!("wire_connected_alive implies a connected body"),
            (false, _) => [connected_anchor.x, connected_anchor.y, 0.0],
        };
        (a, b)
    }

    /// `transform.TransformPoint` of a Rigidbody2D owner's object (a node body's through its chain).
    fn owner_transform_point3(&self, o: BodyOwner, point: [f32; 3]) -> [f32; 3] {
        match o {
            BodyOwner::Physical(p) => self.transform_point3(p, point),
            BodyOwner::Node(k) => {
                let mut v = [point[0], point[1], point[2], 0.0];
                for node in self.node_chain(self.node_rigidbodies[k].node).iter().rev() {
                    v = crate::unity::transform::global_position(node, v);
                }
                [v[0], v[1], v[2]]
            }
        }
    }

    /// FixedJointWireBehaviour.Update, FixedJointWireBehaviour.GetVertexCount (1: `vertices` hold two points): the line's local ends, kept until one moves by more than deltaThreshold.
    pub(crate) fn fixed_wire_vertices(&mut self, w: usize) {
        let (p, f) = (self.blood_wires[w].physical, self.blood_wires[w].joint);
        let anchor = self.fixed_joints[f].state.anchor;
        let end = self.blood_wires[w].localRenderEndpoint;
        let b = match (self.wire_connected_alive(f), self.fixed_joints[f].connected) {
            (true, Some(BodyOwner::Physical(o))) => {
                let world = self.transform_point3(o, [end[0], end[1], 0.0]);
                let local = self.inverse_transform_point(p, world);
                [local[0], local[1]]
            }
            (true, Some(BodyOwner::Node(k))) => {
                // connectedBody.transform.TransformPoint(localRenderEndpoint) through the node's chain.
                let mut v = [end[0], end[1], 0.0, 0.0];
                for node in self.node_chain(self.node_rigidbodies[k].node).iter().rev() {
                    v = crate::unity::transform::global_position(node, v);
                }
                let local = self.inverse_transform_point(p, [v[0], v[1], v[2]]);
                [local[0], local[1]]
            }
            (true, None) => unreachable!("wire_connected_alive implies a connected body"),
            (false, _) => end,
        };
        let v = &mut self.blood_wires[w].vertices;
        let moved = |x: [f32; 3], y: [f32; 2]| mathf::sqr_magnitude([x[0] - y[0], x[1] - y[1]]) > 0.001;
        if moved(v[0], [anchor.x, anchor.y]) || moved(v[1], b) {
            v[0] = [anchor.x, anchor.y, 0.0];
            v[1] = [b[0], b[1], 0.0];
        }
    }

    /// The stress sound of SteelBindingBehaviour.Tick and WoodStrutBehaviour.Tick: true when it plays. The Random.value goes unless the body is gone.
    pub(crate) fn wire_stress_sound(&mut self, w: usize, threshold: f32) -> bool {
        if !self.wire_physical_alive(self.blood_wires[w].physical) || self.random.value() <= 0.9997 {
            return false;
        }
        let f = self.blood_wires[w].joint;
        let reaction = mathf::magnitude2(self.fixed_joint_reaction_force(f));
        (super::d(reaction) / self.fixed_joints[f].state.break_force).f() > threshold
    }

    /// SteelBindingBehaviour.Tick
    pub(crate) fn binding_tick(&mut self, w: usize) {
        if self.wire_stress_sound(w, 0.2) {
            let (folder, n) = self.wire_tool.stress_clips.expect("Created set it");
            let clip = self.pick_resource_clip(folder, n);
            self.play_clip_once(self.blood_wires[w].physical, clip, 0.5);
        }
        self.wire_burn(w);
    }

    /// WoodStrutBehaviour.Tick
    pub(crate) fn wood_strut_tick(&mut self, w: usize) {
        let due = (super::d(self.time.time) - self.wire_tool.lastStressSoundTime.unwrap_or(LAST_STRESS_SOUND_TIME)).f() > 0.5;
        if due && self.wire_stress_sound(w, 0.1) {
            self.wire_tool.lastStressSoundTime = Some(self.time.time);
            let clip = self.pick_clip(0, "WoodStrutStressClips", WOOD_STRUT_STRESS_CLIPS);
            self.play_clip_once(self.blood_wires[w].physical, clip, 0.5);
        }
        self.wire_burn(w);
    }

    /// SliderJointWireBehaviour.Update after base.Update: GetPoints (anchor and connected anchor as Vector2s, so no z) and the three
    /// vertices lerped between them. It does not look at shouldIgnoreUpdate.
    pub(crate) fn slider_wire_vertices(&mut self, w: usize) {
        let v = self.slider_line(self.blood_wires[w].joint);
        self.blood_wires[w].vertices[..3].copy_from_slice(&v);
    }

    /// SliderJointWireBehaviour.GetPoints: `joint_ends` as Vector2s.
    pub(crate) fn slider_wire_get_points(&self, f: usize) -> ([f32; 2], [f32; 2]) {
        let (a, b) = self.joint_ends(f);
        ([a[0], a[1]], [b[0], b[1]])
    }

    /// GetPoints and the three vertices (GetVertexCount: 2).
    fn slider_line(&self, f: usize) -> [[f32; 3]; 3] {
        let (a, b) = self.slider_wire_get_points(f);
        std::array::from_fn(|i| {
            let t = i as f32 / 2.0;
            [mathf::lerp(a[0], b[0], t), mathf::lerp(a[1], b[1], t), 0.0]
        })
    }

    /// SpringCableBehaviour.Tick (WireBehaviour.Tick is empty): the burn while typedJoint lives (a wire before Start has
    /// none) and initialBreakForce (breakForceBuffer) is finite.
    pub(crate) fn spring_cable_tick(&mut self, w: usize) {
        let alive = self.fixed_joints.get(self.blood_wires[w].joint).is_some_and(|j| !j.destroyed);
        if alive && !self.blood_wires[w].breakForceBuffer.is_infinite() {
            self.wire_burn(w);
        }
    }

    /// SpringCableBehaviour.Update's straight line.
    pub(crate) fn spring_cable_vertices(&mut self, w: usize) {
        let (f, count) = (self.blood_wires[w].joint, self.blood_wires[w].vertex_count);
        let v = self.spring_line(f, count);
        self.blood_wires[w].vertices[..=count].copy_from_slice(&v);
    }

    /// SpringCableBehaviour.Update's `count + 1` vertices lerped between joint `f`'s ends.
    fn spring_line(&self, f: usize, count: usize) -> Vec<[f32; 3]> {
        let (a, b) = self.joint_ends(f);
        (0..=count)
            .map(|i| {
                let t = i as f32 / count as f32;
                std::array::from_fn(|k| mathf::lerp(a[k], b[k], t))
            })
            .collect()
    }

    /// BloodWireBehaviour.Update: DistanceJointWireBehaviour.Update (WireBehaviour.Update, then the vertices), then
    /// ProcessCardiopulmonaryBypassWire.
    pub fn blood_wire_update(&mut self, w: usize) {
        if !self.blood_wires[w].started {
            return;
        }
        self.wire_base_update(w);
        if self.blood_wires[w].kind.is_slider() {
            self.slider_wire_vertices(w);
        } else if self.blood_wires[w].kind.is_fixed() {
            if !self.blood_wires[w].shouldIgnoreUpdate {
                self.fixed_wire_vertices(w);
            }
        } else if self.blood_wires[w].kind.is_spring() {
            // SpringCableBehaviour.Update: the line whenever lineChild lives, with no shouldIgnoreUpdate test (base.Update's
            // Destroy branch is delayed, so that frame still lays the vertices).
            self.spring_cable_vertices(w);
        } else if !self.blood_wires[w].shouldIgnoreUpdate {
            self.blood_wire_calculate_parabola(w);
        }
        if self.blood_wires[w].kind == WireKind::Blood {
            self.blood_wire_process_cardiopulmonary_bypass(w);
        }
    }

    /// WireBehaviour.Update on a started wire (the GetPausedMenu test is the caller's): Destroy once the other end disintegrated
    /// or the connected body went, else the mouse test and the stress colour.
    pub(crate) fn wire_base_update(&mut self, w: usize) {
        let (id, f) = (self.blood_wires[w].id, self.blood_wires[w].joint);
        let other_gone = self.blood_wires[w].other.is_some_and(|o| self.wire_physical_alive(o) && self.physicals[o].isDisintegrated);
        if other_gone || (!self.wire_connected_alive(f) && self.blood_wires[w].usedToHaveConnectedBody) {
            self.blood_wires[w].shouldIgnoreUpdate = true;
            self.tr_destroy_component(id, 0.0);
        } else {
            self.blood_wire_check_mouse_input(w);
            self.wire_stress_colour(w);
        }
    }

    /// WireBehaviour.GetNormalisedStress and WireBehaviour.Update's stress colour: red as the joint nears its break force, in limb status view.
    pub(crate) fn wire_stress_colour(&mut self, w: usize) {
        let f = self.blood_wires[w].joint;
        let force = self.fixed_joints[f].state.break_force;
        if self.blood_wires[w].IsMouseInsideBounds || !self.ShowLimbStatus || force.is_infinite() {
            return;
        }
        let reaction = self.fixed_joint_reaction_force(f);
        let stress = mathf::sqr_magnitude(reaction) / (force * force);
        let t = mathf::clamp01(mathf::sqrt(stress));
        let c = self.blood_wires[w].WireColor;
        let color = [mathf::lerp(c[0], 1.0, t), mathf::lerp(c[1], 0.0, t), mathf::lerp(c[2], 0.0, t), mathf::lerp(c[3], 1.0, t)];
        self.blood_wires[w].start_color = color;
        self.blood_wires[w].end_color = color;
    }

    // CopperWireBehaviour.Tick: wire_burn below (after EnergyWireBehaviour.Tick's TransferEnergy).
    // CopperWireBehaviour.Start: breakForceBuffer = currentBreakingForce is set where the joint is adopted (breakForceBuffer above); the break clips are a static load.
    // CopperWireBehaviour.JointBroken: blood_wire_on_joint_break (PlayClipOnce(breakClips.PickRandom(), Random.value)).
    // CopperWireBehaviour.Slice: breakForceBuffer = 0 (the slice code in this file).
    // CopperWireBehaviour.SetWireColours: empty, as energy_wire_tick notes. CopperWireBehaviour..ctor: field defaults of `kind`.
    /// The burn that takes rope and copper wire apart: breakForce = breakForceBuffer * (1 - the worse end's BurnProgress).
    pub(crate) fn wire_burn(&mut self, w: usize) {
        let (p, other, f) = (self.blood_wires[w].physical, self.blood_wires[w].other, self.blood_wires[w].joint);
        let mut num = 0.0;
        if self.wire_physical_alive(p) {
            num = self.physicals[p].BurnProgress;
        }
        if let Some(o) = other.filter(|&o| self.wire_physical_alive(o)) {
            num = mathf::max(num, self.physicals[o].BurnProgress);
        }
        let force = (super::d(self.blood_wires[w].breakForceBuffer) * (super::d(1.0) - super::d(num))).f();
        self.fixed_joint_set_break_force(f, force);
    }

    /// EnergyWireBehaviour.TransferAntennaSignal (Tick calls it while hasAntenna; without a pair it does nothing): a destroyed
    /// end clears the pair (receiver.j.ConnectedAntennae-- while the receiver lives); else the receiver's Charge and SignalStrength.
    pub(crate) fn energy_wire_transfer_antenna_signal(&mut self, w: usize) {
        let Some(x) = self.blood_wires[w].antenna else { return };
        if self.physicals[x.antenna].destroyed || self.destroyed_components.contains(&x.a) || self.physicals[x.receiver].destroyed {
            if !self.physicals[x.receiver].destroyed {
                let j = self.antenna_receiver_of(x.receiver).expect("an AntennaReceiverBehaviour in the arena");
                self.antenna_receivers[j].ConnectedAntennae -= 1;
            }
            self.blood_wires[w].antenna = None;
        } else {
            let strength = self.antenna_signal_strength(x);
            let charge = self.physicals[x.receiver].Charge();
            self.physicals[x.receiver].charge = charge + strength;
            let j = self.antenna_receiver_of(x.receiver).expect("an AntennaReceiverBehaviour in the arena");
            let t = (d(self.time.fixedDeltaTime) * 10.0f32).f();
            self.antenna_receivers[j].SignalStrength = mathf::lerp(self.antenna_receivers[j].SignalStrength, strength, t);
        }
    }

    /// DistanceJointWireBehaviour.Tick (every distance wire's base.Tick): currentDistance = typedJoint.distance; a destroyed
    /// joint throws there, before the subclass part.
    pub(crate) fn distance_wire_tick(&mut self, w: usize) {
        let f = self.blood_wires[w].joint;
        if self.fixed_joints[f].destroyed {
            crate::game::menu::npe();
        }
        self.blood_wires[w].current.currentDistance = self.distance_joint_distance(f);
    }

    /// EnergyWireBehaviour.Tick: base.Tick (DistanceJointWireBehaviour's), TransferEnergy, then SetWireColours, which
    /// CopperWireBehaviour leaves empty.
    pub(crate) fn energy_wire_tick(&mut self, w: usize) {
        self.distance_wire_tick(w);
        let (p, other, kind) = (self.blood_wires[w].physical, self.blood_wires[w].other, self.blood_wires[w].kind);
        let Some(o) = other.filter(|&o| self.wire_physical_alive(o) && self.wire_physical_alive(p)) else { return };
        // EnergyWireBehaviour.TransferEnergy: Utils.TransferEnergyFixedRate.
        self.transfer_energy_fixed_rate(p, o, 0.89);
        self.energy_wire_transfer_antenna_signal(w);
        if kind == WireKind::Energy {
            self.energy_wire_set_wire_colours(w);
        }
    }

    /// EnergyWireBehaviour.SetWireColours: the line's colours, with the Random.value draws.
    pub(crate) fn energy_wire_set_wire_colours(&mut self, w: usize) {
        let p = self.blood_wires[w].physical;
        let charge = self.physicals[p].GetChargeWithWireResistance();
        let c = self.blood_wires[w].WireColor;
        if charge > 0.001 {
            let mut cyan = |g: &mut Game| {
                let t = (super::d(g.random.value()) * charge).f();
                [c[0] + (0.0 - c[0]) * t, c[1] + (1.0 - c[1]) * t, c[2] + (1.0 - c[2]) * t, c[3] + (1.0 - c[3]) * t]
            };
            self.blood_wires[w].start_color = cyan(self);
            self.blood_wires[w].end_color = cyan(self);
        } else {
            self.blood_wires[w].start_color = c;
            self.blood_wires[w].end_color = c;
        }
    }

    /// AntennaBehaviour.GetSignalStrength
    pub(crate) fn antenna_signal_strength(&self, x: Antenna) -> f32 {
        let right = self.transform_right(x.antenna);
        let f = super::physical::dot2(x.CorrectUp, [right[0], right[1]]).abs();
        let k = (super::d(1.0f32) + x.Fuzziness).f();
        let s = mathf::pow((super::d(1.0f32) - mathf::pow(f, k)).f(), (super::d(4.0f32) * (super::d(1.0f32) + x.Fuzziness)).f());
        if s.is_nan() { 0.0 } else { s }
    }

    // HeatPipeBehaviour.GetHeatTransferFactor: HEAT_PIPE_FACTOR (FACTOR below; nothing overrides it).
    // HeatPipeBehaviour.Start: the line's MaterialPropertyBlock (GetPropertyBlock of the line's current block); OnWillRenderObject's _Progress is wires.rs `shown`.
    // FixedCableBehaviour.Start: FixedJointWireBehaviour.Start; the startColor test against LegacyColour is false for a fresh wire (only a loaded save has that colour)
    /// HeatPipeBehaviour.Tick
    pub(crate) fn heat_pipe_tick(&mut self, w: usize) {
        const FACTOR: f32 = HEAT_PIPE_FACTOR;
        let (p, other) = (self.blood_wires[w].physical, self.blood_wires[w].other);
        if let Some(o) = other.filter(|&o| self.wire_physical_alive(o) && self.wire_physical_alive(p)) {
            if self.physicals[o].SimulateTemperature && self.physicals[p].SimulateTemperature {
                self.average_temperature(p, o, FACTOR);
                let sum = super::d(self.physicals[p].Temperature) + super::d(self.physicals[o].Temperature);
                self.blood_wires[w].fakeCableTemperature = (sum * 0.5).f();
            }
        }
        let t = self.blood_wires[w].fakeCableTemperature;
        self.blood_wires[w].fakeCableTemperature = mathf::lerp(t, self.AmbientTemperature, FACTOR);
    }

    /// BloodWireBehaviour.ProcessCardiopulmonaryBypassWire. A destroyed machine or limb throws (Missing/NullReference
    /// exception, logged by Unity), which ends the method.
    pub(crate) fn blood_wire_process_cardiopulmonary_bypass(&mut self, w: usize) {
        if !self.blood_wires[w].IsCardiopulmonaryBypassWire {
            return;
        }
        // A null (GenRun's flipped flag) or destroyed machine: GetFinalIntensity's enabled read throws.
        let Some(c) = self.blood_wires[w].cardiopulmonaryBypass.filter(|&c| self.wire_physical_alive(self.cardiopulmonary_bypasses[c].physical)) else { crate::game::menu::npe() };
        let num = mathf::clamp01(self.cardiopulmonary_bypass_final_intensity(c));
        let Some(l) = self.wire_first_limb(w) else { crate::game::menu::npe() };
        if self.nodes[l].IsConnectedToRoot {
            let person = self.limbs[l].Person;
            self.people[person].OxygenLevel = mathf::max(0.5 * num, self.people[person].OxygenLevel);
        }
        self.circulations[l].ArtificialHeartbeat = self.cardiopulmonary_bypasses[c].TargetHeartrate * num;
        self.circulations[l].BloodFlow = mathf::max(self.circulations[l].BloodFlow, num);
    }

    /// `limbA ? limbA : limbB` with a destroyed component reading as null.
    fn wire_first_limb(&self, w: usize) -> Option<usize> {
        let alive = |l: Option<usize>| l.filter(|&l| self.wire_physical_alive(self.limbs[l].PhysicalBehaviour));
        alive(self.blood_wires[w].limbA).or_else(|| alive(self.blood_wires[w].limbB))
    }

    /// WireBehaviour.FixedUpdate (WireBehaviour.Tick, empty here: the base) and BloodWireBehaviour.Tick
    pub fn blood_wire_fixed_update(&mut self, w: usize) {
        if !self.blood_wires[w].started {
            return;
        }
        // Energy and Copper run it inside energy_wire_tick (their Tick's base.Tick).
        if matches!(self.blood_wires[w].kind, WireKind::Cable | WireKind::Chain | WireKind::Rigid | WireKind::Rope | WireKind::NewUse | WireKind::Use(..)) {
            self.distance_wire_tick(w);
        }
        match self.blood_wires[w].kind {
            WireKind::Blood => {}
            // SliderJointWireBehaviour has no Tick.
            WireKind::Cable | WireKind::Chain | WireKind::Rigid | WireKind::FixedCable | WireKind::Slider => return,
            WireKind::SteelBinding | WireKind::WoodenBinding => return self.binding_tick(w),
            WireKind::WoodStrut => return self.wood_strut_tick(w),
            // RopeBehaviour.Tick: base.Tick() (above), then the burn.
            WireKind::Rope => return self.wire_burn(w),
            WireKind::Energy => return self.energy_wire_tick(w),
            WireKind::Copper => {
                self.energy_wire_tick(w);
                return self.wire_burn(w);
            }
            WireKind::HeatPipe => return self.heat_pipe_tick(w),
            WireKind::Spring | WireKind::StrongSpring | WireKind::DestructibleSpring => return self.spring_cable_tick(w),
            WireKind::Bandage => return self.applied_bandage_fixed_update(w),
            WireKind::NewUse => return self.new_use_wire_tick(w),
            // UseWireBehaviour.Tick: the burn, while CanBreak.
            WireKind::Use(..) => {
                if self.blood_wires[w].CanBreak {
                    self.wire_burn(w);
                }
                return;
            }
        }
        if self.blood_wires[w].IsHeartRateWire {
            self.blood_wire_send_heart_rate(w);
        }
        let dt = self.time.fixedDeltaTime;
        let transfer_rate = dt * 2.0;
        let (a, b) = (self.blood_wires[w].containerA.filter(|&o| self.wire_container_alive(o)), self.blood_wires[w].containerB.filter(|&o| self.wire_container_alive(o)));
        if let (Some(a), Some(b)) = (a, b) {
            if self.blood_wire_can_transfer(a, b) {
                self.blood_wire_transfer(w, a, b, 1.0, transfer_rate);
            }
            self.blood_wire_average_pressure(dt, a, b);
        }
        if self.blood_wires[w].AreBothLimbs {
            let (la, lb) = (self.blood_wires[w].limbA.unwrap(), self.blood_wires[w].limbB.unwrap());
            let (fa, fb) = (self.circulations[la].BloodFlow, self.circulations[lb].BloodFlow);
            let mean = 0.5 * (fa + fb);
            if self.container_allows_pressure(Owner::Circulation(la)) && self.container_allows_pressure(Owner::Circulation(lb)) {
                self.circulations[la].BloodFlow = mathf::lerp(fa, mean, dt);
                self.circulations[lb].BloodFlow = mathf::lerp(fb, mean, dt);
            }
        }
        let wire = &mut self.blood_wires[w];
        wire.flow = mathf::lerp(wire.flow, 0.0, 0.1);
        wire.color = wire.color.map(|c| mathf::lerp(c, 0.0, 0.1));
    }

    /// BloodWireBehaviour.SendHeartRate
    pub(crate) fn blood_wire_send_heart_rate(&mut self, w: usize) {
        // heartLimb.Limb is read first: a null heartLimb throws.
        let Some(heart) = self.blood_wires[w].heartLimb else { super::menu::throw("NullReferenceException: heartLimb") };
        let Some(monitor) = self.blood_wires[w].heartMonitor else { super::menu::throw("NullReferenceException: heartMonitor") };
        let gone = self.physicals[self.limbs[heart].PhysicalBehaviour].isDisintegrated || self.physicals[self.blood_wires[w].physical].isDisintegrated;
        if gone {
            self.heart_monitors[monitor].IsConnected = false;
        } else {
            self.heart_monitors[monitor].IsConnected = true;
            self.heart_monitors[monitor].ConnectedLimb = Some(heart);
        }
    }

    /// BloodWireBehaviour.Transfer: from the container at the higher MeasuredPressure to the other.
    pub(crate) fn blood_wire_transfer(&mut self, w: usize, a: Owner, b: Owner, visual_dir: f32, rate: f32) {
        let (pa, pb) = (self.bc(a).MeasuredPressure, self.bc(b).MeasuredPressure);
        if pa > pb {
            self.blood_wire_transfer_liquid(w, rate, a, b, visual_dir);
        } else if pb > pa {
            self.blood_wire_transfer_liquid(w, rate, b, a, -visual_dir);
        }
    }

    /// BloodWireBehaviour.TransferLiquid (a negative rate throws; the rate is fixedDeltaTime * 2).
    pub(crate) fn blood_wire_transfer_liquid(&mut self, w: usize, rate: f32, from: Owner, to: Owner, visual_direction: f32) {
        if rate < 0.0 {
            super::menu::throw("Exception: Rate can't be negative");
        }
        if self.bc(from).TotalLiquidAmount > self.container_limits(from)[0]
            && self.bc(to).TotalLiquidAmount < self.upper_limit(to)
            && self.transfer_to(from, rate, to).abs() > mathf::EPSILON
        {
            self.delete_empty_liquid_entries(from);
            self.delete_empty_liquid_entries(to);
            let fallback = self.bc(from).cachedComputedColour;
            let computed = self.get_computed_color(from, fallback);
            let wire = &mut self.blood_wires[w];
            wire.flow = mathf::lerp(wire.flow, visual_direction, rate);
            for i in 0..4 {
                wire.color[i] = mathf::lerp(wire.color[i], computed[i], rate);
            }
        }
    }

    /// BloodWireBehaviour.AveragePressure
    pub(crate) fn blood_wire_average_pressure(&mut self, rate: f32, from: Owner, to: Owner) {
        if rate < 0.0 {
            super::menu::throw("Exception: Rate can't be negative");
        }
        let (a, b) = (self.bc(from).MeasuredPressure, self.bc(to).MeasuredPressure);
        let mean = 0.5 * (a + b);
        if self.container_allows_pressure(from) && self.container_allows_pressure(to) {
            self.bc_mut(from).MeasuredPressure = mathf::lerp(a, mean, rate);
            self.bc_mut(to).MeasuredPressure = mathf::lerp(b, mean, rate);
        }
    }

    /// DistanceJointWireBehaviour.OnPreFlip / FixedJointWireBehaviour.OnPreFlip (SpringCableBehaviour has neither): a
    /// distance wire remembers its distance and stops autoconfiguring it, a fixed wire stops autoconfiguring its anchor.
    pub(crate) fn blood_wire_on_pre_flip(&mut self, w: usize) {
        let (kind, f) = (self.blood_wires[w].kind, self.blood_wires[w].joint);
        // SpringCableBehaviour and SliderJointWireBehaviour have no flip messages.
        if kind.is_spring() || kind.is_slider() {
            return;
        }
        if kind.is_fixed() {
            self.fixed_joint_set_auto_configure(f, false);
        } else {
            self.blood_wires[w].distanceBeforeFlip = self.distance_joint_distance(f);
            self.distance_joint_set_auto_configure_distance(f, false);
        }
    }

    /// FixedJointWireBehaviour.GetReferenceAngle: transform.eulerAngles.z - otherPhysicalBehaviour.transform.eulerAngles.z (wire body `me`, welded body `o`).
    pub(crate) fn fixed_wire_get_reference_angle(&self, me: usize, o: usize) -> f32 {
        (d(super::dump::euler_z(self.physicals[me].transform.rotation)) - super::dump::euler_z(self.physicals[o].transform.rotation)).f()
    }

    /// FixedJointWireBehaviour.OnChangePause, FixedJointWireBehaviour.Awake's and OnDestroy's OnChangePause listener (Game::change_pause calls every live fixed wire): after
    /// a drag in pause mode, a wire whose end has turned against the object it is welded to reattaches the joint's body once the game runs again;
    /// with that object gone, the joint's connectedBody is cleared.
    pub(crate) fn fixed_wire_on_change_pause(&mut self, w: usize, stopped: bool) {
        let (me, f) = (self.blood_wires[w].physical, self.blood_wires[w].joint);
        if self.blood_wires[w].hasMovedInPauseMode && !stopped && !self.fixed_joints[f].destroyed {
            match self.blood_wires[w].other.filter(|&o| self.wire_physical_alive(o)) {
                Some(o) => {
                    let now = self.fixed_wire_get_reference_angle(me, o);
                    if !mathf::approximately(now, self.blood_wires[w].originalRotation) {
                        self.fixed_joint_set_connected_body(f, None);
                        self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(o)));
                        self.blood_wires[w].originalRotation = now;
                    }
                }
                None => self.fixed_joint_set_connected_body(f, None),
            }
        }
        self.blood_wires[w].hasMovedInPauseMode = false;
    }

    /// DistanceJointWireBehaviour.OnFlip, FixedJointWireBehaviour.OnFlip: the remembered distance is written back and the next fixed update turns
    /// autoConfigureDistance on again. FixedJointWireBehaviour.OnFlip: a connected body is detached and attached again
    /// (the joint is rebuilt on the flipped bodies), a joint to the world has its connectedAnchor mirrored about
    /// center.x; the next fixed update turns autoConfigureConnectedAnchor on again.
    pub(crate) fn blood_wire_on_flip(&mut self, w: usize, center: [f32; 3]) {
        let (kind, f) = (self.blood_wires[w].kind, self.blood_wires[w].joint);
        if kind.is_spring() || kind.is_slider() {
            return;
        }
        if kind.is_fixed() {
            if let Some(body) = self.fixed_joints[f].connected {
                self.fixed_joint_set_connected_body(f, None);
                self.fixed_joint_set_connected_body(f, Some(body));
            } else {
                let anchor = self.fixed_joints[f].state.connected_anchor;
                let num = anchor.x - center[0];
                self.fixed_joint_set_connected_anchor(f, [center[0] - num, anchor.y]);
            }
        } else {
            let before = self.blood_wires[w].distanceBeforeFlip;
            self.distance_joint_set_distance(f, before);
        }
        self.coroutine_seq += 1;
        let owner = self.blood_wires[w].id;
        self.fixed_update_waits.push(super::destroyable::Coroutine { seq: self.coroutine_seq, at: self.time.timeAsDouble, owner, from: i64::MIN, step: super::destroyable::Step::WireFlipReconfigure(w) });
    }

    /// Utils.NextFixedUpdateCoroutine: the action after WaitForFixedUpdate in OnFlip: autoConfigureDistance / autoConfigureConnectedAnchor back on.
    pub(crate) fn blood_wire_flip_reconfigure(&mut self, w: usize) {
        if self.destroyed_components.contains(&self.blood_wires[w].id) {
            return;
        }
        let (kind, f) = (self.blood_wires[w].kind, self.blood_wires[w].joint);
        if kind.is_fixed() {
            self.fixed_joint_set_auto_configure(f, true);
        } else {
            self.distance_joint_set_auto_configure_distance(f, true);
        }
    }

    /// Messages.ISlice.Slice of every wire on the physical (WireBehaviour.Slice for the plain ones): breakForce and breakTorque 0.
    pub(crate) fn blood_wire_slice(&mut self, p: usize) {
        for w in 0..self.blood_wires.len() {
            if self.blood_wires[w].physical == p && !self.destroyed_components.contains(&self.blood_wires[w].id) {
                self.wire_slice(w);
            }
        }
    }

    /// One wire's Slice: WireBehaviour.Slice (breakForce and breakTorque 0), or its kind's override.
    pub(crate) fn wire_slice(&mut self, w: usize) {
        // WoodStrutBehaviour.Slice: breakForceBuffer = 0 (below).
        // SteelBindingBehaviour.Slice and WoodStrutBehaviour.Slice: breakForceBuffer = 0 (below).
        // RopeBehaviour.Slice, CopperWireBehaviour.Slice, SpringCableBehaviour.Slice and UseWireBehaviour.Slice:
        // NewUseWireBehaviour.Slice: breakForceBuffer = 0, as UseWireBehaviour.Slice (the next Tick breaks it).
        // UseWireBehaviour.Slice (and the rope's, copper's and spring cable's):
        // SpringCableBehaviour.Slice: initialBreakForce = 0 (as RopeBehaviour.Slice, CopperWireBehaviour.Slice and UseWireBehaviour.Slice):
        // the next Tick breaks it (a use wire that cannot break keeps its infinite force).
        let kind = self.blood_wires[w].kind;
        if matches!(kind, WireKind::Rope | WireKind::Copper | WireKind::WoodStrut | WireKind::SteelBinding | WireKind::WoodenBinding | WireKind::Use(..) | WireKind::NewUse) || kind.is_spring() {
            self.blood_wires[w].breakForceBuffer = 0.0;
            return;
        }
        let f = self.blood_wires[w].joint;
        self.fixed_joint_set_break_force(f, 0.0);
        self.fixed_joint_set_break_torque(f, 0.0);
    }

    /// WireBehaviour.EdgeCollider: [Obsolete], always null.
    pub(crate) fn wire_edge_collider(&self, _w: usize) -> Option<usize> {
        None
    }

    /// WireBehaviour.OnJointBreak2D, WoodStrutBehaviour.JointBroken and WoodenBindingBehaviour.JointBroken, sent to the broken joint's GameObject: JointBroken is empty, then Destroy(this).
    pub(crate) fn blood_wire_on_joint_break(&mut self, f: usize) {
        for w in 0..self.blood_wires.len() {
            let id = self.blood_wires[w].id;
            if self.blood_wires[w].joint == f && !self.destroyed_components.contains(&id) {
                // SteelBindingBehaviour.JointBroken, WoodenBindingBehaviour.JointBroken and WoodStrutBehaviour.JointBroken: the same PlayClipOnce
                // (clips from `break_clips`, volume Random.value).
                // RopeBehaviour.JointBroken: PlayClipOnce(rope snap clips.PickRandom(), Random.value), the same draws.
                // Rope and copper JointBroken: PlayClipOnce(clips.PickRandom(), Random.value); the spring's has no
                // SpringCableBehaviour.JointBroken: PlayClipOnce(breakClips.PickRandom()); rope and copper JointBroken: PlayClipOnce(clips.PickRandom(), Random.value); the spring's has no
                // volume.
                // NewUseWireBehaviour.JointBroken: PlayClipOnce("Audio/use wire break" clips.PickRandom(), Random.value), UseWireBehaviour's draws.
                // UseWireBehaviour.JointBroken (and the rope's and copper's): PlayClipOnce(clips.PickRandom(), Random.value); the spring's
                // has no volume. Audio, the draws stay.
                // WireBehaviour.JointBroken: empty (the base, for the kinds without break clips).
                self.wire_joint_broken(w);
                self.tr_destroy_component(id, 0.0);
            }
        }
    }

    /// The wire's JointBroken (see blood_wire_on_joint_break): its break clip at a random volume, when it has one.
    pub(crate) fn wire_joint_broken(&mut self, w: usize) {
        let kind = self.blood_wires[w].kind;
        if let Some((folder, n)) = kind.break_clips() {
            let p = self.blood_wires[w].physical;
            if self.wire_physical_alive(p) {
                let clip = match folder {
                    Some(f) => self.pick_resource_clip(f, n),
                    None => self.pick_clip(0, "WoodStutSnapClips", n),
                };
                let volume = if kind.is_spring() { 1.0 } else { self.random.value() };
                self.play_clip_once(p, clip, volume);
            }
        }
    }

    /// The delayed Destroy of a BloodWireBehaviour (false: `id` is not one): RemoveFromManager, its coroutines stop, then
    /// OnDestroy.
    pub fn blood_wire_run_destroy(&mut self, id: i32) -> bool {
        let Some(w) = self.blood_wires.iter().position(|x| x.id == id) else { return false };
        self.component_destroys.retain(|&(_, i)| i != id);
        if self.destroyed_components.insert(id) {
            self.blood_wire_destroy(w);
        }
        true
    }

    /// A wire on a GameObject that is being destroyed: the Joint2D component goes first (it was added before the wire),
    /// then the wire.
    pub(super) fn blood_wire_object_destroy(&mut self, components: &[i32]) {
        for w in 0..self.blood_wires.len() {
            let id = self.blood_wires[w].id;
            if components.contains(&self.physicals[self.blood_wires[w].physical].id) && self.destroyed_components.insert(id) {
                self.component_destroys.retain(|&(_, i)| i != id);
                // A loaded wire whose Start has not run has no joint yet.
                if self.blood_wires[w].saved.is_none() || self.blood_wires[w].started {
                    self.run_joint_destroy(self.blood_wires[w].joint);
                }
                self.blood_wire_destroy(w);
            }
        }
    }

    fn blood_wire_destroy(&mut self, w: usize) {
        let id = self.blood_wires[w].id;
        self.stop_all_coroutines(id);
        if let Some((_, b)) = self.behaviour_of.remove(&id) {
            self.behaviours.remove(b);
        }
        // OnDestroy starts with InvisibilityTracker.Dispose(): a wire whose Start never ran has none (NullReferenceException).
        if self.blood_wires[w].started {
            self.blood_wire_on_destroy(w);
            if matches!(self.blood_wires[w].kind, WireKind::Energy | WireKind::Copper) {
                self.energy_wire_on_destroy(w);
            }
        }
    }

    /// WireBehaviour.SetColor: startColor, endColor and WireColor (one field here).
    pub(crate) fn wire_set_color(&mut self, w: usize, color: [f32; 4]) {
        self.blood_wires[w].WireColor = color;
    }

    /// WireBehaviour.OnDestroy (the base body alone): InvisibilityTracker.Dispose(), then ModdingApi.Events.OnWireDestroyed.Invoke(this).
    /// The undo action leaves the history (FindRelevantAction, DeregisterAction); the Wire child and the joint are destroyed
    /// (Destroy(this) is a no-op now). OnCableDisconnect has no receiver in 1.28.41.
    pub(crate) fn wire_base_on_destroy(&mut self, w: usize) {
        let (id, f) = (self.blood_wires[w].id, self.blood_wires[w].joint);
        self.tracker_dispose(super::tool_invisibility::Owner::Wire(w));
        self.mod_event(&super::mods::mod_api::OnWireDestroyed, id);
        self.undo_deregister_related(id);
        if !self.fixed_joints[f].destroyed && !self.joint_destroys.iter().any(|&(_, j)| j == f) {
            self.destroy_joint_delayed(f);
        }
    }

    /// WireBehaviour.OnDestroy, then BloodWireBehaviour.OnDestroy.
    pub(crate) fn blood_wire_on_destroy(&mut self, w: usize) {
        // UseWireBehaviour.OnDestroy: RemoveListener(OnContinuousUpdate); the continuous update skips a destroyed wire.
        self.wire_base_on_destroy(w);
        if self.blood_wires[w].IsHeartRateWire {
            if let Some(m) = self.blood_wires[w].heartMonitor.filter(|&m| self.wire_physical_alive(self.heart_monitors[m].physical)) {
                self.heart_monitors[m].IsConnected = false;
            }
        }
        if self.blood_wires[w].IsCardiopulmonaryBypassWire {
            if let Some(c) = self.blood_wires[w].cardiopulmonaryBypass.filter(|&c| self.wire_physical_alive(self.cardiopulmonary_bypasses[c].physical)) {
                // `(limbA ? limbA : limbB).ArtificialHeartbeat = 0` throws when both limbs are destroyed, which ends the method.
                let Some(l) = self.wire_first_limb(w) else { return };
                self.circulations[l].ArtificialHeartbeat = 0.0;
                self.cardiopulmonary_bypasses[c].ValidConnectionCount -= 1;
                let cid = self.cardiopulmonary_bypasses[c].id;
                self.with_component_instance(cid, |g, inst, _| g.cardiopulmonary_bypass_update_activation(c, inst));
            }
        }
        if self.blood_wires[w].kind == WireKind::NewUse {
            self.new_use_wire_on_destroy(w);
        }
        // FixedJointWireBehaviour.OnDestroy: after base.OnDestroy, the OnChangePause listener goes (Game::change_pause skips a destroyed
        // wire) and Destroy(otherEnd): the CableOtherEndBehaviour has no OnDestroy and no state here (`otherEnd` is read off the wire).
    }
}

/// A WireBehaviour loaded onto an object without a PhysicalBehaviour (contraption 1872008384 saves a FixedCableBehaviour on the
/// Car root, whose PhysicalBehaviours are all on child objects): Awake's GetComponent<PhysicalBehaviour>() is null, so
/// Initialise throws after making the line and calling Created, and the rest of Start never runs. The LineRenderer keeps the
/// settings Initialise gave it (useWorldSpace, so a fixed wire's local vertices are drawn as world points; numCapVertices 6 and
/// Tile but where Created changes them; the stored width), and InvisibilityTracker stays null.
#[derive(Default, Clone, Debug)]
pub struct RootWire {
    pub kind: WireKind,
    pub id: i32,
    /// Its object's Transform id.
    pub object: i32,
    pub saved: SavedWire,
    pub usedToHaveConnectedBody: bool,
    pub started: bool,
    /// otherPhysicalBehaviour (a saved reference), then the connected body's PhysicalBehaviour.
    pub other: Option<usize>,
    /// The joint (`fixed_joints`) and the Rigidbody2D its RequireComponent added (`node_rigidbodies`).
    pub joint: usize,
    pub body: usize,
    pub WireColor: [f32; 4],
    pub WireWidth: f32,
    pub localRenderEndpoint: [f32; 2],
    pub Invisible: bool,
    /// The breakForceBuffer of SteelBindingBehaviour, RopeBehaviour, CopperWireBehaviour, WoodStrutBehaviour and
    /// UseWireBehaviour: its initialiser 1 (the `= currentBreakingForce` after base.Start never runs).
    pub breakForceBuffer: f32,
    /// SpringCableBehaviour.initialBreakForce: currentBreakingForce (set before Initialise).
    pub initialBreakForce: f32,
    /// DistanceJointWireBehaviour.distanceBeforeFlip
    pub distanceBeforeFlip: f32,
    /// UseWireBehaviour.Color, Channel and CanBreak and AppliedBandageBehaviour.SoakAmount: stored members (Start never gets
    /// to read them).
    pub Color: [f32; 4],
    pub Channel: u16,
    pub CanBreak: bool,
    pub SoakAmount: f32,
    /// AppliedBandageBehaviour.SoakColour (1.28.45).
    pub SoakColour: [f32; 4],
    pub shouldIgnoreUpdate: bool,
    pub IsMouseInsideBounds: bool,
    pub IsMouseOverlapping: bool,
    /// `vertices`: GetVertexCount() + 1 points.
    pub vertices: Vec<[f32; 3]>,
    /// FixedJointWireBehaviour.positionsCached; for the other wires, Update has set the line's positions.
    pub positionsCached: bool,
    /// The LineRenderer: its world-space positions (a new one's (0,0,0), (0,0,1); positionCount GetVertexCount() + 1) and colours.
    pub line: Vec<[f32; 3]>,
    pub start_color: [f32; 4],
    pub end_color: [f32; 4],
    /// The object (and the Wire child with it) was destroyed.
    pub gone: bool,
}

impl RootWire {
    pub fn new(kind: WireKind, id: i32, object: i32, saved: SavedWire) -> Self {
        Self {
            kind,
            id,
            object,
            saved,
            usedToHaveConnectedBody: false,
            started: false,
            other: None,
            joint: usize::MAX,
            body: usize::MAX,
            WireColor: [1.0; 4],
            WireWidth: 0.0,
            localRenderEndpoint: [0.0; 2],
            Invisible: false,
            breakForceBuffer: 1.0,
            initialBreakForce: f32::INFINITY,
            distanceBeforeFlip: 0.0,
            // UseWireBehaviour.Color's initialiser (1, 11, 0).
            Color: [1.0, 11.0, 0.0, 1.0],
            Channel: 0,
            CanBreak: false,
            SoakAmount: 0.0,
            SoakColour: [1.0; 4],
            shouldIgnoreUpdate: false,
            IsMouseInsideBounds: false,
            IsMouseOverlapping: false,
            vertices: Vec::new(),
            positionsCached: false,
            line: vec![[0.0; 3], [0.0, 0.0, 1.0]],
            start_color: [1.0; 4],
            end_color: [1.0; 4],
            gone: false,
        }
    }

    /// A DistanceJointWireBehaviour: a DistanceJoint2D.
    fn is_distance(&self) -> bool {
        !self.kind.is_fixed() && !self.kind.is_spring() && !self.kind.is_slider()
    }
}

impl Game {
    /// The Start of a root wire. The `!typedJoint` branch of FixedJointWireBehaviour.Start, DistanceJointWireBehaviour.Start,
    /// SpringCableBehaviour.Start or SliderJointWireBehaviour.Start: AddComponent of the joint (its RequireComponent Rigidbody2D
    /// comes first, and is the root's first body) set up from the stored members as `wire_saved_joint` does; the fixed wire's
    /// enableCollision off, the slider's autoConfigureAngle on and useLimits off, the spring's autoConfigureDistance off,
    /// initialBreakForce and vertexCount. Then Initialise: the Wire child (GameObject, Transform, Optout, LineRenderer), Created,
    /// OnWireCreated, OnCableConnect to the other end (no receiver), and `physicalBehaviour.BroadcastMessage` throws a
    /// NullReferenceException. What each Start does after base.Start or Initialise never runs. The parts of the subclasses'
    /// Starts before base.Start load static audio clips only.
    pub fn root_wire_start(&mut self, r: usize) {
        let (id, object, kind) = (self.root_wires[r].id, self.root_wires[r].object, self.root_wires[r].kind);
        if self.destroyed_components.contains(&id) {
            return;
        }
        let s = self.root_wires[r].saved.clone();
        let rb_id = self.allocate_instance_id();
        self.allocate_instance_id();
        let k = self.with_component_instance(object, |g, inst, o| g.add_rigidbody2d_component(inst, o, rb_id));
        let on = BodyOwner::Node(k);
        let f = if kind.is_fixed() {
            self.add_fixed_joint_component_on(on)
        } else if kind.is_spring() {
            self.add_spring_joint_component_on(on)
        } else if kind.is_slider() {
            self.add_slider_joint_component_on(on)
        } else {
            self.add_distance_joint_component_on(on)
        };
        (self.root_wires[r].joint, self.root_wires[r].body) = (f, k);
        self.root_wires[r].started = true;
        let distance = self.root_wires[r].is_distance();
        if kind.is_fixed() {
            self.fixed_joint_set_auto_configure(f, true);
            self.fixed_joint_set_frequency(f, 0.0);
            self.fixed_joint_set_damping_ratio(f, 1.0);
        } else {
            self.fixed_joint_set_auto_configure(f, false);
            self.fixed_joint_set_enable_collision(f, true);
            if distance {
                self.distance_joint_set_auto_configure_distance(f, false);
            }
        }
        match self.root_wires[r].other.filter(|&o| self.wire_physical_alive(o)) {
            Some(o) => {
                self.fixed_joint_set_connected_body(f, Some(BodyOwner::Physical(o)));
                self.fixed_joint_set_connected_anchor(f, s.currentConnectedAnchor);
            }
            None => {
                // transform.root.TransformPoint
                let trs = self.root_wire_root_trs(r);
                let v = crate::unity::transform::global_position(&trs, [s.currentConnectedAnchor[0], s.currentConnectedAnchor[1], 0.0, 0.0]);
                self.fixed_joint_set_connected_anchor(f, [v[0], v[1]]);
            }
        }
        self.fixed_joint_set_anchor(f, s.currentAnchor);
        if distance {
            self.distance_joint_set_max_distance_only(f, s.currentMaxDistanceOnly);
            self.distance_joint_set_distance(f, s.currentDistance);
        }
        if !s.currentBreakingForce.is_infinite() {
            self.fixed_joint_set_break_force(f, s.currentBreakingForce);
        }
        if distance {
            self.distance_joint_set_auto_configure_distance(f, true);
        }
        let count = if kind.is_fixed() {
            self.fixed_joint_set_enable_collision(f, false);
            kind.vertex_count()
        } else if kind.is_slider() {
            self.slider_joint_set_auto_configure_angle(f, true);
            self.slider_joint_set_use_limits(f, false);
            kind.vertex_count()
        } else if kind.is_spring() {
            self.spring_joint_set_distance(f, s.targetDistance);
            self.fixed_joints[f].spring.as_mut().unwrap().auto_configure_distance = false;
            self.root_wires[r].initialBreakForce = s.currentBreakingForce;
            // vertexCount = Max(2, CeilToInt(distance)); GetVertexCount is vertexCount - 1.
            (self.spring_joint_distance(f) as f64).ceil().max(2.0) as usize - 1
        } else {
            kind.vertex_count()
        };
        // Initialise
        let w = &mut self.root_wires[r];
        if w.WireColor[3] < 0.01 || s.WireMaterialName == "DeleteWire" {
            w.Invisible = true;
        }
        w.vertices = vec![[0.0; 3]; count + 1];
        // positionCount: new positions are zero.
        w.line.resize(count + 1, [0.0; 3]);
        (w.start_color, w.end_color) = (w.WireColor, w.WireColor);
        for _ in 0..4 {
            self.allocate_instance_id();
        }
        if self.wire_connected_alive(f) {
            self.root_wires[r].usedToHaveConnectedBody = true;
            self.root_wires[r].other = match self.fixed_joints[f].connected {
                Some(BodyOwner::Physical(p)) => Some(p),
                _ => None,
            };
        }
        // Created. SteelBindingBehaviour.Created and WoodenBindingBehaviour.Created: the stress clips; WireColor and the line
        // white (numCapVertices 0 and the Background layer: `wires.rs`). WoodStrutBehaviour.Created: white, numCapVertices 0.
        // BloodWireBehaviour.Created: no physicalBehaviour, so the property block's _Flow 0 and _BloodColor magenta, which
        // OnWillRenderObject overwrites with flow and color (both 0: nothing transfers). EnergyWireBehaviour.Created (the copper
        // wire's): SetAntennaObjects and SetEggs need both ends.
        match kind {
            WireKind::SteelBinding => {
                self.wire_tool.stress_clips.get_or_insert((crate::il!(str "SteelBindingBehaviour::Created" 0), METAL_STRESS_CLIPS));
            }
            WireKind::WoodenBinding => self.wire_tool.stress_clips = Some((crate::il!(str "WoodenBindingBehaviour::Created" 0), WOODEN_STRESS_CLIPS)),
            _ => {}
        }
        if matches!(kind, WireKind::SteelBinding | WireKind::WoodenBinding | WireKind::WoodStrut) {
            let w = &mut self.root_wires[r];
            (w.WireColor, w.start_color, w.end_color) = ([1.0; 4], [1.0; 4], [1.0; 4]);
        }
        crate::game::menu::npe();
    }

    /// The root's Transform as a Trs (the wire's object is a root).
    fn root_wire_root_trs(&self, r: usize) -> crate::unity::transform::Trs {
        let mut n = self.node_rigidbodies[self.root_wires[r].body].node;
        while let Some(up) = self.transform_nodes[n].parent {
            n = up;
        }
        let (t, q) = self.node_world(n);
        crate::unity::transform::Trs { t: [t[0], t[1], t[2], 0.0], q, s: self.transform_nodes[n].trs.s }
    }

    /// `untypedJoint` of a root wire for a member read: a broken joint is destroyed (MissingReferenceException).
    fn root_wire_joint(&self, r: usize) -> usize {
        let f = self.root_wires[r].joint;
        if self.fixed_joints[f].destroyed {
            let class = if self.root_wires[r].kind.is_fixed() {
                "FixedJoint2D"
            } else if self.root_wires[r].kind.is_spring() {
                "SpringJoint2D"
            } else if self.root_wires[r].kind.is_slider() {
                "SliderJoint2D"
            } else {
                "DistanceJoint2D"
            };
            crate::game::menu::throw(format!(
                "MissingReferenceException: The object of type '{class}' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object."
            ));
        }
        f
    }

    /// WireBehaviour.Update, then the subclass's Update, of a root wire: FixedJointWireBehaviour.Update's cached local ends,
    /// DistanceJointWireBehaviour.Update's parabola (BloodWireBehaviour's ProcessCardiopulmonaryBypassWire after it: not a
    /// bypass wire), SpringCableBehaviour.Update's and SliderJointWireBehaviour.Update's straight lines (they do not look at
    /// shouldIgnoreUpdate).
    pub fn root_wire_update(&mut self, r: usize) {
        if !self.root_wires[r].started {
            // untypedJoint is null: `untypedJoint.connectedBody` throws.
            crate::game::menu::npe();
        }
        let other_gone = self.root_wires[r].other.is_some_and(|o| self.wire_physical_alive(o) && self.physicals[o].isDisintegrated);
        let destroy = other_gone || {
            let f = self.root_wire_joint(r);
            !self.wire_connected_alive(f) && self.root_wires[r].usedToHaveConnectedBody
        };
        if destroy {
            // Its OnDestroy throws at once (InvisibilityTracker.Dispose of null): the joint, the Rigidbody2D and the Wire child stay.
            self.root_wires[r].shouldIgnoreUpdate = true;
            self.tr_destroy_component(self.root_wires[r].id, 0.0);
        } else {
            // lineChild exists: Initialise made it before throwing.
            self.root_wire_check_mouse_input(r);
            let f = self.root_wire_joint(r);
            let force = self.fixed_joints[f].state.break_force;
            if !self.root_wires[r].IsMouseInsideBounds && self.ShowLimbStatus && !force.is_infinite() {
                let reaction = self.fixed_joint_reaction_force(f);
                let t = mathf::clamp01(mathf::sqrt(mathf::sqr_magnitude(reaction) / (force * force)));
                let c = self.root_wires[r].WireColor;
                let color = [mathf::lerp(c[0], 1.0, t), mathf::lerp(c[1], 0.0, t), mathf::lerp(c[2], 0.0, t), mathf::lerp(c[3], 1.0, t)];
                (self.root_wires[r].start_color, self.root_wires[r].end_color) = (color, color);
            }
        }
        let kind = self.root_wires[r].kind;
        let ignore = self.root_wires[r].shouldIgnoreUpdate;
        if kind.is_fixed() {
            if !ignore {
                self.root_fixed_wire_vertices(r);
            }
            return;
        }
        if ignore && self.root_wires[r].is_distance() {
            return;
        }
        let f = self.root_wire_joint(r);
        let count = self.root_wires[r].vertices.len() - 1;
        let v = if kind.is_slider() {
            self.slider_line(f).to_vec()
        } else if kind.is_spring() {
            self.spring_line(f, count)
        } else {
            self.wire_parabola(f, count)
        };
        let w = &mut self.root_wires[r];
        // lineRenderer.SetPositions(vertices)
        w.line.clone_from(&v);
        w.vertices = v;
        w.positionsCached = true;
    }

    /// FixedJointWireBehaviour.Update of a root wire: the line's local ends (GetVertexCount 1), kept until one moves by more
    /// than deltaThreshold; SetPositions when they changed.
    fn root_fixed_wire_vertices(&mut self, r: usize) {
        let f = self.root_wires[r].joint;
        let anchor = self.fixed_joints[f].state.anchor;
        let end = self.root_wires[r].localRenderEndpoint;
        let b = match (self.wire_connected_alive(f), self.fixed_joints[f].connected) {
            (true, Some(BodyOwner::Physical(o))) => {
                // transform.InverseTransformPoint(connectedBody.transform.TransformPoint(localRenderEndpoint))
                let world = self.transform_point3(o, [end[0], end[1], 0.0]);
                let trs = self.root_wire_root_trs(r);
                let (local, _) = crate::unity::transform::inverse(&trs, [world[0], world[1], world[2], 0.0], [0.0, 0.0, 0.0, 1.0]);
                [local[0], local[1]]
            }
            (true, _) => unreachable!("a root wire's joint connects only to a PhysicalBehaviour's body"),
            (false, _) => end,
        };
        let w = &mut self.root_wires[r];
        let moved = |x: [f32; 3], y: [f32; 2]| mathf::sqr_magnitude([x[0] - y[0], x[1] - y[1]]) > 0.001;
        if moved(w.vertices[0], [anchor.x, anchor.y]) || moved(w.vertices[1], b) {
            w.vertices = vec![[anchor.x, anchor.y, 0.0], [b[0], b[1], 0.0]];
            w.positionsCached = false;
        }
        if !w.positionsCached {
            w.line.clone_from(&w.vertices);
            w.positionsCached = true;
        }
    }

    /// Hover.CheckMouseInput of a root wire: the world-space line's bounds (its LineRenderer positions, which Update sets
    /// after this) and WireBehaviour.IsMouseInsideCollider: the mouse within WireWidth of a segment of `vertices` as world
    /// points (useWorldSpace). The delete key's CanBeDeleted reads InvisibilityTracker.ShouldBeVisible of null and throws;
    /// OnMouseEnter and OnMouseExit have no listener.
    fn root_wire_check_mouse_input(&mut self, r: usize) {
        let m = self.MousePosition;
        let w = &self.root_wires[r];
        let width = w.WireWidth;
        let inside_bounds = (0..2).all(|i| {
            let lo = w.line.iter().map(|v| v[i]).fold(f32::INFINITY, f32::min);
            let hi = w.line.iter().map(|v| v[i]).fold(f32::NEG_INFINITY, f32::max);
            let (centre, extent) = ((hi + lo) * 0.5, (hi - lo) * 0.5 + width * 0.5);
            m[i] > centre - extent && m[i] < centre + extent
        });
        let flag = inside_bounds && w.vertices.windows(2).any(|s| sqrd_distance_to_segment([m[0], m[1]], [s[0][0], s[0][1]], [s[1][0], s[1][1]]) <= width * width);
        let id = w.id;
        self.root_wires[r].IsMouseInsideBounds = inside_bounds;
        self.hover_track(id, inside_bounds, flag);
        if flag && self.input.held(crate::il!(str "Hover::CheckMouseInput" 0)) {
            crate::game::menu::npe();
        }
        self.root_wires[r].IsMouseOverlapping = flag;
    }

    /// WireBehaviour.FixedUpdate: Tick, with physicalBehaviour null. DistanceJointWireBehaviour.Tick keeps currentDistance at
    /// typedJoint.distance (the cable's, chain's, rigid wire's, energy and copper wire's, rope's and use wire's call it;
    /// BloodWireBehaviour.Tick, HeatPipeBehaviour.Tick and WoodStrutBehaviour.Tick do not, and move only render state here).
    /// The burn (rope, copper wire, wood strut, steel and wooden binding, a breakable use wire; the spring cable's with
    /// initialBreakForce while its joint lives): breakForce = buffer * (1 - the other end's BurnProgress), with no stress
    /// sound without a physicalBehaviour. AppliedBandageBehaviour's own FixedUpdate hides WireBehaviour's: no limb, nothing.
    pub fn root_wire_fixed_update(&mut self, r: usize) {
        let kind = self.root_wires[r].kind;
        if !self.root_wires[r].started || matches!(kind, WireKind::FixedCable | WireKind::Slider | WireKind::Bandage | WireKind::Blood | WireKind::HeatPipe) {
            return;
        }
        let burn = |g: &Game| g.root_wires[r].other.filter(|&o| g.wire_physical_alive(o)).map_or(0.0, |o| mathf::max(0.0, g.physicals[o].BurnProgress));
        if kind.is_spring() {
            let f = self.root_wires[r].joint;
            let initial = self.root_wires[r].initialBreakForce;
            if !self.fixed_joints[f].destroyed && !initial.is_infinite() {
                let force = (d(initial) * (d(1.0) - d(burn(self)))).f();
                self.fixed_joint_set_break_force(f, force);
            }
            return;
        }
        let f = self.root_wire_joint(r);
        if !kind.is_fixed() && kind != WireKind::WoodStrut {
            self.root_wires[r].saved.currentDistance = self.distance_joint_distance(f);
        }
        let burns = match kind {
            WireKind::Use(_, can_break) => can_break,
            _ => matches!(kind, WireKind::Rope | WireKind::Copper | WireKind::WoodStrut | WireKind::SteelBinding | WireKind::WoodenBinding),
        };
        if burns {
            let force = (d(self.root_wires[r].breakForceBuffer) * (d(1.0) - d(burn(self)))).f();
            self.fixed_joint_set_break_force(f, force);
        }
    }

    /// WireBehaviour.OnJointBreak2D of a root wire: JointBroken (every override's sound needs physicalBehaviour), Destroy(this).
    pub(super) fn root_wire_on_joint_break(&mut self, f: usize) {
        for r in 0..self.root_wires.len() {
            let id = self.root_wires[r].id;
            if self.root_wires[r].started && self.root_wires[r].joint == f && !self.destroyed_components.contains(&id) {
                self.tr_destroy_component(id, 0.0);
            }
        }
    }

    /// FixedJointWireBehaviour.OnPreFlip (`pre`) and OnFlip, or DistanceJointWireBehaviour's, of root wire component `id`, as
    /// `blood_wire_on_pre_flip` and `blood_wire_on_flip`. The spring cable and the slider have no flip messages.
    pub(super) fn root_wire_flip(&mut self, id: i32, pre: bool, center: [f32; 3]) {
        let Some(r) = self.root_wires.iter().position(|x| x.id == id) else { return };
        let (kind, f) = (self.root_wires[r].kind, self.root_wires[r].joint);
        if !self.root_wires[r].started || kind.is_spring() || kind.is_slider() {
            return;
        }
        if !kind.is_fixed() {
            // typedJoint.distance of a destroyed joint throws.
            let f = self.root_wire_joint(r);
            if pre {
                self.root_wires[r].distanceBeforeFlip = self.distance_joint_distance(f);
                self.distance_joint_set_auto_configure_distance(f, false);
                return;
            }
            self.distance_joint_set_distance(f, self.root_wires[r].distanceBeforeFlip);
        } else {
            // `(bool)typedJoint`
            if self.fixed_joints[f].destroyed {
                return;
            }
            if pre {
                self.fixed_joint_set_auto_configure(f, false);
                return;
            }
            if let Some(body) = self.fixed_joints[f].connected {
                self.fixed_joint_set_connected_body(f, None);
                self.fixed_joint_set_connected_body(f, Some(body));
            } else {
                let anchor = self.fixed_joints[f].state.connected_anchor;
                let num = anchor.x - center[0];
                self.fixed_joint_set_connected_anchor(f, [center[0] - num, anchor.y]);
            }
        }
        // StartCoroutine(Utils.NextFixedUpdateCoroutine(...)): autoConfigureConnectedAnchor (autoConfigureDistance) back on; the
        // distance wire's lambda reads typedJoint without a check.
        let mut waited = false;
        self.co_start(id, Box::new(move |g| {
            if !std::mem::replace(&mut waited, true) {
                return Yield::WaitForFixedUpdate;
            }
            if kind.is_fixed() {
                if !g.fixed_joints[f].destroyed {
                    g.fixed_joint_set_auto_configure(f, true);
                }
            } else {
                if g.fixed_joints[f].destroyed {
                    crate::game::menu::throw(
                        "MissingReferenceException: The object of type 'DistanceJoint2D' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.",
                    );
                }
                g.distance_joint_set_auto_configure_distance(f, true);
            }
            Yield::Done
        }));
    }

    /// The delayed Destroy of a root wire (false: `id` is not one).
    pub fn root_wire_run_destroy(&mut self, id: i32) -> bool {
        if !self.root_wires.iter().any(|x| x.id == id) {
            return false;
        }
        self.component_destroys.retain(|&(_, i)| i != id);
        if self.destroyed_components.insert(id) {
            self.stop_all_coroutines(id);
            if let Some((_, b)) = self.behaviour_of.remove(&id) {
                self.behaviours.remove(b);
            }
        }
        true
    }

    /// A root wire on a GameObject being destroyed: a live one's OnDestroy throws at InvisibilityTracker.Dispose (null), so
    /// only the component goes; its joint goes with the object's body and its line with the Wire child.
    pub(super) fn root_wire_object_destroy(&mut self, components: &[i32]) {
        for r in 0..self.root_wires.len() {
            let id = self.root_wires[r].id;
            if !components.contains(&id) {
                continue;
            }
            self.root_wires[r].gone = true;
            if self.destroyed_components.insert(id) {
                self.component_destroys.retain(|&(_, i)| i != id);
                self.stop_all_coroutines(id);
                if let Some((_, b)) = self.behaviour_of.remove(&id) {
                    self.behaviours.remove(b);
                }
            }
        }
    }
}

/// HeatPipeBehaviour.GetHeatTransferFactor
pub const HEAT_PIPE_FACTOR: f32 = 0.2;

/// WoodStrutBehaviour.lastStressSoundTime's initialiser (static): float.MinValue.
pub const LAST_STRESS_SOUND_TIME: f32 = f32::MIN;
