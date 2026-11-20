//! ToolControllerBehaviour with its SelectionController and DragTool (the tool the game starts with), fed by
//! InputSystem's controls and Global.MousePosition.
use super::signal::Act;
use super::*;
use crate::b2::math::Vec2 as V;
use crate::b2::revolute::RevoluteDef;
use crate::b2::world::{BodyId, JointId, Shape};

/// InputSystem.Down / Held / Up of one control this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Control {
    pub down: bool,
    pub held: bool,
    pub up: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Input {
    /// camera.ScreenToWorldPoint(Input.mousePosition) at the camera's distance: what Global.UpdateMousePosition
    /// stores this frame.
    pub mouse: [f32; 3],
    /// The bound controls by InputSystem name; a missing one reads false.
    pub controls: HashMap<String, Control>,
    /// Input.mousePosition and the main camera, in a window (recordings carry only the world position).
    pub screen: Option<Screen>,
    /// The main camera's orthographic view (world min, max), in a window.
    pub view: Option<([f32; 2], [f32; 2])>,
    /// Input.GetKeyDown / GetKey / GetKeyUp by KeyCode name (keycode.rs); a missing one reads false.
    pub keys: HashMap<String, Control>,
    /// A dialog box, text entry or menu is open (DialogBox.IsAnyDialogboxOpen || Global.ActiveUiBlock).
    pub ui_block: bool,
    /// !Global.main.UiEnabled (the toggleUi key hid the canvas; the UI's `hide_ui`).
    pub ui_hidden: bool,
    /// The window CameraControlBehaviour reads (None: headless).
    pub window: Option<super::camera_control::WindowInput>,
    /// A recording's Global.main.UILock (`uilock=`): the game's EventSystem under the mouse, which a replay has not.
    pub ui_lock: Option<bool>,
}

/// Input.mousePosition (pixels from the bottom left) and the orthographic camera's ScreenToWorldPoint, world =
/// origin + pixel × scale.
#[derive(Clone, Copy, Debug, Default)]
pub struct Screen {
    pub mouse: [f32; 2],
    pub origin: [f32; 2],
    pub scale: f32,
    /// The camera itself (x, y, orthographicSize, pixelWidth, pixelHeight) for WorldToScreenPoint's own matrix
    /// arithmetic; None outside a recording.
    pub view: Option<[f32; 5]>,
}

impl Input {
    /// Input.mousePosition and the main camera's view: the window's, or a recording's `screen` (load_input).
    pub fn screen(&self) -> Screen {
        self.screen.expect("Input.mousePosition: a recording made before the recorder wrote `screen=` (re-record it)")
    }
}

impl Screen {
    pub fn to_world(&self, p: [f32; 2]) -> [f32; 2] {
        [self.origin[0] + p[0] * self.scale, self.origin[1] + p[1] * self.scale]
    }
}

impl Input {
    /// Input.GetKeyDown/GetKey/GetKeyUp(code) together.
    pub fn key(&self, code: i32) -> Control {
        super::keycode::name(code).and_then(|n| self.keys.get(n)).copied().unwrap_or_default()
    }
    /// InputSystem.Down: the action's IsPressed (a name that is not an action reads false; the warning is only logged).
    pub fn down(&self, name: &str) -> bool {
        self.controls.get(name).is_some_and(|c| c.down)
    }
    pub fn held(&self, name: &str) -> bool {
        self.controls.get(name).is_some_and(|c| c.held)
    }
    /// InputSystem.Up: the action's IsReleased (a name that is not an action reads false; the warning is only logged).
    pub fn up(&self, name: &str) -> bool {
        self.controls.get(name).is_some_and(|c| c.up)
    }
}

/// The HingeJoint2D DragTool adds to the ToolController: its JointMotor2D and useMotor, and the b2 joint.
#[derive(Clone, Debug)]
pub struct DragJoint {
    /// The HingeJoint2D's instance id (AddComponent takes the next one; Object.Destroy names it).
    pub id: i32,
    pub joint: Option<JointId>,
    /// JointMotor2D.motorSpeed (degrees per second) and maxMotorTorque.
    pub motorSpeed: f32,
    pub maxMotorTorque: f32,
    pub useMotor: bool,
    /// When Object.Destroy's DelayedCallManager entry runs.
    pub destroy_at: Option<f64>,
}

/// The ToolController object: ToolControllerBehaviour, its SelectionController, the DragTool and their Rigidbody2D.
#[derive(Clone, Debug)]
pub struct Tool {
    pub body: BodyId,
    /// The ToolController Transform (a root object).
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    // ToolControllerBehaviour
    pub SelectionLayer: u32,
    pub isCurrentlyHolding: bool,
    pub currentlyHovering: Option<usize>,
    pub isSelecting: bool,
    pub selectionDragStart: [f32; 2],
    // SelectionController (physicals)
    pub rawSelection: Vec<usize>,
    pub hover: Option<usize>,
    // DragTool
    pub offset: [f32; 2],
    pub dragJoint: Option<DragJoint>,
    /// Destroyed drag joints whose delayed destroy has not run yet.
    pub destroying: Vec<DragJoint>,
    pub controlRotation: bool,
    pub selectedRigidbody: Option<usize>,
    pub rotationSpeed: f32,
    pub currentRotation: f32,
    pub snappedMouseDelta: [f32; 3],
    /// DragTool.objectsToMoveInPauseMode (a HashSet<Transform>: insertion order while nothing is removed): the physicals' Transforms.
    pub objectsToMoveInPauseMode: Vec<usize>,
    pub prevMousePos: [f32; 3],
    pub initialSnappingFinished: bool,
    pub isActive: bool,
    pub shouldSendContinuousUseMessage: bool,
    pub initialTranslationSnapOffset: [f32; 3],
    pub ActiveSingleSelected: Option<usize>,
    /// The current tool is the GentleDragTool (a DragTool whose OnSelect, MoveToMouse and three functions differ).
    pub gentle: bool,
    /// The GentleDragTool component has been added to the ToolController.
    pub gentle_added: bool,
    /// ToolControllerBehaviour.parentCycleIndex / childCycleIndex / childCount: the tool-number keys cycle a tool's children.
    pub parentCycleIndex: i32,
    pub childCycleIndex: i32,
    pub childCount: i32,
    /// ToolWheelBehaviour.CurrentTab: ToolTab.Tools 0, ToolTab.Powers 1.
    pub tab: i32,
    /// ToolWheelBehaviour.ToolVisibility == Visibility.Invisible.
    pub invisible: bool,
    /// The ToolLibrary entry name of the tool SetTool chose last (what the tool wheel highlights).
    pub chosen: String,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            body: Default::default(),
            position: Default::default(),
            rotation: Default::default(),
            SelectionLayer: Default::default(),
            isCurrentlyHolding: Default::default(),
            currentlyHovering: Default::default(),
            isSelecting: Default::default(),
            selectionDragStart: Default::default(),
            rawSelection: Default::default(),
            hover: Default::default(),
            offset: Default::default(),
            dragJoint: Default::default(),
            destroying: Default::default(),
            controlRotation: Default::default(),
            selectedRigidbody: Default::default(),
            rotationSpeed: 1.0,
            currentRotation: Default::default(),
            snappedMouseDelta: Default::default(),
            objectsToMoveInPauseMode: Default::default(),
            prevMousePos: Default::default(),
            initialSnappingFinished: Default::default(),
            isActive: Default::default(),
            shouldSendContinuousUseMessage: Default::default(),
            initialTranslationSnapOffset: Default::default(),
            ActiveSingleSelected: Default::default(),
            gentle: Default::default(),
            gentle_added: Default::default(),
            parentCycleIndex: Default::default(),
            childCycleIndex: Default::default(),
            childCount: Default::default(),
            tab: Default::default(),
            invisible: Default::default(),
            chosen: Default::default(),
        }
    }
}

/// One ToolLibrary.Tool: its Name, Type and Parent (empty for none); a mod's (ModAPI.RegisterTool) also its Description and Icon.
#[derive(Clone, Debug, Default)]
pub struct ToolEntry {
    pub name: String,
    pub ty: String,
    pub parent: String,
    pub description: String,
    pub icon: Option<super::legacy_mods::Picture>,
}

impl ToolEntry {
    /// Tool.Equals(object): `obj is Tool tool` and the Types are equal (ToolEntry is the only Tool there is, so the type test always holds).
    pub fn Equals(&self, other: &ToolEntry) -> bool {
        self.ty == other.ty
    }

    /// Tool.GetHashCode: 2049151605 + EqualityComparer<string>.Default.GetHashCode(Type) (Mono's string hash), wrapping.
    pub fn GetHashCode(&self) -> i32 {
        2049151605i32.wrapping_add(super::decal_renderer::string_hash(&self.ty))
    }

    /// Tool.op_Equality: left.Equals(right).
    pub fn op_Equality(left: &ToolEntry, right: &ToolEntry) -> bool {
        left.Equals(right)
    }

    /// Tool.op_Inequality: !(left == right).
    pub fn op_Inequality(left: &ToolEntry, right: &ToolEntry) -> bool {
        !Self::op_Equality(left, right)
    }
}

#[cfg(test)]
mod tool_entry_tests {
    use super::ToolEntry;

    #[test]
    fn tools_are_equal_by_type() {
        let t = |name: &str, ty: &str| ToolEntry { name: name.into(), ty: ty.into(), ..Default::default() };
        let (a, b, c) = (t("Drag", "DragTool"), t("Other name", "DragTool"), t("Drag", "WireTool"));
        assert!(a.Equals(&b) && ToolEntry::op_Equality(&a, &b) && !ToolEntry::op_Inequality(&a, &b));
        assert!(ToolEntry::op_Inequality(&a, &c));
        assert_eq!(a.GetHashCode(), b.GetHashCode());
        // The game DLL: Tool{Type=""}.GetHashCode() = -1873958541, Type="ab" -1152185156.
        assert_eq!((t("", "").GetHashCode(), t("", "ab").GetHashCode()), (-1873958541, -1152185156));
        assert_ne!(a.GetHashCode(), c.GetHashCode());
    }
}

/// UserPreferences.ToolSelectionMode
pub const SELECTION_AREA: i32 = 0;
pub const SELECTION_MASS: i32 = 1;
pub const SELECTION_DEPTH: i32 = 2;

const DEG2RAD: f32 = f32::from_bits(0x3c8efa35);

/// A receiver of BroadcastMessage("Use"): a rewritten behaviour or a UseEventTrigger (its component id).
enum UseReceiver {
    Behaviour(Behaviour),
    Event(i32),
    /// PhysicalBehaviour.Use: ModdingApi.Events.OnItemActivated.Invoke(this) (the component's id).
    Physical(i32),
    /// UseBubbler: OnlyDirect, and the receivers of `transform.parent.SendMessage("Use")`: its scripts, and the use
    /// wires added to it (by its physical).
    Bubbler(bool, Vec<Behaviour>, Option<usize>),
    UseWire(usize),
}

/// The classes with a Use(ActivationPropagation) method, and their subclasses.
pub(super) const USE_RECEIVERS: &[&str] = &[
    "AcidSyringe", "ActivatorElectrodeBehaviour", "AdhesiveCouplerBehaviour", "AdrenalineSyringe",
    "AerialFaithPlateBehaviour", "AndroidLaserBehaviour", "ArchelixCasterBehaviour", "AtomBombBehaviour",
    "BatteryAttachmentBehaviour", "BeamCannonBehaviour", "BeamRepeaterBehaviour", "BeamformerBehaviour",
    "BellBehaviour", "BlasterBehaviour", "BloodTankBehaviour", "BoatMotorBehaviour", "BoneEatingPoisonSyringe",
    "BulbBehaviour", "CarBehaviour", "CardiopulmonaryBypassMachineBehaviour", "CathodeLightBehaviour",
    "CentrifugeBehaviour", "ChainsawBehaviour", "CoagulationSyringe", "ConveyorBeltBehaviour", "CounterBehaviour",
    "DeathSyringe", "DecimatorBehaviour", "DefibrillatorBehaviour", "DetectorBehaviour", "DisplayBehaviour",
    "DroneBehaviour", "EBikeBehaviour", "EMPBehaviour", "EffectGunBehaviour", "ElectricityTransformerBehaviour",
    "EmptySyringe", "EnergySwordBehaviour", "ExcaliburBehaviour", "ExplosiveAttachmentBehaviour",
    "ExplosiveBehaviour", "FanBehaviour", "FirearmAttachmentBehaviour", "FirearmBehaviour", "FireworksBehaviour",
    "FlareBehaviour", "FlashlightAttachmentBehaviour", "ForcefieldGeneratorBehaviour", "FreezeGunBehaviour",
    "FreezeSyringe", "GeneratorBehaviour", "GenericScifiWeapon40Behaviour", "GlowtubeBehaviour", "GripBehaviour",
    "GyrostabiliserBehaviour", "HealthGunBehaviour", "HeartMonitorBehaviour", "HeatingElementBehaviour",
    "HoverThrusterBehaviour", "HoverboxBehaviour", "HovercarBehaviour", "ImmobilityFieldBehaviour",
    "IncendiaryAttachmentBehaviour", "IndustrialGyrostabiliserBehaviour", "InfraredThermometer", "InverterBehaviour",
    "IonThrusterBehaviour", "JetEngineBehaviour", "JukeboxBehaviour", "KnockoutSyringe", "LEDBulbBehaviour",
    "LagboxBehaviour", "LaserBehaviour", "LaserTurretBehaviour", "LifeSyringe", "LiquidDuplicatorBehaviour",
    "LiquidPressureValveBehaviour", "LiquidPumpBehaviour", "LiquidentifierBehaviour", "MachineGunBehaviour",
    "MagnetBehaviour", "MendingSyringe", "MetronomeBehaviour", "MinigunBehaviour", "MotorisedHingeBehaviour",
    "ObstructionEmitterBehaviour", "PainKillerSyringe", "ParticleMachineBehaviour", "PhysicalBehaviour",
    "PinkSyringe", "PistonBehaviour", "PointerBehaviour", "ProjectileLauncherBehaviour", "PulseDrumBehaviour",
    "RailgunBehaviour", "RayCannonBehaviour", "RaygunBehaviour", "RocketLauncherBehaviour", "RotorBehaviour",
    "ScherbewerferBehaviour", "ScopeAttachmentBehaviour", "SeismicChargeBehaviour", "SignalConverterBehaviour",
    "SingleFloodlightBehaviour", "SirenBehaviour", "SpikeGrenadeBehaviour", "SyringeBehaviour", "TankBarrelBehaviour",
    "TelevisionBehaviour", "TeslaCoilBehaviour", "ThermometerBehaviour", "ThrusterbedBehaviour", "TimedGateBehaviour",
    "TransmitterBehaviour", "TruckDoorBehaviour", "UltraStrengthSyringe", "UniversalBellBehaviour", "UseBubbler",
    "UseEventTrigger", "UseWireBehaviour", "ValveBehaviour", "WaterBreathingSyringe", "WinchBehaviour",
    "ZombieSyringe",
];

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// b2PolygonShape::TestPoint (180bb6060) and b2CircleShape::TestPoint (180bb5f20).
pub(super) fn test_point(shape: &Shape, xf: &crate::b2::math::Transform, p: V) -> bool {
    let (s, c) = (xf.q.s, xf.q.c);
    match shape {
        Shape::Polygon(poly) => {
            let (dx, dy) = (p.x - xf.p.x, p.y - xf.p.y);
            let local = V::new(c * dx + s * dy, c * dy - s * dx);
            let polygon_radius = crate::b2::settings::globals().polygon_radius;
            let n = poly.count;
            let sep = |i: usize| (local.x - poly.vertices[i].x) * poly.normals[i].x + (local.y - poly.vertices[i].y) * poly.normals[i].y;
            if !(polygon_radius < poly.radius) {
                return (0..n).all(|i| !(sep(i) > 0.0));
            }
            // A rounded polygon: within its extra radius of the first edge the point is outside.
            let Some(i) = (0..n).find(|&i| sep(i) > 0.0) else { return true };
            let near = nearest_point_on_segment(local, poly.vertices[i], poly.vertices[if i + 1 == n { 0 } else { i + 1 }]);
            let r = poly.radius - polygon_radius;
            let (nx, ny) = (near.x - local.x, near.y - local.y);
            r * r >= ny * ny + nx * nx
        }
        Shape::Circle(circle) => {
            let cx = (circle.p.x * c - s * circle.p.y) + xf.p.x;
            let cy = (s * circle.p.x + circle.p.y * c) + xf.p.y;
            let (dx, dy) = (p.x - cx, p.y - cy);
            circle.radius * circle.radius >= dy * dy + dx * dx
        }
        // b2ChainShape::TestPoint (180bb5de0): within the radius less b2_polygonRadius of any segment.
        Shape::Capsule(capsule) => capsule.test_point(xf, p),
        Shape::Chain(chain) => {
            let (dx, dy) = (p.x - xf.p.x, p.y - xf.p.y);
            let local = V::new(c * dx + s * dy, c * dy - s * dx);
            let r = chain.radius - crate::b2::settings::globals().polygon_radius;
            chain.vertices.windows(2).any(|w| {
                let near = nearest_point_on_segment(local, w[0], w[1]);
                let (nx, ny) = (near.x - local.x, near.y - local.y);
                r * r >= nx * nx + ny * ny
            })
        }
    }
}

use crate::b2::shape::nearest_point_on_segment;

impl Game {
    /// Physics2D.OverlapPointNonAlloc(point, buffer[16], layerMask, minDepth, maxDepth) as the physicals hit:
    /// PhysicsQuery2D::OverlapPointAll (180bf4d70) runs b2World::QueryAABB on the point; ReportFixture (180bf72d0)
    /// skips triggers (queriesHitTriggers is off), other layers, Transforms outside the depth range, a collider
    /// already hit and shapes that don't contain the point; the hits go by ascending Transform z (stable).
    pub(super) fn overlap_point(&mut self, point: [f32; 3], mask: u32, min_depth: f32, max_depth: f32) -> Vec<usize> {
        self.overlap_point_buffer(point, mask, min_depth, max_depth, 16)
    }

    /// overlap_point into a buffer of `buffer` colliders (SkinMaterialHandler.colliderBuffer's own length, for one).
    pub(super) fn overlap_point_buffer(&mut self, point: [f32; 3], mask: u32, min_depth: f32, max_depth: f32, buffer: usize) -> Vec<usize> {
        let mut hits = self.overlap_point_colliders(point, mask, min_depth, max_depth);
        // NonAlloc copies the first `buffer` of the sorted results.
        hits.truncate(buffer);
        // Every caller takes the collider's PhysicalBehaviour (TryGetComponent, the victim's GameObject or
        // PhysicalObjectsInWorldByTransform): a bare collider only fills its buffer slot.
        hits.into_iter().filter_map(|h| h.0).collect()
    }

    /// PhysicsQuery2D::OverlapPointAll's colliders, in order, with their physicals.
    pub(super) fn overlap_point_colliders(&mut self, point: [f32; 3], mask: u32, min_depth: f32, max_depth: f32) -> Vec<(Option<usize>, i32)> {
        self.sync_pending();
        let p = V::new(point[0], point[1]);
        self.overlap_query(self.world.query_point(p), mask, min_depth, max_depth, |shape, _, xf| test_point(shape, xf, p))
    }

    /// Physics2D.OverlapPoint(point, mask): the lowest-z collider at the point.
    pub fn overlap_point_first(&mut self, point: [f32; 2], mask: u32) -> Option<(Option<usize>, i32)> {
        self.overlap_point_depth(point, mask, f32::NEG_INFINITY, f32::INFINITY)
    }

    /// Physics2D.OverlapPoint(point, mask, minDepth, maxDepth): the lowest-z collider at the point in the range.
    pub fn overlap_point_depth(&mut self, point: [f32; 2], mask: u32, min_depth: f32, max_depth: f32) -> Option<(Option<usize>, i32)> {
        self.sync_pending();
        let p = V::new(point[0], point[1]);
        self.overlap_query(self.world.query_point(p), mask, min_depth, max_depth, |shape, _, xf| test_point(shape, xf, p)).first().copied()
    }

    /// Physics2D.OverlapAreaAll(a, b, mask): OverlapAreaAllToBox_Internal's OverlapBoxAll at the midpoint, size
    /// |a − b|, angle 0, depths unbounded. OverlapBoxQuery2D: SetAsBox(max(size/2, 1e-4)) with b2_polygonRadius,
    /// its AABB at the identity, and TestShapeOverlap (b2TestOverlap) per fixture.
    fn overlap_area_all(&mut self, a: [f32; 2], b: [f32; 2], mask: u32) -> Vec<usize> {
        self.sync_pending();
        let point = V::new((a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5);
        let size = [(a[0] - b[0]).abs(), (a[1] - b[1]).abs()];
        let min = f32::from_bits(0x38d1b717);
        let shape = Shape::Polygon(crate::b2::shape::Polygon::set_as_box((size[0] * 0.5).max(min), (size[1] * 0.5).max(min), point, crate::b2::settings::globals().polygon_radius));
        let Shape::Polygon(poly) = &shape else { unreachable!() };
        let (mut lower, mut upper) = (poly.vertices[0], poly.vertices[0]);
        for v in &poly.vertices[1..4] {
            (lower, upper) = (V::new(lower.x.min(v.x), lower.y.min(v.y)), V::new(upper.x.max(v.x), upper.y.max(v.y)));
        }
        let r = poly.radius;
        let aabb = crate::b2::broadphase::Aabb { lower: V::new(lower.x - r, lower.y - r), upper: V::new(upper.x + r, upper.y + r) };
        let identity = crate::b2::math::Transform { p: V::ZERO, q: crate::b2::math::Rot::new(0.0) };
        // The selection keeps the colliders whose Transform is in PhysicalObjectsInWorldByTransform.
        self.overlap_query(self.world.query_aabb(&aabb), mask, f32::NEG_INFINITY, f32::INFINITY, |other, child, xf| shape.test_overlap(0, &identity, other, child, xf)).into_iter().filter_map(|h| h.0).collect()
    }

    /// Collider2D.OverlapCollider(ContactFilter2D, Collider2D[1024]) on the collider whose fixtures are `query`
    /// (OverlapColliderQuery2D, 180bf1240 and Overlap2DQueryBase::RunQuery): AutoSyncTransforms, then b2World::QueryAABB
    /// over the union of every child's ComputeAABB, ReportFixture's filters as `overlap_query` (the filter's depth
    /// and normal-angle tests are off), and IsFixtureValid: any child of the query shapes against the other shape's
    /// children. The hits are sorted by z with std::sort (`sort::msvc_sort`), after the query collider's own are dropped.
    pub(super) fn overlap_collider(&mut self, query: &[crate::b2::world::FixtureId], mask: u32) -> Vec<(Option<usize>, i32)> {
        self.overlap_collider_triggers(query, mask, false)
    }

    /// `overlap_collider` with the ContactFilter2D's useTriggers: sensors are hit too.
    pub(super) fn overlap_collider_triggers(&mut self, query: &[crate::b2::world::FixtureId], mask: u32, triggers: bool) -> Vec<(Option<usize>, i32)> {
        self.sync_pending();
        let shapes: Vec<(&Shape, crate::b2::math::Transform)> = query.iter().map(|&f| self.world.fixture(f)).map(|fx| (&fx.shape, self.world.body(fx.body).xf)).collect();
        let mut aabb: Option<crate::b2::broadphase::Aabb> = None;
        for &(shape, xf) in &shapes {
            for c in 0..shape.child_count() {
                let b = shape.compute_aabb(&xf, c);
                aabb = Some(match aabb {
                    None => b,
                    Some(a) => crate::b2::broadphase::Aabb { lower: V::new(a.lower.x.min(b.lower.x), a.lower.y.min(b.lower.y)), upper: V::new(a.upper.x.max(b.upper.x), a.upper.y.max(b.upper.y)) },
                });
            }
        }
        let Some(aabb) = aabb else { return Vec::new() };
        let mut hits = self.overlap_query_unsorted(self.world.query_aabb(&aabb), mask, triggers, f32::NEG_INFINITY, f32::INFINITY, |other, _, xf| {
            shapes.iter().any(|&(shape, xa)| (0..shape.child_count()).any(|ca| (0..other.child_count()).any(|cb| shape.test_overlap(ca, &xa, other, cb, xf))))
        });
        // ReportFixture skips the query collider's own fixtures.
        let own: Vec<i32> = query.iter().map(|&f| self.world.fixture(f).collider.id).collect();
        hits.retain(|h| !own.contains(&h.1));
        super::sort::msvc_sort(&mut hits, |a, b| a.2 < b.2);
        hits.into_iter().map(|(ph, c, _)| (ph, c)).collect()
    }

    /// The overlap queries' shared filter: sensors skipped (QueriesHitTriggers off), the layer mask, the depth
    /// range on the Transform's z, each collider once, then std::sort by z (`sort::msvc_sort`). The callers run PhysicsQuery2D's
    /// AutoSyncTransforms (Physics2DSettings.autoSyncTransforms is on) before querying.
    /// The hits as (the collider's physical, or None for a GameObject without one; the Collider2D id).
    pub(super) fn overlap_query(&self, fixtures: Vec<(crate::b2::world::FixtureId, usize)>, mask: u32, min_depth: f32, max_depth: f32, test: impl Fn(&Shape, usize, &crate::b2::math::Transform) -> bool) -> Vec<(Option<usize>, i32)> {
        let mut hits = self.overlap_query_unsorted(fixtures, mask, false, min_depth, max_depth, test);
        super::sort::msvc_sort(&mut hits, |a, b| a.2 < b.2);
        hits.into_iter().map(|(ph, c, _)| (ph, c)).collect()
    }

    /// `overlap_query` before the sort: the hits with their z, in the broadphase's report order.
    fn overlap_query_unsorted(&self, fixtures: Vec<(crate::b2::world::FixtureId, usize)>, mask: u32, triggers: bool, min_depth: f32, max_depth: f32, test: impl Fn(&Shape, usize, &crate::b2::math::Transform) -> bool) -> Vec<(Option<usize>, i32, f32)> {
        let mut hits: Vec<(Option<usize>, i32, f32)> = Vec::new();
        for (f, child) in fixtures {
            let fx = self.world.fixture(f);
            if fx.is_sensor && !triggers {
                continue;
            }
            let Some(&layer) = self.world.layers.get(&fx.collider.game_object) else {
                panic!("OverlapPoint: no layer for collider {}", fx.collider.id);
            };
            if mask & (1 << layer) == 0 {
                continue;
            }
            let ph = self.physical_of_collider(fx.collider.id);
            let z = match ph {
                Some(ph) => self.physicals[ph].transform.position[2],
                None => self.bare_position(fx.collider.id)[2],
            };
            if z < min_depth || max_depth < z {
                continue;
            }
            if hits.iter().any(|&(_, c, _)| c == fx.collider.id) {
                continue;
            }
            if test(&fx.shape, child, &self.world.body(fx.body).xf) {
                hits.push((ph, fx.collider.id, z));
            }
        }
        hits
    }

    /// Transform.position of a bare collider's GameObject: its local position through its chain, then the anchor's.
    pub(super) fn bare_position(&self, collider: i32) -> [f32; 3] {
        let Some(b) = self.bare_colliders.get(&collider) else { panic!("collider {collider} is neither a physical's nor bare") };
        self.anchored_world(b.anchor, &b.chain).0
    }

    /// The Transforms of an object `chain` below `anchor`: the object first, then each ancestor up to the root.
    pub fn anchored_nodes(&self, anchor: super::BareAnchor, chain: &[crate::unity::transform::Trs]) -> Vec<crate::unity::transform::Trs> {
        let mut nodes = chain.to_vec();
        match anchor {
            super::BareAnchor::Physical(p) => nodes.extend(self.local_chain(p).into_iter().rev()),
            super::BareAnchor::Node(mut i) => loop {
                nodes.push(self.node_trs(i));
                let Some(up) = self.transform_nodes[i].parent else { break };
                i = up;
            },
            super::BareAnchor::Root => {}
        }
        nodes
    }

    /// Transform.InverseTransformVector (1807f29f0 -> 1807f27e0) of an object `chain` below `anchor`: each
    /// ancestor's inverse from the root down, then the object's.
    pub fn anchored_inverse_vector(&self, anchor: super::BareAnchor, chain: &[crate::unity::transform::Trs], v: [f32; 3]) -> [f32; 3] {
        let mut v = [v[0], v[1], v[2], 0.0];
        for node in self.anchored_nodes(anchor, chain).iter().rev() {
            v = crate::unity::transform::inverse_vector(node, v);
        }
        [v[0], v[1], v[2]]
    }

    /// Transform.position, rotation and the product of the local scales of an object `chain` (object first) below
    /// `anchor`.
    pub fn anchored_world(&self, anchor: super::BareAnchor, chain: &[crate::unity::transform::Trs]) -> ([f32; 3], [f32; 4], [f32; 3]) {
        use crate::unity::transform::{global_position, global_rotation};
        let nodes = self.anchored_nodes(anchor, chain);
        let (mut v, mut r, mut s) = ([0.0; 4], [0.0, 0.0, 0.0, 1.0], [1.0f32; 3]);
        for n in &nodes {
            (v, r) = (global_position(n, v), global_rotation(n, r));
            s = [s[0] * n.s[0], s[1] * n.s[1], s[2] * n.s[2]];
        }
        ([v[0], v[1], v[2]], r, s)
    }


    /// Transform.localToWorldMatrix of a physical object (CalculateGlobalMatrix up its chain).
    pub fn local_to_world(&self, p: usize) -> [f32; 16] {
        let nodes: Vec<crate::unity::transform::Trs> = self.local_chain(p).into_iter().rev().collect();
        crate::unity::transform::local_to_world(&nodes)
    }

    /// The physical's Transform ancestors, root first, then its own local TRS.
    pub(super) fn local_chain(&self, p: usize) -> Vec<crate::unity::transform::Trs> {
        use crate::unity::transform::{inverse, Trs};
        let mut chain = Vec::new();
        let mut n = self.physicals[p].parent;
        while let Some(i) = n {
            chain.push(self.node_trs(i));
            n = self.transform_nodes[i].parent;
        }
        chain.reverse();
        let t = &self.physicals[p].transform;
        let (pos, rot) = match t.local {
            Some((p, q)) => ([p[0], p[1], p[2], 0.0], q),
            None => {
                let (mut pos, mut rot) = ([t.position[0], t.position[1], t.position[2], 0.0], t.rotation);
                for node in &chain {
                    (pos, rot) = inverse(node, pos, rot);
                }
                (pos, crate::unity::transform::normalize(rot))
            }
        };
        let s = t.localScale;
        chain.push(Trs { t: pos, q: rot, s: [s[0], s[1], s[2], 0.0] });
        chain
    }

    /// Transform.InverseTransformPoint (1807f2600): InverseTransformPosition (180171750) from the root down.
    pub fn inverse_transform_point(&self, p: usize, point: [f32; 3]) -> [f32; 3] {
        let mut v = [point[0], point[1], point[2], 0.0];
        for node in &self.local_chain(p) {
            v = crate::unity::transform::inverse(node, v, [0.0, 0.0, 0.0, 1.0]).0;
        }
        [v[0], v[1], v[2]]
    }

    /// Transform.TransformPoint (1807f6830): CalculateGlobalPosition's step through the object, then each parent.
    pub fn transform_point3(&self, p: usize, point: [f32; 3]) -> [f32; 3] {
        let mut v = [point[0], point[1], point[2], 0.0];
        for node in self.local_chain(p).iter().rev() {
            v = crate::unity::transform::global_position(node, v);
        }
        [v[0], v[1], v[2]]
    }

    /// Transform.TransformDirection of a physical object.
    pub fn transform_direction(&self, p: usize, dir: [f32; 3]) -> [f32; 3] {
        let mut v = [dir[0], dir[1], dir[2], 0.0];
        for (i, node) in self.local_chain(p).iter().rev().enumerate() {
            v = crate::unity::transform::rotate_direction(node.q, (i > 0).then_some(node.s), v);
        }
        [v[0], v[1], v[2]]
    }

    /// Transform.rotation of a physical object: CalculateGlobalRotation up its chain.
    pub(super) fn transform_rotation(&self, p: usize) -> [f32; 4] {
        let chain = self.local_chain(p);
        let (leaf, parents) = chain.split_last().unwrap();
        parents.iter().rev().fold(leaf.q, |q, node| crate::unity::transform::global_rotation(node, q))
    }

    /// Transform.up of a physical object: rotation * Vector3.up.
    pub fn transform_up(&self, p: usize) -> [f32; 3] {
        mathf::rotate(self.transform_rotation(p), [0.0, 1.0, 0.0])
    }

    /// Transform.right of a physical object: rotation * Vector3.right.
    pub fn transform_right(&self, p: usize) -> [f32; 3] {
        mathf::rotate(self.transform_rotation(p), [1.0, 0.0, 0.0])
    }

    /// Transform.lossyScale of a physical object.
    pub fn lossy_scale(&self, p: usize) -> [f32; 3] {
        let mut nodes = self.local_chain(p);
        nodes.reverse();
        crate::unity::transform::lossy_scale(&nodes, self.physicals[p].transform.rotation)
    }

    /// DragTool.SnapToGrid
    pub(crate) fn snap_to_grid(&self, v: [f32; 3]) -> [f32; 3] {
        // WorldGridOffset.Offset: the Default map has no WorldGridOffset, so it stays zero.
        let offset = [0.0f32; 3];
        let s = 1.2136364f32 * self.prefs.GridSnap;
        // Utils.Snap(Vector3, float): each component Mathf.Round'ed to the step.
        let snap = |x: f32| mathf::snap(x, s);
        let w = [v[0] + offset[0], v[1] + offset[1], v[2] + offset[2]];
        sub3([snap(w[0]), snap(w[1]), snap(w[2])], offset)
    }

    /// DragTool.MousePosWithSnap
    pub(crate) fn mouse_pos_with_snap(&self) -> [f32; 3] {
        self.snap_to_grid(self.MousePosition)
    }

    /// ToolControllerBehaviour.FixedUpdate
    pub fn tool_fixed_update(&mut self) {
        if self.tool.is_none() || !self.input.held(crate::il!(str "ToolControllerBehaviour::FixedUpdate" 0)) {
            return;
        }
        // SpringJointWireTool.OnFixedHold is empty.
        // DistanceWireTool.OnFixedHold is empty.
        if let Some(power) = self.power {
            self.power_on_fixed_hold(power);
        } else if !self.wire_tool.chosen && !self.link_tool.chosen && !self.belt_tool.chosen && self.pin_tool.is_none() && !self.entity_tool_chosen() {
            // PinTool.OnFixedHold: empty (as DistanceWireTool, LinkDeviceTool, LightningTool and PyroPowerTool.OnFixedHold).
            self.drag_on_fixed_hold();
        }
    }

    /// ToolControllerBehaviour.Update (no menu, dialog or UI block is ever open).
    pub fn tool_update(&mut self) {
        let Some(t) = self.tool.as_mut() else { return };
        t.currentlyHovering = None;
        self.handle_tool_selection_input();
        // ToolControllerBehaviour.HandleIndirectInteraction
        if self.input.down("activateDirect") && self.tool.as_ref().unwrap().ActiveSingleSelected.is_none() {
            if let Some(p) = self.tool.as_ref().unwrap().hover {
                self.activate_direct(p);
            }
        }
        self.determine_hovering();
        self.handle_tools();
        // ToolControllerBehaviour.HandleContextMenu: ContextMenuBehaviour.Show (unless the pause menu is up) builds the dynamic buttons from the
        // selection now. UpdateSelectionBox draws the box.
        if self.input.down(crate::il!(str "ToolControllerBehaviour::HandleContextMenu" 0)) && !self.input.ui_block {
            self.context_menu = Some(self.context_menu_buttons());
            self.context_menu_show();
        }
    }

    /// Activations.SendOnce: target.OnSingleUse.Invoke(a); the ActivationToggleBehaviours are its only listeners.
    pub(crate) fn activations_send_once(&mut self, target: usize) {
        self.activation_toggle_use_direct(target);
    }

    /// Activations.SendContinuous: target.OnContinuousUse.Invoke(a) has no listener.
    pub(crate) fn activations_send_continuous(&mut self, _target: usize) {}

    /// A direct activation of a physical object: Activations.SendOnce, then BroadcastMessage("Use").
    pub(super) fn activate_direct(&mut self, p: usize) {
        // new ActivationPropagation(direct: true, 0, ..) with Target = the object.
        let a = self.signal_direct(true, 0);
        self.sig_mut(&a).Target = Some(p);
        self.activations_send_once(p);
        self.broadcast_use(p, &a);
    }

    /// PhysicalBehaviour.ForceSendUse
    /// BroadcastMessage("Use") on a physical object's GameObject: it and its active descendants, each object's
    /// components in order.
    /// PhysicalBehaviour.ForceSendUse: for each of ActivationPropagation.AllChannels (0, 1, 2),
    /// BroadcastMessage("Use", new ActivationPropagation(transform.root, channel, transform.root.gameObject), DontRequireReceiver).
    /// The root is the path's first object (not direct); every receiver below the object (its scripts, its use wires)
    /// reads the channel. The Sender is not read by any receiver the rewrite has.
    pub(crate) fn physical_force_send_use(&mut self, p: usize) {
        let root = self.root_id(p);
        for channel in 0..3 {
            let a = self.signal_from(root, channel);
            self.broadcast_use(p, &a);
        }
    }

    /// The Use receivers BroadcastMessage("Use") reaches from `start` of `inst`: each active object's components in order, its
    /// children, then the clones parented under it (the last of its children), such as an attachment re-parented below a firearm.
    fn use_receivers_below(&mut self, inst: &super::spawn::Instance, start: usize) -> Vec<UseReceiver> {
        let mut out = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        for x in std::iter::once(start).chain(inst.descendants(start)) {
            while let Some(&top) = open.last() {
                if inst.descendants(top).contains(&x) {
                    break;
                }
                open.pop();
                self.attached_use_receivers(inst.objects[top].components[0], &mut out);
            }
            open.push(x);
            if !inst.objects[x].active {
                continue;
            }
            // The use wires added to the object come after its own components.
            let go = inst.objects[x].components[0];
            let wired = self.physicals.iter().position(|ph| ph.transform_id == go && !ph.destroyed).map(|q| self.use_wires_on(q)).unwrap_or_default();
            for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                let crate::assets::db::Component::Script { class, fields, .. } = c else { continue };
                // The tools' use wires are the `wired` ones below.
                let v45 = crate::assets::il::is_1_28_45() && super::v45_new::USE_CLASSES.contains(&class.as_str());
                if !(USE_RECEIVERS.contains(&class.as_str()) || v45) || class == "UseWireBehaviour" {
                    continue;
                }
                if class == "PhysicalBehaviour" {
                    out.push(UseReceiver::Physical(inst.objects[x].components[k]));
                    continue;
                }
                if class == "UseEventTrigger" {
                    out.push(UseReceiver::Event(inst.objects[x].components[k]));
                    continue;
                }
                if class == "UseBubbler" {
                    let only_direct = match self.use_bubblers.get(&inst.objects[x].components[k]) {
                        Some(&v) => v,
                        None => fields.get("OnlyDirect").and_then(crate::assets::mono::Value::bool).unwrap_or_else(|| unimplemented!("mod-only: UseBubbler without OnlyDirect")),
                    };
                    // A root's transform.parent is null: the NullReferenceException ends the message.
                    let parent = inst.objects[x].parent.map_or(Vec::new(), |up| {
                        let scripts = inst.prefab.objects[up].components.iter().enumerate();
                        scripts.filter_map(|(k, (_, c))| match c {
                            crate::assets::db::Component::Script { class, .. } if USE_RECEIVERS.contains(&class.as_str()) => match self.behaviour_of.get(&inst.objects[up].components[k]) {
                                Some(&(_, b @ Behaviour::Car(_))) => Some(b),
                                _ => unimplemented!("mod-only: {class}.Use from a UseBubbler"),
                            },
                            _ => None,
                        }).collect()
                    });
                    let wired = inst.objects[x].parent.and_then(|up| {
                        let go = inst.objects[up].components[0];
                        self.physicals.iter().position(|ph| ph.transform_id == go && !ph.destroyed)
                    });
                    out.push(UseReceiver::Bubbler(only_direct, parent, wired));
                    continue;
                }
                // A cloned person's hands: GripBehaviour has no behaviour_of entry (link.rs keeps it in grips by id).
                if class == "GripBehaviour" {
                    if let Some(gr) = self.grips.iter().position(|gr| gr.id == inst.objects[x].components[k]) {
                        out.push(UseReceiver::Behaviour(Behaviour::Grip(gr)));
                        continue;
                    }
                }
                match self.behaviour_of.get(&inst.objects[x].components[k]) {
                    Some(&(_, b @ (Behaviour::Inverter(_) | Behaviour::ObstructionEmitter(_) | Behaviour::Counter(_) | Behaviour::Pointer(_) | Behaviour::Transmitter(_) | Behaviour::Coupler(_) | Behaviour::Jukebox(_) | Behaviour::Winch(_) | Behaviour::Conveyor(_) | Behaviour::Hovercar(_) | Behaviour::HoverThruster(_) | Behaviour::Fireworks(_) | Behaviour::RayCannon(_) | Behaviour::Railgun(_) | Behaviour::BeamCannon(_) | Behaviour::BeamRepeater(_) | Behaviour::AndroidLaser(_) | Behaviour::Lagbox(_) | Behaviour::ParticleMachine(_) | Behaviour::LiquidDuplicator(_) | Behaviour::PulseDrum(_) | Behaviour::Liquidentifier(_) | Behaviour::Centrifuge(_) | Behaviour::LiquidPump(_) | Behaviour::Firearm(_) | Behaviour::Glowtube(_) | Behaviour::Led(_) | Behaviour::Detector(_) | Behaviour::Explosive(_) | Behaviour::Launcher(_) | Behaviour::Bell(_) | Behaviour::MachineGun(_) | Behaviour::Minigun(_) | Behaviour::Generator(_) | Behaviour::Floodlight(_) | Behaviour::SpikeGrenade(_) | Behaviour::HeatingElement(_) | Behaviour::Bulb(_) | Behaviour::CathodeLight(_) | Behaviour::Thrusterbed(_) | Behaviour::Piston(_) | Behaviour::Car(_) | Behaviour::TruckDoor(_) | Behaviour::Scherbewerfer(_) | Behaviour::Needler(_) | Behaviour::Raygun(_) | Behaviour::Plane(_) | Behaviour::Syringe(_) | Behaviour::JetEngine(_) | Behaviour::Chainsaw(_) | Behaviour::Emp(_) | Behaviour::Decimator(_) | Behaviour::ImmobilityField(_) | Behaviour::EnergySword(_) | Behaviour::SignalConverter(_) | Behaviour::Metronome(_) | Behaviour::ActivatorElectrode(_) | Behaviour::Thermometer(_) | Behaviour::InfraredThermometer(_) | Behaviour::Blaster(_) | Behaviour::Beamformer(_) | Behaviour::Scifi40(_) | Behaviour::HealthGun(_) | Behaviour::FreezeGun(_) | Behaviour::Flare(_) | Behaviour::Display(_) | Behaviour::Laser(_) | Behaviour::LaserTurret(_) | Behaviour::FirearmAttachment(_) | Behaviour::BatteryAttachment(_) | Behaviour::Television(_) | Behaviour::UniversalBell(_) | Behaviour::Valve(_) | Behaviour::TimedGate(_) | Behaviour::Defibrillator(_) | Behaviour::CardiopulmonaryBypass(_) | Behaviour::HeartMonitor(_) | Behaviour::Excalibur(_) | Behaviour::BloodTank(_) | Behaviour::ArchelixCaster(_) | Behaviour::Grip(_) | Behaviour::TeslaCoil(_) | Behaviour::AtomBomb(_) | Behaviour::SeismicCharge(_) | Behaviour::Drone(_))))=> out.push(UseReceiver::Behaviour(b)),
                    Some(&(_, b @ Behaviour::Translated(t))) if t.has_use() => out.push(UseReceiver::Behaviour(b)),
                    Some(&(_, b @ Behaviour::V45(_))) if v45 => out.push(UseReceiver::Behaviour(b)),
                    _ => unimplemented!("mod-only: {class}.Use"),
                }
            }
            out.extend(wired.into_iter().map(UseReceiver::UseWire));
        }
        while let Some(top) = open.pop() {
            self.attached_use_receivers(inst.objects[top].components[0], &mut out);
        }
        out
    }

    /// The Use receivers of the clones parented under the Transform `key` (Instantiate(.., parent), or a re-parented body).
    fn attached_use_receivers(&mut self, key: i32, out: &mut Vec<UseReceiver>) {
        for c in self.attached_clones.get(&key).cloned().unwrap_or_default() {
            if !self.destroyed_components.contains(&c.id) {
                let below = self.with_component_instance(c.id, |g, clone, o| g.use_receivers_below(clone, o));
                out.extend(below);
            }
        }
    }

    pub(super) fn broadcast_use(&mut self, p: usize, act: &Act) {
        let id = self.physicals[p].id;
        let receivers: Vec<UseReceiver> = match self.harness_limb(p) {
            // A limb of the recording's Human (rebuilt from the snapshot, no clone kept): its GameObject's Use
            // receivers are PhysicalBehaviour then the hands' GripBehaviour, then the use wires added to it; no child has one.
            Some(l) => std::iter::once(UseReceiver::Physical(id))
                .chain(self.limbs[l].GripBehaviour.map(|g| UseReceiver::Behaviour(Behaviour::Grip(g))))
                .chain(self.use_wires_on(p).into_iter().map(UseReceiver::UseWire))
                .collect(),
            None => return self.broadcast_use_object(id, act),
        };
        self.send_use(receivers, act);
    }

    /// BroadcastMessage("Use") on the cloned object owning component `id` (an item's root without a PhysicalBehaviour too).
    pub(super) fn broadcast_use_object(&mut self, id: i32, act: &Act) {
        let receivers = self.with_component_instance(id, |g, inst, start| g.use_receivers_below(inst, start));
        self.send_use(receivers, act);
    }

    fn send_use(&mut self, receivers: Vec<UseReceiver>, act: &Act) {
        for b in receivers {
            // SendMessage: a receiver's exception is logged and the next receiver still gets the message.
            let act = act.clone();
            self.unity_callback(move |g| g.use_receiver(b, &act));
        }
    }

    /// A limb of a recording's Human, rebuilt from the snapshot with no clone kept (the replays only).
    pub(super) fn harness_limb(&self, p: usize) -> Option<usize> {
        let id = self.physicals[p].id;
        let owns = |i: &super::spawn::Instance| i.objects.iter().any(|x| x.components.contains(&id));
        if self.unsynced.iter().any(owns) || self.retained.iter().any(owns) {
            return None;
        }
        self.limbs.iter().position(|l| l.PhysicalBehaviour == p)
    }

    /// UseBubbler.Use on component `id` alone (a direct call: its own receiver, not the object's BroadcastMessage).
    pub(crate) fn use_bubbler_use(&mut self, id: i32, act: &Act) {
        let receivers = self.with_component_instance(id, |g, inst, o| g.use_receivers_below(inst, o));
        let b = receivers.into_iter().find(|r| matches!(r, UseReceiver::Bubbler(..))).expect("a UseBubbler on its object");
        self.use_receiver(b, act);
    }

    /// One receiver of BroadcastMessage("Use").
    fn use_receiver(&mut self, b: UseReceiver, act: &Act) {
        // Each receiver reads the shared signal as it is now (a use wire may have set a direct one's channel).
        let (direct, channel) = (self.sig(act).Direct, self.sig(act).Channel);
        let b = match b {
            UseReceiver::UseWire(w) => {
                self.use_wire_receive(w, act, false);
                return;
            }
            UseReceiver::Event(id) => {
                self.use_event_trigger_use(id);
                return;
            }
            // PhysicalBehaviour.Use: ModdingApi.Events.OnItemActivated.Invoke(this).
            UseReceiver::Physical(id) => {
                self.mod_event(&super::mods::mod_api::OnItemActivated, id);
                self.extras_activated(id);
                return;
            }
            // UseBubbler.Use: parent.SendMessage("Use") unless OnlyDirect and not direct.
            UseReceiver::Bubbler(only_direct, parent, wired) => {
                if !only_direct || direct {
                    for b in parent {
                        let Behaviour::Car(i) = b else { unreachable!() };
                        self.car_use(i, self.sig(act).Channel as i32);
                    }
                    // The use wires added to the parent come after its own components.
                    if let Some(q) = wired {
                        for w in self.use_wires_on(q) {
                            self.use_wire_receive(w, act, false);
                        }
                    }
                }
                return;
            }
            UseReceiver::Behaviour(b) => b,
        };
        match b {
            Behaviour::Firearm(f) => self.firearm_use(f),
            Behaviour::ParticleMachine(i) => self.particle_machine_use(i),
            Behaviour::Grip(g) => self.grip_use(g),
            Behaviour::Explosive(e) => self.explosive_use(e),
            Behaviour::Launcher(l) => self.launcher_use(l),
            Behaviour::Bell(b) => self.bell_use(b),
            Behaviour::UniversalBell(b) => self.universal_bell_use(b),
            Behaviour::Counter(c) => self.counter_use(c, channel as u16),
            Behaviour::Inverter(i) => self.inverter_use(i),
            Behaviour::ObstructionEmitter(i) => self.obstruction_emitter_use(i),
            Behaviour::Pointer(i) => self.pointer_use(i),
            // The activation's channel.
            Behaviour::Valve(v) => self.valve_use(v, channel as i32),
            Behaviour::HoverThruster(h) => self.hover_thruster_use(h, channel as i32),
            Behaviour::Hovercar(h) => self.hovercar_use(h, channel as i32),
            Behaviour::Winch(w) => self.winch_use(w, channel as i32),
            Behaviour::Conveyor(c) => self.conveyor_use(c),
            Behaviour::Jukebox(j) => self.jukebox_use(j),
            Behaviour::Coupler(c) => self.coupler_use(c),
            Behaviour::Transmitter(t) => self.transmitter_use(t, act),
            Behaviour::LiquidDuplicator(l) => self.liquid_duplicator_use(l, channel as i32),
            Behaviour::PulseDrum(i) => self.pulse_drum_use(i),
            Behaviour::Liquidentifier(l) => self.liquidentifier_use(l, channel as i32),
            Behaviour::Centrifuge(c) => self.centrifuge_use(c),
            Behaviour::LiquidPump(l) => self.liquid_pump_use(l, channel as i32),
            Behaviour::TimedGate(g) => self.timed_gate_use(g),
            Behaviour::Lagbox(g) => self.lagbox_use(g),
            Behaviour::Defibrillator(d) => self.defibrillator_use(d),
            Behaviour::CardiopulmonaryBypass(c) => self.cardiopulmonary_bypass_use(c),
            Behaviour::HeartMonitor(h) => self.heart_monitor_use(h),
            Behaviour::Excalibur(e) => self.excalibur_use(e),
            Behaviour::BloodTank(t) => self.blood_tank_use(t, channel as i32),
            Behaviour::ArchelixCaster(c) => self.archelix_caster_use(c),
            Behaviour::MachineGun(m) => self.machine_gun_use(m),
            Behaviour::Minigun(m) => self.minigun_use(m),
            Behaviour::Translated(t) => self.tr_use(t, super::translated::rt::Activation { Channel: channel as i32 }),
            Behaviour::Glowtube(g) => self.glowtube_use(g),
            Behaviour::CathodeLight(i) => self.cathode_light_use(i),
            Behaviour::Led(i) => self.led_use(i),
            Behaviour::Detector(i) => self.detector_use(i),
            Behaviour::Generator(i) => self.generator_use(i),
            Behaviour::Floodlight(i) => self.floodlight_use(i),
            Behaviour::SpikeGrenade(i) => self.spike_grenade_use(i),
            Behaviour::HeatingElement(i) => self.heating_element_use(i),
            Behaviour::Bulb(i) => self.bulb_use(i),
            Behaviour::Thrusterbed(i) => self.thrusterbed_use(i),
            Behaviour::Piston(i) => self.piston_use(i),
            Behaviour::Car(i) => self.car_use(i, channel as i32),
            Behaviour::TruckDoor(i) => self.truck_door_use(i),
            Behaviour::Syringe(i) => self.syringe_use(i, channel as i32),
            Behaviour::Scherbewerfer(i) => self.scherbewerfer_use(i),
            Behaviour::Needler(i) => self.needler_use(i),
            Behaviour::Raygun(i) => self.raygun_use(i),
            Behaviour::Plane(i) => self.plane_use(i),
            Behaviour::JetEngine(i) => self.jet_engine_use(i),
            // ChainsawBehaviour.Use: PlayClipOnce(Rev) when not destroyed (audio).
            Behaviour::Chainsaw(_) => {}
            Behaviour::Emp(i) => self.emp_use(i),
            Behaviour::LaserTurret(i) => self.laser_turret_use(i),
            Behaviour::Decimator(i) => self.decimator_use(i),
            Behaviour::Drone(i) => self.drone_use(i),
            Behaviour::ImmobilityField(i) => self.immobility_field_use(i),
            Behaviour::Forcefield(i) => self.forcefield_use(i),
            // TeslaCoilBehaviour.Use() takes no parameter; SendMessage calls it all the same.
            Behaviour::TeslaCoil(i) => self.tesla_coil_use(i),
            Behaviour::AtomBomb(i) | Behaviour::SeismicCharge(i) => self.prefab_bomb_use(i),
            Behaviour::EnergySword(i) => self.energy_sword_use(i),
            // MetronomeBehaviour.Use: `base.enabled && !activation.Contains(base.transform.root)`.
            Behaviour::Metronome(m) => {
                if !self.signal_contains(act, self.root_id(self.metronomes[m].phys)) {
                    self.metronome_use(m)
                }
            }
            Behaviour::ActivatorElectrode(e) => self.activator_electrode_use(e, act),
            Behaviour::Thermometer(t) => self.thermometer_use(t),
            Behaviour::Flare(f) => self.flare_use(f),
            Behaviour::Display(d) => self.display_use(d),
            Behaviour::Laser(l) => self.laser_use(l),
            Behaviour::Television(t) => self.television_use(t),
            Behaviour::FirearmAttachment(a) | Behaviour::BatteryAttachment(a) => self.firearm_attachment_use(a, self.sig(act).Target),
            Behaviour::InfraredThermometer(i) => self.infrared_use(i),
            Behaviour::Blaster(i) => self.blaster_use(i),
            Behaviour::BeamCannon(i) => self.beam_cannon_shoot(i),
            Behaviour::BeamRepeater(i) => self.beam_repeater_use(i),
            Behaviour::AndroidLaser(i) => self.android_laser_use(i),
            Behaviour::RayCannon(i) => self.ray_cannon_shoot(i),
            Behaviour::Railgun(i) => self.railgun_shoot(i),
            Behaviour::Fireworks(i) => self.fireworks_use(i),
            Behaviour::Beamformer(i) => self.beamformer_use(i),
            Behaviour::Scifi40(i) => self.scifi40_use(i),
            Behaviour::HealthGun(i) | Behaviour::FreezeGun(i) => self.effect_gun_use(i),
            // The activation's Direct flag and channel.
            Behaviour::SignalConverter(i) => self.signal_converter_use(i, direct, channel),
            Behaviour::V45(v) => self.v45_use(v),
            _ => unreachable!(),
        }
    }

    /// ToolControllerBehaviour.DetermineHovering. Update clears currentlyHovering first, so there the held object's own
    /// check never matches; a direct call with currentlyHovering on the held object clears it and hovers nothing.
    pub(crate) fn determine_hovering(&mut self) {
        let t = self.tool.as_mut().unwrap();
        if t.ActiveSingleSelected.is_some() && t.currentlyHovering == t.ActiveSingleSelected {
            t.currentlyHovering = None;
            return self.determine_hovering_set();
        }
        let mouse = self.MousePosition;
        let best = match self.prefs.ToolSelectionMode {
            SELECTION_MASS => self.select_by_mass(mouse),
            SELECTION_AREA => self.select_by_size(mouse),
            SELECTION_DEPTH => self.select_by_depth(mouse),
            _ => self.tool.as_ref().unwrap().currentlyHovering,
        };
        self.tool.as_mut().unwrap().currentlyHovering = best;
        self.determine_hovering_set();
    }

    /// The physicals under the mouse for the SelectBy methods: OverlapPointNonAlloc(mousePos, buffer, SelectionLayer, -0.9, 0.9).
    /// The loop condition re-runs the query each pass; nothing changes between passes, so one query gives the same hits.
    fn selection_hits(&mut self, mouse: [f32; 3]) -> Vec<usize> {
        let mask = self.tool.as_ref().unwrap().SelectionLayer;
        self.overlap_point(mouse, mask, -0.9, 0.9)
    }

    /// SelectionPriority's pull (1.28.45 only: 1.28.41's DetermineHovering inlines these loops without it).
    fn selection_priority(&self, p: usize) -> bool {
        crate::assets::il::is_1_28_45() && self.physicals[p].SelectionPriority
    }

    /// ToolControllerBehaviour.SelectByMass: the least InitialMass (less 10000000 for SelectionPriority); the first of equals wins.
    fn select_by_mass(&mut self, mouse: [f32; 3]) -> Option<usize> {
        let (mut best, mut least) = (None, f32::MAX);
        for ph in self.selection_hits(mouse) {
            let mut v = self.physicals[ph].InitialMass;
            if self.selection_priority(ph) {
                v -= 10000000.0;
            }
            if v < least {
                least = v;
                best = Some(ph);
            }
        }
        best
    }

    /// ToolControllerBehaviour.SelectBySize: the least PhysicalBehaviour.ObjectAreaByMass (rigidbody.mass / TrueInitialMass * ObjectArea),
    /// less 10000000 for SelectionPriority.
    fn select_by_size(&mut self, mouse: [f32; 3]) -> Option<usize> {
        let (mut best, mut least) = (None, f32::MAX);
        for ph in self.selection_hits(mouse) {
            let p = &self.physicals[ph];
            let mut v = p.rigidbody.mass(&self.world) / p.TrueInitialMass * p.ObjectArea;
            if self.selection_priority(ph) {
                v -= 10000000.0;
            }
            if v < least {
                least = v;
                best = Some(ph);
            }
        }
        best
    }

    /// ToolControllerBehaviour.SelectByDepth: SortingLayer.GetLayerValueFromID(sortingLayerID) and sortingOrder (both +100000 for
    /// SelectionPriority); a higher layer wins, else a higher order.
    fn select_by_depth(&mut self, mouse: [f32; 3]) -> Option<usize> {
        let (mut best, mut depth_layer, mut depth_order) = (None, i32::MIN, i32::MIN);
        for ph in self.selection_hits(mouse) {
            let mut layer = crate::play::sorting_layer_index(self.physicals[ph].sprite_layer) as i32;
            let mut order = self.physicals[ph].sprite.map_or(0, |s| s.2 as i32);
            if self.selection_priority(ph) {
                layer = layer.wrapping_add(100000);
                order = order.wrapping_add(100000);
            }
            if layer > depth_layer {
                depth_layer = layer;
                depth_order = order;
                best = Some(ph);
            } else if order > depth_order {
                depth_order = order;
                best = Some(ph);
            }
        }
        best
    }

    /// DetermineHovering's end: SelectionController.SetHovering when CurrentlyUnderMouse is not currentlyHovering.
    fn determine_hovering_set(&mut self) {
        let t = self.tool.as_mut().unwrap();
        if t.hover != t.currentlyHovering {
            match t.currentlyHovering {
                // SelectionController.SetHovering
                Some(h) if self.physicals[h].Selectable => {
                    t.hover = Some(h);
                    self.refresh_outlines();
                }
                _ if t.hover.is_some() => {
                    t.hover = None;
                    self.refresh_outlines();
                }
                _ => {}
            }
        }
    }

    /// ToolControllerBehaviour.HandleToolSelectionInput: "tool1".."tool10" pick the n-th parentless entry of the current tab's
    /// collection; pressing the same number again cycles through that entry's children, then back to the parent (UISoundBehaviour Scroll).
    pub(crate) fn handle_tool_selection_input(&mut self) {
        let tab = self.tool.as_ref().unwrap().tab;
        let collection = self.tool_collection(tab);
        let mut num = 1usize;
        for tool in &collection {
            if !tool.parent.is_empty() {
                continue;
            }
            if num < 11 && self.input.down(&format!("tool{num}")) {
                let t = self.tool.as_ref().unwrap();
                if t.parentCycleIndex != num as i32 || t.childCount <= 0 {
                    self.set_tool_by_type_name(&tool.ty);
                    let t = self.tool.as_mut().unwrap();
                    t.childCycleIndex = -1;
                    t.parentCycleIndex = num as i32;
                    t.childCount = collection.iter().filter(|c| c.parent == tool.name).count() as i32;
                    // UISoundBehaviour.Main.Scroll()
                    self.ui_sound(super::audio::UiSoundKind::Scroll);
                    self.notify(format!("{} selected", tool.name));
                    break;
                }
                let t = self.tool.as_mut().unwrap();
                t.childCycleIndex += 1;
                if t.childCycleIndex >= t.childCount {
                    t.childCycleIndex = -1;
                    self.set_tool_by_type_name(&tool.ty);
                    // UISoundBehaviour.Main.Scroll()
                    self.ui_sound(super::audio::UiSoundKind::Scroll);
                    self.notify(format!("{} selected", tool.name));
                    break;
                }
                let cycle = t.childCycleIndex;
                let mut num2 = t.childCount - 1;
                for tool2 in &collection {
                    if tool2.parent == tool.name {
                        if num2 == cycle {
                            self.set_tool_by_type_name(&tool2.ty);
                            // UISoundBehaviour.Main.Scroll()
                            self.ui_sound(super::audio::UiSoundKind::Scroll);
                            self.notify(format!("{} selected", tool2.name));
                            return;
                        }
                        num2 -= 1;
                    }
                }
            }
            num += 1;
        }
    }

    /// ToolControllerBehaviour.GetRelevantCollection: ToolLibrary.Tools for the Tools tab, else Powers (read from the scene's
    /// ToolLibrary the first time).
    pub(crate) fn tool_collection(&mut self, tab: i32) -> Vec<ToolEntry> {
        if self.tool_library.is_none() {
            // A game replayed from a recording opens the game's data here.
            let db = self.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&crate::game_assets::find_game(None).expect("the game's data")).expect("the game's data")));
            self.tool_library = Some(Game::read_tool_library(db).expect("ToolLibrary in level1"));
        }
        let (tools, powers) = self.tool_library.as_ref().unwrap();
        if tab != 0 { powers.clone() } else { tools.clone() }
    }

    /// SelectionController.ClearSelection: OnItemDeselected for each live selected object, then the list empties.
    pub(crate) fn clear_selection(&mut self) {
        self.deselect_events();
        self.tool.as_mut().unwrap().rawSelection.clear();
        self.refresh_outlines();
        crate::ui::uifn("SelectionController.ClearSelection", || "0".into());
    }

    /// ModdingApi.Events.OnItemDeselected.Invoke for each live object of rawSelection, in order.
    fn deselect_events(&self) {
        for &p in &self.tool.as_ref().unwrap().rawSelection {
            if !self.physicals[p].destroyed {
                self.mod_event(&super::mods::mod_api::OnItemDeselected, self.physicals[p].id);
            }
        }
    }

    /// SelectionController.Deselect: OnItemDeselected for a live object, then it leaves the list.
    pub(crate) fn deselect(&mut self, ph: usize) {
        if !self.physicals[ph].destroyed {
            self.mod_event(&super::mods::mod_api::OnItemDeselected, self.physicals[ph].id);
        }
        self.tool.as_mut().unwrap().rawSelection.retain(|&p| p != ph);
        self.refresh_outlines();
        crate::ui::uifn("SelectionController.Deselect", || format!("{} {}", self.tool.as_ref().unwrap().rawSelection.len(), self.physicals[ph].name));
    }

    /// SelectionController.RefreshOutlines over Global.main.PhysicalObjectsInWorld: an object's outline shows while it
    /// is hovered, or selected under a tool that shows selection outlines (DragTool and GentleDragTool), except the held
    /// object under a tool that hides it (the same two), none while Global.main.UILock.
    pub(crate) fn refresh_outlines(&mut self) {
        // ToolBehaviour.HideHeldOutline: false; DragTool's override (GentleDragTool inherits it) is true.
        // ToolBehaviour.ShowOutlineOnlyOnHover: true; DragTool's override is false.
        let drag = !(self.lightning_chosen() || self.pyro_chosen() || self.wire_tool.chosen || self.link_tool.chosen || self.belt_tool.chosen || self.power.is_some() || self.pin_tool.is_some());
        let t = self.tool.as_ref().unwrap();
        let (hover, held) = (t.hover, t.ActiveSingleSelected);
        let selected = t.rawSelection.clone();
        for p in 0..self.physicals.len() {
            if self.physicals[p].destroyed || !self.physicals[p].in_world {
                continue;
            }
            let shown = (hover == Some(p) || (drag && selected.contains(&p))) && !(drag && held == Some(p)) && !self.ui_lock();
            self.set_show_outline(p, shown);
        }
        crate::ui::uifn("SelectionController.RefreshOutlines", || self.physicals.iter().filter(|p| !p.destroyed && p.in_world && p.showOutline).count().to_string());
    }

    /// SelectionController.Select(IEnumerable, multiple): not multiple empties the list first (no events); each selectable one
    /// not yet selected is added, with OnItemSelected while it lives.
    pub(crate) fn select_many(&mut self, hits: Vec<usize>, multiple: bool) {
        if !multiple {
            self.tool.as_mut().unwrap().rawSelection.clear();
        }
        for p in hits {
            let selectable = self.physicals[p].Selectable;
            let t = self.tool.as_mut().unwrap();
            if selectable && !t.rawSelection.contains(&p) {
                t.rawSelection.push(p);
                if !self.physicals[p].destroyed {
                    self.mod_event(&super::mods::mod_api::OnItemSelected, self.physicals[p].id);
                }
            }
        }
        self.refresh_outlines();
    }

    /// A plain select of physical `p` (the hidden capture harness; the tool does it for a click).
    pub fn select_for_test(&mut self, p: usize, multiple: bool) {
        self.select(p, multiple);
    }

    /// SelectionController.Select(PhysicalBehaviour, multiple): not multiple deselects (OnItemDeselected for each live one) and
    /// empties the list; a new one is added with OnItemSelected.
    pub(super) fn select(&mut self, ph: usize, multiple: bool) {
        if !self.physicals[ph].Selectable {
            return;
        }
        if !multiple {
            self.deselect_events();
            self.tool.as_mut().unwrap().rawSelection.clear();
        }
        let t = self.tool.as_mut().unwrap();
        if !t.rawSelection.contains(&ph) {
            t.rawSelection.push(ph);
            self.mod_event(&super::mods::mod_api::OnItemSelected, self.physicals[ph].id);
        }
        self.refresh_outlines();
        crate::ui::uifn("SelectionController.Select", || format!("{} {}", self.tool.as_ref().unwrap().rawSelection.len(), self.physicals[ph].name));
    }

    /// ToolControllerBehaviour.HandleTools
    pub(crate) fn handle_tools(&mut self) {
        let fast = self.input.held(crate::il!(str "ToolControllerBehaviour::HandleTools" 1));
        if self.input.down(crate::il!(str "ToolControllerBehaviour::HandleTools" 0)) && !self.ui_lock() {
            let t = self.tool.as_ref().unwrap();
            match t.currentlyHovering {
                Some(h) if fast && t.rawSelection.contains(&h) => {
                    self.deselect(h);
                    let t = self.tool.as_mut().unwrap();
                    if Some(h) == t.ActiveSingleSelected {
                        self.current_on_deselect();
                        let t = self.tool.as_mut().unwrap();
                        if !t.rawSelection.is_empty() {
                            t.ActiveSingleSelected = Some(t.rawSelection[0]);
                        }
                    }
                }
                Some(h) => {
                    let multiple = fast || t.rawSelection.contains(&h);
                    self.select(h, multiple);
                    if self.tool.as_ref().unwrap().ActiveSingleSelected.is_some() {
                        self.current_on_deselect();
                    }
                    self.tool.as_mut().unwrap().ActiveSingleSelected = Some(h);
                }
                None => {
                    self.escape_and_deselect();
                    // ToolBehaviour.UsesEmptyDrag: true for the other tools; DragTool.UsesEmptyDrag is false: StartSelectionDrag.
                    let drag = !self.wire_tool.chosen && !self.link_tool.chosen && !self.belt_tool.chosen && self.power.is_none() && self.pin_tool.is_none() && !self.entity_tool_chosen();
                    let t = self.tool.as_mut().unwrap();
                    // ToolControllerBehaviour.StartSelectionDrag: also SelectionBoxBehaviour.Main.transform.position = selectionDragStart; the box stays
                    // hidden until the drag passes 200 px² and UpdateSelectionDrag sets x and y before Show, so `ui::selection_box`
                    // draws it from selectionDragStart alone.
                    if drag && !t.isSelecting {
                        let screen = self.input.screen();
                        t.isSelecting = true;
                        t.selectionDragStart = screen.mouse;
                    }
                }
            }
            self.tool.as_mut().unwrap().isCurrentlyHolding = true;
            self.unity_callback(|g| g.current_on_select());
        }
        if self.input.down(crate::il!(str "ToolControllerBehaviour::HandleTools" 3)) && self.tool.as_ref().unwrap().currentlyHovering.is_some() {
            let t = self.tool.as_ref().unwrap();
            let h = t.currentlyHovering.unwrap();
            let multiple = fast || t.rawSelection.contains(&h);
            self.select(h, multiple);
        }
        if self.input.held(crate::il!(str "ToolControllerBehaviour::HandleTools" 0)) && self.tool.as_ref().unwrap().isCurrentlyHolding {
            self.unity_callback(|g| g.current_on_hold());
        }
        // The EventSystem's selected object is never part of the context menu.
        if self.input.up(crate::il!(str "ToolControllerBehaviour::HandleTools" 0)) {
            self.escape_and_deselect();
            self.stop_selection_drag(fast);
        }
    }

    /// ToolControllerBehaviour.StopSelectionDrag: a box dragged past √200 px selects what it overlaps (replacing
    /// the selection unless "fast" is held). The box UI is drawn by the window.
    pub(crate) fn stop_selection_drag(&mut self, fast: bool) {
        let t = self.tool.as_mut().unwrap();
        if !t.isSelecting {
            return;
        }
        t.isSelecting = false;
        let screen = self.input.screen();
        let start = t.selectionDragStart;
        let a = screen.to_world(start);
        let d = [start[0] - screen.mouse[0], start[1] - screen.mouse[1]];
        if d[0] * d[0] + d[1] * d[1] < 200.0 {
            return;
        }
        let mask = t.SelectionLayer;
        if !fast {
            self.clear_selection();
        }
        let hits = self.overlap_area_all(a, [self.MousePosition[0], self.MousePosition[1]], mask);
        self.select_many(hits, fast);
    }

    /// ToolControllerBehaviour.EscapeAndDeselect (the pointer is never over a NoDeselectUi).
    pub(crate) fn escape_and_deselect(&mut self) {
        self.current_on_deselect();
        let fast = self.input.held(crate::il!(str "ToolControllerBehaviour::EscapeAndDeselect" 0));
        let t = self.tool.as_mut().unwrap();
        t.ActiveSingleSelected = None;
        t.isCurrentlyHolding = false;
        let kept = t.currentlyHovering.is_some_and(|h| t.rawSelection.contains(&h));
        if !kept && !fast {
            t.rawSelection.clear();
        }
    }

    /// CurrentTool.OnSelect / OnHold / OnDeselect: the DragTool, or the BloodWireTool after SetToolByTypeName.
    fn current_on_select(&mut self) {
        if self.entity_tool_chosen() {
            // LightningTool.OnSelect is empty.
        } else if let Some(wood) = self.pin_tool {
            self.pin_on_select(wood);
        } else if let Some(power) = self.power {
            self.power_on_select(power);
        } else if self.wire_tool.chosen {
            self.wire_on_select();
        } else if self.link_tool.chosen {
            self.link_on_select();
        } else if self.belt_tool.chosen {
            self.belt_on_select();
        } else {
            self.drag_on_select();
        }
    }

    fn current_on_hold(&mut self) {
        if self.power.is_some() || self.pin_tool.is_some() || self.entity_tool_chosen() {
            // PinTool.OnHold: empty (as AOEPowerTool.OnHold and LightningTool.OnHold).
        } else if self.wire_tool.chosen {
            self.wire_on_hold();
        } else if self.link_tool.chosen {
            self.link_on_hold();
        } else if self.belt_tool.chosen {
            self.belt_on_hold();
        } else {
            self.drag_on_hold();
        }
    }

    fn current_on_deselect(&mut self) {
        if self.pin_tool.is_some() || self.entity_tool_chosen() {
            // PinTool.OnDeselect and LightningTool.OnDeselect are empty.
        } else if let Some(power) = self.power {
            self.power_on_deselect(power);
        } else if self.wire_tool.chosen {
            self.wire_on_deselect();
        } else if self.link_tool.chosen {
            self.link_on_deselect();
        } else if self.belt_tool.chosen {
            self.belt_on_deselect();
        } else {
            self.drag_on_deselect();
        }
    }

    /// The LightningTool is the current tool.
    pub(super) fn lightning_chosen(&self) -> bool {
        self.lightning_tool.as_ref().is_some_and(|l| l.chosen)
    }

    /// The LightningTool or PyroPowerTool is the current tool: OnSelect, OnHold, OnDeselect and OnFixedHold are empty.
    pub(super) fn entity_tool_chosen(&self) -> bool {
        self.lightning_chosen() || self.pyro_chosen()
    }

    /// ToolControllerBehaviour.SetToolByTypeName (-> SetTool): the old tool is unchosen and deselected, then the new one is
    /// found or added to the ToolController and chosen. The DragTool, wire tools and AOE powers are the tools rewritten.
    pub fn set_tool_by_type_name(&mut self, name: &str) {
        // ToolControllerBehaviour.SetImplementationArray: toolImplementations is every ToolBehaviour class of the game (no modded types), which
        // every ToolLibrary entry's Type names; SetTool's lookup in it is `ty` here.
        // The first Tools entry of that Type, else the first Powers entry: SetTool(index, tab). No match changes nothing.
        let (tools, powers) = (self.tool_collection(0), self.tool_collection(1));
        if let Some(i) = tools.iter().position(|t| t.ty == name) {
            self.set_tool(i as i32, 0);
        } else if let Some(i) = powers.iter().position(|t| t.ty == name) {
            self.set_tool(i as i32, 1);
        }
    }

    /// ToolControllerBehaviour.SetTool(index, tab): an index outside the collection logs an error and stops.
    pub fn set_tool(&mut self, index: i32, tab: i32) {
        let collection = self.tool_collection(tab);
        let entry = usize::try_from(index).ok().and_then(|i| collection.get(i)).map(|t| (t.ty.clone(), t.name.clone()));
        let in_bounds = entry.is_some();
        let (name, entry_name) = entry.unwrap_or_default();
        let name = name.as_str();
        let wire = super::blood_wire::WireKind::of_tool(name);
        let power = super::power::PowerKind::of_tool(name);
        let pin = match name {
            "PinTool" => Some(false),
            "WoodPinTool" => Some(true),
            _ => None,
        };
        let lightning = name == "LightningTool";
        let pyro = name == "PyroPowerTool";
        let phase_link = name == "PhaseLinkTool";
        let belt = name == "SerpentineBeltTool";
        if in_bounds && name != "DragTool" && name != "GentleDragTool" && !lightning && !pyro && !phase_link && !belt && wire.is_none() && power.is_none() && pin.is_none() {
            unimplemented!("mod-only: ToolControllerBehaviour.SetToolByTypeName({name})");
        }
        if self.lightning_chosen() {
            self.lightning_tool_unchoose();
        } else if self.pyro_chosen() {
            self.pyro_tool_unchoose();
        } else if self.wire_tool.chosen || self.link_tool.chosen || self.belt_tool.chosen || self.power.is_some() || self.pin_tool.is_some() {
            // SpringJointWireTool.OnToolUnchosen: empty.
            // PinTool.OnToolUnchosen: empty (as DistanceWireTool, LinkDeviceTool and AOEPowerTool.OnToolUnchosen).
        } else {
            // DragTool.OnToolUnchosen
            let t = self.tool.as_mut().unwrap();
            t.isActive = false;
            t.shouldSendContinuousUseMessage = false;
            self.drag_end_pause_moves();
        }
        self.tool.as_mut().unwrap().isCurrentlyHolding = false;
        self.current_on_deselect();
        self.refresh_outlines();
        self.tool.as_mut().unwrap().ActiveSingleSelected = None;
        if !in_bounds {
            // Debug.LogError and return: the old tool stays CurrentTool, unchosen and deselected.
            self.log_message_received(&format!("Attempt to set out of bounds tool: {index}"), super::menu::LogType::Error);
            return;
        }
        self.wire_tool.chosen = false;
        self.link_tool.chosen = false;
        self.belt_tool.chosen = false;
        self.power = None;
        self.pin_tool = None;
        let t = self.tool.as_mut().unwrap();
        t.chosen = entry_name;
        t.gentle = name == "GentleDragTool";
        if t.gentle && !t.gentle_added {
            t.gentle_added = true;
            self.allocate_instance_id();
        }
        if lightning {
            self.lightning_tool_choose();
        } else if pyro {
            self.pyro_tool_choose();
        } else if phase_link {
            self.link_tool_choose();
        } else if belt {
            self.belt_tool_choose();
        } else if let Some(wood) = pin {
            self.pin_tool_choose(wood);
        } else if let Some(kind) = wire {
            self.wire_tool_choose(kind);
        } else if let Some(kind) = power {
            self.power_choose(kind);
        } else {
            // DragTool.OnToolChosen: isActive = true.
            self.tool.as_mut().unwrap().isActive = true;
        }
    }

    // ToolChangeEventArgs..ctor: Index = index; ToolChanged's only listener, ToolStatusView.ToolChanged (view.rs), never reads it.
    /// DragTool.GetDraggerMassFunction, GentleDragTool.GetDraggerMassFunction: ActiveSingleSelected.rigidbody.mass * 10f; GentleDragTool's override is 0.01f.
    pub(crate) fn drag_tool_get_dragger_mass(&self, sel: usize) -> f32 {
        if self.tool.as_ref().unwrap().gentle {
            GENTLE_DRAGGER_MASS
        } else {
            self.physicals[sel].rigidbody.mass(&self.world) * 10.0
        }
    }

    /// DragTool.DragStrengthFunction, GentleDragTool.DragStrengthFunction: Rigidbody.mass; GentleDragTool's override is 0.008f.
    pub(crate) fn drag_tool_drag_strength(&self, body: BodyId) -> f32 {
        if self.tool.as_ref().unwrap().gentle {
            GENTLE_DRAG_STRENGTH
        } else {
            self.world.body(body).rigidbody.mass
        }
    }

    /// Rigidbody2D.mass = v on the tool (Rigidbody2D::SetMass 180c23790): clamped, then b2Body::ResetMassData.
    fn tool_set_mass(&mut self, v: f32) {
        let body = self.tool.as_ref().unwrap().body;
        self.body_set_mass(body, v);
    }

    // DragTool.IsUndraggableInSelection: private, with no caller.
    // DragTool.GetHeldObject: no caller (mods only).
    // DragTool.OnDestroy: the DragTool lives on the ToolController, which is never destroyed in a session.
    /// DragTool.OnSelect (never paused; nothing in the scene is Undraggable).
    pub(crate) fn drag_on_select(&mut self) {
        self.drag_on_select_base();
        // GentleDragTool.OnSelect: base.OnSelect, then Rigidbody.drag = angularDrag = 15 (also after an early return).
        let t = self.tool.as_ref().unwrap();
        if t.gentle {
            let b = self.world.body_mut(t.body);
            b.linear_damping = 15.0;
            b.angular_damping = 15.0;
        }
    }

    /// DragTool.OnSelect (nothing in the scene is Undraggable), paused case included.
    fn drag_on_select_base(&mut self) {
        let Some(sel) = self.tool.as_ref().unwrap().ActiveSingleSelected else { return };
        let mouse = self.MousePosition;
        let t = self.tool.as_mut().unwrap();
        t.controlRotation = false;
        t.position = mouse;
        let body = t.body;
        // Rigidbody2D::SetPosition (180c23cb0): b2Body::SetTransform at the body's angle, then awake.
        let clamp = |v: f32| if -1_000_000.0 <= v && v <= 1_000_000.0 { v } else if -1_000_000.0 <= v { 1_000_000.0 } else { -1_000_000.0 };
        let angle = self.world.body(body).sweep.a;
        self.world.set_transform(body, V::new(clamp(mouse[0]), clamp(mouse[1])), angle);
        self.world.wake_body(body);
        // Rigidbody2D::SetVelocity with zero: no wake.
        self.world.body_mut(body).linear_velocity = V::ZERO;
        let local = self.inverse_transform_point(sel, mouse);
        let mass = self.drag_tool_get_dragger_mass(sel);
        self.tool_set_mass(mass);
        let rotation = dump::euler_z(self.physicals[sel].transform.rotation);
        // DragTool.get_IsSnapping: InputSystem.Held("snap").
        let snapping = self.input.held(crate::il!(str "DragTool::get_IsSnapping" 0));
        let prev = self.mouse_pos_with_snap();
        let t = self.tool.as_mut().unwrap();
        t.offset = [local[0], local[1]];
        t.currentRotation = rotation;
        t.initialSnappingFinished = !snapping;
        // Rigidbody2D::SetDrag / SetAngularDrag: the b2 damping, no wake.
        let b = self.world.body_mut(body);
        b.linear_damping = 150.0;
        b.angular_damping = 150.0;
        t.selectedRigidbody = Some(sel);
        t.prevMousePos = prev;
        if !self.paused {
            self.destroy_drag_joint();
            self.add_drag_joint(sel);
            return;
        }
        let selected = self.tool.as_ref().unwrap().rawSelection.clone();
        self.tool.as_mut().unwrap().objectsToMoveInPauseMode.clear();
        for p in selected {
            // Nothing in the scene is Undraggable. 1.28.45: PauseDragOverride must be set.
            if !self.physicals[p].destroyed && (!crate::assets::il::is_1_28_45() || self.physicals[p].PauseDragOverride) && !self.drag_is_any_parent_selected(p) {
                self.tool.as_mut().unwrap().objectsToMoveInPauseMode.push(p);
                self.drag_broadcast_drag_message(p, false);
            }
        }
    }

    /// Transform.IsChildOf: `q`'s Transform is `p`'s or hangs below it.
    pub(super) fn physical_is_child_of(&self, q: usize, p: usize) -> bool {
        if q == p {
            return true;
        }
        let mut n = self.physicals[q].parent;
        while let Some(i) = n {
            if self.node_bodies.get(&i) == Some(&p) {
                return true;
            }
            n = self.transform_nodes[i].parent;
        }
        false
    }

    /// item.BroadcastMessage("OnPlayerDragStart" / "OnPlayerDragEnd" (`end`), DontRequireReceiver) on the active object and
    /// everything below it. The receivers are Messages.IOnDrag's: FixedJointWireBehaviour.OnPlayerDragStart and
    /// OnPlayerDragEnd (both hasMovedInPauseMode = true), and CableOtherEndBehaviour (OnPlayerDragStart empty;
    /// OnPlayerDragEnd: when Connected lives and its transform.root is not its own, Connected.SendMessage(
    /// "OnPlayerDragStart"), which reaches the FixedJointWireBehaviours on the wire's active object).
    fn drag_broadcast_drag_message(&mut self, item: usize, end: bool) {
        let mut connected = Vec::new();
        for w in 0..self.blood_wires.len() {
            if !self.blood_wires[w].kind.is_fixed() || self.destroyed_components.contains(&self.blood_wires[w].id) {
                continue;
            }
            let q = self.blood_wires[w].physical;
            // FixedJointWireBehaviour.OnPlayerDragStart: hasMovedInPauseMode = true.
            // FixedJointWireBehaviour.OnPlayerDragEnd: hasMovedInPauseMode = true.
            // CableOtherEndBehaviour.OnPlayerDragStart: empty.
            if self.physicals[q].active && self.physical_is_child_of(q, item) {
                self.blood_wires[w].hasMovedInPauseMode = true;
            }
            // CableOtherEndBehaviour.OnPlayerDragEnd: when Connected lives and its transform.root is not its own, Connected.SendMessage("OnPlayerDragStart").
            if let Some(o) = self.blood_wires[w].other.filter(|&o| end && self.blood_wires[w].otherEnd && !self.physicals[o].destroyed) {
                if self.physicals[o].active && self.physical_is_child_of(o, item) && self.transform_root_id(q) != self.transform_root_id(o) {
                    connected.push(q);
                }
            }
        }
        for q in connected {
            if self.physicals[q].active {
                for w in 0..self.blood_wires.len() {
                    if self.blood_wires[w].kind.is_fixed() && self.blood_wires[w].physical == q && !self.destroyed_components.contains(&self.blood_wires[w].id) {
                        self.blood_wires[w].hasMovedInPauseMode = true;
                    }
                }
            }
        }
        // 1.28.45: CrossbowBehaviour.OnPlayerDragStart / OnPlayerDragEnd.
        self.crossbow_on_drag(item, end);
    }

    /// DragTool.OnDeselect's and OnToolUnchosen's loop: OnPlayerDragEnd (`drag_broadcast_drag_message`) to each object moved in pause mode, then the set is cleared.
    pub(crate) fn drag_end_pause_moves(&mut self) {
        for p in std::mem::take(&mut self.tool.as_mut().unwrap().objectsToMoveInPauseMode) {
            if !self.physicals[p].destroyed {
                self.drag_broadcast_drag_message(p, true);
            }
        }
    }

    /// Object.Destroy(dragJoint): DestroyObjectDelayed's DelayedCallManager entry at min(fixedTimeAsDouble, time).
    fn destroy_drag_joint(&mut self) {
        let at = self.time.fixedTimeAsDouble.min(self.time.timeAsDouble);
        if let Some(j) = self.tool.as_mut().unwrap().dragJoint.as_mut() {
            j.destroy_at.get_or_insert(at);
            // DestroyObjectFromScripting (1808cd940) disables the Behaviour at once.
            if let Some(id) = j.joint.take() {
                self.world.destroy_joint(id);
            }
        }
    }

    /// AddComponent<HingeJoint2D>() and the setters OnSelect runs on it, ending with connectedBody = the selected
    /// limb. HingeJoint2D::Reset gives anchor (0,0), autoConfigureConnectedAnchor, motor 0°/s at 10000, useMotor
    /// and useLimits off, limits 0..359°, infinite break force and torque. AddToManager creates the joint on the
    /// scene's ground body; useMotor = false finds it already off; autoConfigureConnectedAnchor and connectedAnchor
    /// each ReCreate it there and connectedBody builds it again (Cleanup(true): a fresh reference angle). The
    /// ground joints leave behind the tool woken by b2World::DestroyJoint. The last HingeJoint2D::Create:
    /// localAnchorA = anchor × lossyScale, localAnchorB = connectedAnchor × the limb's lossyScale, referenceAngle
    /// = body B angle − body A angle; FinalizeCreateJoint wakes both; infinite breaking stays off the break list.
    fn add_drag_joint(&mut self, sel: usize) {
        let id = self.allocate_instance_id();
        let mut joint = DragJoint { id, joint: None, motorSpeed: 0.0, maxMotorTorque: 10000.0, useMotor: false, destroy_at: None };
        let t = self.tool.as_ref().unwrap();
        let (a, offset) = (t.body, t.offset);
        let ph = &self.physicals[sel];
        // HingeJoint2D::Create returns early on an inactive connected object.
        if ph.active && ph.rigidbody.detached.is_none() {
            let b = ph.rigidbody.body;
            let s = self.lossy_scale(sel);
            let def = RevoluteDef {
                local_anchor_a: V::ZERO,
                local_anchor_b: V::new(s[0] * offset[0], s[1] * offset[1]),
                reference_angle: self.world.body(b).sweep.a - self.world.body(a).sweep.a,
                enable_limit: false,
                lower_angle: 0.0,
                upper_angle: 359.0 * DEG2RAD,
                enable_motor: false,
                motor_speed: 0.0,
                max_motor_torque: 10000.0,
            };
            self.world.wake_body(a);
            let id = self.world.create_joint(def, a, b, false);
            self.world.wake_joined(a);
            self.world.wake_joined(b);
            joint.joint = Some(id);
        }
        let t = self.tool.as_mut().unwrap();
        if let Some(old) = t.dragJoint.replace(joint) {
            if old.destroy_at.is_some() {
                t.destroying.push(old);
            }
        }
    }

    /// Joint2D::ReCreate of the DragTool's HingeJoint2D `id` (the held object's Transform changed): Cleanup(false) keeps the
    /// reference angle and the body angles (unchanged within the call), Create builds it again with connectedAnchor x the
    /// held object's lossy scale as it is now (a flipped object has the x sign changed) and the current motor. False when
    /// `id` is not the drag joint.
    pub(super) fn drag_joint_recreate(&mut self, id: JointId) -> bool {
        // No ToolController (an fn-check world): no DragTool, so no drag joint.
        let Some(t) = self.tool.as_ref() else { return false };
        let Some(j) = t.dragJoint.as_ref().filter(|j| j.joint == Some(id)) else { return false };
        let (motor_speed, max_motor_torque, use_motor, offset) = (j.motorSpeed, j.maxMotorTorque, j.useMotor, t.offset);
        let joint = self.world.joints[id].as_ref().expect("the drag joint without its b2 joint");
        let (a, b, reference_angle) = (joint.body_a, joint.body_b, joint.revolute().def.reference_angle);
        self.tool.as_mut().unwrap().dragJoint.as_mut().unwrap().joint = None;
        self.world.destroy_joint(id);
        let sel = self.physicals.iter().position(|p| !p.destroyed && p.rigidbody.body == b).expect("the held object");
        // HingeJoint2D::Create returns early on an inactive connected object.
        if !self.physicals[sel].active || self.physicals[sel].rigidbody.detached.is_some() {
            return true;
        }
        let s = self.lossy_scale(sel);
        let def = RevoluteDef {
            local_anchor_a: V::ZERO,
            local_anchor_b: V::new(s[0] * offset[0], s[1] * offset[1]),
            reference_angle,
            enable_limit: false,
            lower_angle: 0.0,
            upper_angle: 359.0 * DEG2RAD,
            enable_motor: use_motor,
            motor_speed: motor_speed * DEG2RAD,
            max_motor_torque,
        };
        let new = self.world.create_joint(def, a, b, false);
        self.world.wake_joined(a);
        self.world.wake_joined(b);
        self.tool.as_mut().unwrap().dragJoint.as_mut().unwrap().joint = Some(new);
        true
    }

    /// DragTool.OnHold (no UI lock).
    pub(crate) fn drag_on_hold(&mut self) {
        let t = self.tool.as_mut().unwrap();
        t.shouldSendContinuousUseMessage = false;
        let (Some(sel), Some(_)) = (t.ActiveSingleSelected, t.selectedRigidbody) else { return };
        // (bool)ActiveSingleSelected: a destroyed object reads null.
        if self.physicals[sel].destroyed {
            return;
        }
        if self.input.down(crate::il!(str "DragTool::OnHold" 0)) {
            self.tool.as_mut().unwrap().initialSnappingFinished = false;
        }
        let flag = !self.tool.as_ref().unwrap().initialSnappingFinished;
        let snapped = self.mouse_pos_with_snap();
        if flag {
            let pos = self.physicals[sel].transform.position;
            let d = sub3(self.snap_to_grid(pos), pos);
            let t = self.tool.as_mut().unwrap();
            t.initialTranslationSnapOffset = d;
            t.snappedMouseDelta = d;
            t.initialSnappingFinished = true;
        } else {
            let t = self.tool.as_mut().unwrap();
            t.snappedMouseDelta = sub3(snapped, t.prevMousePos);
        }
        self.tool.as_mut().unwrap().prevMousePos = snapped;
        if self.input.down(crate::il!(str "DragTool::OnHold" 1)) || self.input.down(crate::il!(str "DragTool::OnHold" 2)) {
            self.tool.as_mut().unwrap().rotationSpeed = 1.0;
        }
        if self.input.held(crate::il!(str "DragTool::OnHold" 1)) || self.input.held(crate::il!(str "DragTool::OnHold" 2)) {
            // Mono: in double, rounded at the store.
            let t = self.tool.as_mut().unwrap();
            t.rotationSpeed = (t.rotationSpeed as f64 + self.time.unscaledDeltaTime as f64 * 15.0) as f32;
        }
        if self.paused {
            self.drag_paused_move(sel, flag);
        } else {
            self.move_to_mouse();
        }
        if self.ui_lock() {
            return;
        }
        if self.input.down("activateDirect") {
            for p in self.tool.as_ref().unwrap().rawSelection.clone() {
                if !self.physicals[p].destroyed {
                    self.activate_direct(p);
                }
            }
        }
        // The UseContinuous broadcast is ContinuousActivationBehaviour's (continuous.rs).
        self.tool.as_mut().unwrap().shouldSendContinuousUseMessage = self.input.held("activateDirect");
        if self.input.held(crate::il!(str "DragTool::OnHold" 11)) {
            self.user_delete_selection();
        }
    }

    /// DragTool.OnHold's paused branch: the objects moved in pause mode follow the mouse (or the snapped delta) and turn about it.
    fn drag_paused_move(&mut self, sel: usize, flag: bool) {
        let snapping = self.input.held(crate::il!(str "DragTool::OnHold" 0));
        let mut num = 0.0f32;
        if self.input.held(crate::il!(str "DragTool::OnHold" 1)) {
            num -= 250.0;
        }
        if self.input.held(crate::il!(str "DragTool::OnHold" 2)) {
            num += 250.0;
        }
        if self.input.held(crate::il!(str "DragTool::OnHold" 7)) {
            num *= 4.0;
        }
        num *= self.time.unscaledDeltaTime;
        num *= self.tool.as_ref().unwrap().rotationSpeed * 0.1;
        let num2 = if snapping { self.prefs.AngleSnap as f32 } else { 0.0 };
        // Utils.Snap(float, float)
        let snap = mathf::snap;
        let current = self.tool.as_ref().unwrap().currentRotation;
        // Mathf.LerpAngle(current, current + num, 1)
        let mut delta = mathf::repeat(current + num - current, 360.0);
        if delta > 180.0 {
            delta -= 360.0;
        }
        let mut num3 = (d(current) + d(delta) * mathf::clamp01(1.0)).f();
        let mut angle = mathf::delta_angle(snap(current, num2), snap(num3, num2));
        if flag {
            let cur = dump::euler_z(self.physicals[sel].transform.rotation);
            self.tool.as_mut().unwrap().currentRotation = cur;
            num3 = snap(cur, num2);
            angle = mathf::delta_angle(cur, num3);
        }
        self.tool.as_mut().unwrap().currentRotation = num3;
        let pos = self.physicals[sel].transform.position;
        let vector = if snapping { sub3(self.snap_to_grid(pos), pos) } else { [0.0; 3] };
        let mouse = self.MousePosition;
        let step = if snapping {
            let m = self.tool.as_ref().unwrap().snappedMouseDelta;
            [m[0] + vector[0], m[1] + vector[1], m[2] + vector[2]]
        } else {
            [self.MouseDelta[0], self.MouseDelta[1], 0.0]
        };
        for p in self.tool.as_ref().unwrap().objectsToMoveInPauseMode.clone() {
            if self.physicals[p].destroyed {
                continue;
            }
            let at = self.physicals[p].transform.position;
            self.physical_set_position(p, [at[0] + step[0], at[1] + step[1], at[2] + step[2]]);
            self.physical_rotate_around_z(p, mouse, angle);
        }
    }

    /// Transform.RotateAround(point, Vector3.forward, angle): the position turned about the point, the rotation turned by the same
    /// quaternion in world space.
    fn physical_rotate_around_z(&mut self, p: usize, point: [f32; 3], angle: f32) {
        let half = angle * mathf::DEG2RAD * 0.5;
        let q = [0.0, 0.0, half.sin(), half.cos()];
        let at = self.physicals[p].transform.position;
        let v = [at[0] - point[0], at[1] - point[1], at[2] - point[2]];
        // q * v for a rotation about z
        let (s, c) = (q[2] * 2.0, q[3] * q[3] - q[2] * q[2]);
        let r = [c * v[0] - s * q[3] * v[1], s * q[3] * v[0] + c * v[1], v[2]];
        self.physical_set_position(p, [r[0] + point[0], r[1] + point[1], r[2] + point[2]]);
        let rot = self.physicals[p].transform.rotation;
        let m = [
            q[3] * rot[0] + q[0] * rot[3] + q[1] * rot[2] - q[2] * rot[1],
            q[3] * rot[1] + q[1] * rot[3] + q[2] * rot[0] - q[0] * rot[2],
            q[3] * rot[2] + q[2] * rot[3] + q[0] * rot[1] - q[1] * rot[0],
            q[3] * rot[3] - q[0] * rot[0] - q[1] * rot[1] - q[2] * rot[2],
        ];
        self.physical_set_rotation(p, m);
    }

    /// SelectionController.UserDeleteSelection: each live, Deletable object of the selection gets OnUserDelete on its
    /// root GameObject, then Object.Destroy(root); the names (once each) are announced.
    pub fn user_delete_selection(&mut self) {
        let selected = self.tool.as_ref().unwrap().rawSelection.clone();
        if selected.is_empty() {
            return;
        }
        // HashSet<string> enumerates in insertion order while nothing is removed.
        let mut names: Vec<String> = Vec::new();
        let mut checked: Vec<String> = Vec::new();
        for p in selected {
            if self.physicals[p].destroyed || !self.physicals[p].Deletable {
                continue;
            }
            let root = self.root_id(p);
            self.send_on_user_delete(root);
            self.destroy_object(root);
            let name = self.object_name(p, root);
            if !names.contains(&name) {
                names.push(name);
            }
        }
        // The check: the live, Deletable selected objects' root names, once each (the game's Object.Destroy waits for
        // the frame's end, so every one is still alive while the loop runs).
        checked.extend(names.iter().cloned());
        crate::ui::uifn("SelectionController.UserDeleteSelection", || format!("{} {}", checked.len(), checked.join("|")));
        for name in names {
            self.notify(format!("<b>{}</b> removed", name.to_uppercase()));
        }
    }

    /// transform.root.gameObject.name of a physical object whose root Transform is `root`.
    pub(super) fn object_name(&mut self, p: usize, root: i32) -> String {
        if let Some(n) = self.object_names.get(&root) {
            return n.clone();
        }
        if self.physicals[p].transform_id == root {
            return self.physicals[p].name.clone();
        }
        self.with_component_instance(root, |_, inst, o| inst.prefab.objects[o].name.clone())
    }

    /// SendMessage("OnUserDelete", DontRequireReceiver) on a GameObject (by its Transform id): its own components only.
    /// Messages.IOnUserDelete is declared by ActOnUserDelete (on no catalog item) and by the Hover wires, pins and
    /// links, which the rewrite refuses at spawn.
    fn send_on_user_delete(&mut self, root: i32) {
        let receivers = self.with_component_instance(root, |_, inst, o| {
            let active = inst.objects[o].active;
            inst.prefab.objects[o].components.iter().filter(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "ActOnUserDelete")).count() * active as usize
        });
        if receivers > 0 {
            unimplemented!("mod-only: ActOnUserDelete.OnUserDelete");
        }
    }

    /// DragTool.Update (no dialog, UI block, trigger editor or pause menu is ever open): copy, paste and flip.
    pub fn drag_tool_update(&mut self) {
        let Some(t) = self.tool.as_ref() else { return };
        if !t.isActive {
            return;
        }
        // ClipboardControllerBehaviour.ObjectStateInClipboard: the auto-property is `clipboard`, which copy fills.
        // (The outline SpawnableOutlineBehaviour draws on paste is presentation.)
        if self.input.down(crate::il!(str "DragTool::Update" 0)) && !t.rawSelection.is_empty() {
            let (selected, center) = (t.rawSelection.clone(), self.MousePosition);
            self.clipboard_copy(&selected, center);
        }
        if self.input.up(crate::il!(str "DragTool::Update" 2)) && self.clipboard.is_some() {
            let at = self.MousePosition;
            self.clipboard_paste(at);
        }
        if self.input.down(crate::il!(str "DragTool::Update" 6)) {
            self.flip_selection();
        }
    }

    /// SelectionController.FlipSelection. The selected objects are spawned items: none carries the World or Map tag or an
    /// Undraggable, so those skips never apply. Every selected object's root is collected, with the roots of the stabbers
    /// lodged in it. A single root with a PhysicalBehaviour is mirrored in place: OnPreFlip, localScale.x negated, OnFlip.
    pub(crate) fn flip_selection(&mut self) {
        let selected = self.tool.as_ref().unwrap().rawSelection.clone();
        if selected.is_empty() {
            return;
        }
        let mut roots: Vec<i32> = Vec::new();
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in selected {
            if self.physicals[p].destroyed {
                continue;
            }
            let root = self.root_id(p);
            if roots.contains(&root) || !self.physicals[p].Selectable || !self.physicals[p].Deletable {
                continue;
            }
            for q in self.flip_hierarchy_physicals(root) {
                // 1.28.45 also skips a disintegrated one.
                if self.physicals[q].has_optout || (crate::assets::il::is_1_28_45() && self.physicals[q].isDisintegrated) {
                    continue;
                }
                if self.physicals[q].sprite_bounds.is_some() {
                    // Vector3.Min / Max of the SpriteRenderer's bounds.min / bounds.max.
                    let (c, e) = self.sprite_world_bounds(q);
                    for i in 0..3 {
                        let (mn, mx) = (c[i] - e[i], c[i] + e[i]);
                        lo[i] = if lo[i] < mn { lo[i] } else { mn };
                        hi[i] = if mx < hi[i] { hi[i] } else { mx };
                    }
                }
                for pen in self.physicals[q].victimPenetrations.clone() {
                    let stabber = self.penetration_arena[pen].Stabber;
                    if self.penetration_arena[pen].Active && !self.physicals[stabber].destroyed {
                        let r = self.root_id(stabber);
                        if !roots.contains(&r) {
                            roots.push(r);
                        }
                    }
                }
            }
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        if roots.is_empty() {
            return;
        }
        let single = if roots.len() == 1 { self.physicals.iter().position(|ph| ph.transform_id == roots[0] && !ph.destroyed) } else { None };
        let Some(p) = single else {
            let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
            self.flip_roots_mirrored(&roots, center);
            return;
        };
        let root = roots[0];
        if self.physicals[p].parent.is_some() {
            unimplemented!("mod-only: SelectionController.FlipSelection: a root PhysicalBehaviour below another Transform");
        }
        let position = self.physicals[p].transform.position;
        self.flip_broadcast(root, true, position);
        let s = self.physicals[p].transform.localScale;
        self.physical_set_local_scale(p, [0.0 - s[0], s[1], s[2]]);
        // The check: the first selected object's root's localScale.x (this root is the one physical object here).
        if self.tool.as_ref().unwrap().rawSelection.first() == Some(&p) {
            let x = self.physicals[p].transform.localScale[0];
            crate::ui::uifn("SelectionController.FlipSelection", || super::menu::cs_float(x));
        }
        // PersonBehaviour.localScale is a copy of the root Transform's scale.
        for i in 0..self.people.len() {
            if let Some(&l) = self.people[i].Limbs.first() {
                if self.root_id(self.limbs[l].PhysicalBehaviour) == root {
                    self.people[i].localScale = [0.0 - s[0], s[1], s[2]];
                }
            }
        }
        self.flip_broadcast(root, false, position);
    }

    /// SelectionController.FlipSelection, the several-roots / no-PhysicalBehaviour branch: OnPreFlip(center) to each root,
    /// each root SetParent (worldPositionStays) under a new GameObject at `center`, that GameObject's localScale set to
    /// (-1, 1, 1), each root SetParent(null, worldPositionStays), OnFlip(center) to each root. The GameObject has no
    /// rotation and no other effect, so a root's pose goes through it by Transform math: its local pose under the
    /// unscaled parent, then its world pose and lossy scale under the mirrored one.
    fn flip_roots_mirrored(&mut self, roots: &[i32], center: [f32; 3]) {
        use crate::unity::transform::{global_position, global_rotation, lossy_scale, set_world, Trs};
        for &root in roots {
            self.flip_broadcast(root, true, center);
        }
        let mut parent = Trs { t: [center[0], center[1], center[2], 0.0], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.0, 0.0] };
        for &root in roots {
            let node = self.transform_nodes.iter().position(|n| n.id == root);
            let physical = self.physicals.iter().position(|ph| ph.transform_id == root && !ph.destroyed);
            let ((position, rotation), scale) = match (physical, node) {
                (Some(p), _) => {
                    if self.physicals[p].parent.is_some() {
                        unimplemented!("mod-only: SelectionController.FlipSelection: a root PhysicalBehaviour below another Transform");
                    }
                    ((self.physicals[p].transform.position, self.physicals[p].transform.rotation), self.lossy_scale(p))
                }
                (None, Some(n)) => {
                    if self.transform_nodes[n].parent.is_some() {
                        unimplemented!("mod-only: SelectionController.FlipSelection: a root Transform below another Transform");
                    }
                    let s = self.transform_nodes[n].trs.s;
                    (self.node_world(n), [s[0], s[1], s[2]])
                }
                (None, None) => unimplemented!("mod-only: SelectionController.FlipSelection: a root that is neither a body nor a Transform node"),
            };
            parent.s = [1.0, 1.0, 1.0, 0.0];
            let (local, _) = set_world(&[parent], position, rotation);
            let local = Trs { t: [local.0[0], local.0[1], local.0[2], 0.0], q: local.1, s: [scale[0], scale[1], scale[2], 0.0] };
            parent.s = [-1.0, 1.0, 1.0, 0.0];
            let (v, q) = (global_position(&parent, local.t), global_rotation(&parent, local.q));
            let lossy = lossy_scale(&[local, parent], q);
            self.flip_root_apply(root, physical.ok_or_else(|| node.unwrap()), [v[0], v[1], v[2]], q, lossy);
        }
        for &root in roots {
            self.flip_broadcast(root, false, center);
        }
    }

    /// Transform.SetParent(null) of root `root` to a world pose and lossy scale: the clone's Transforms are refreshed from
    /// the live ones first (as for Transform.localScale), then the root's become the new ones.
    fn flip_root_apply(&mut self, root: i32, owner: Result<usize, usize>, position: [f32; 3], rotation: [f32; 4], scale: [f32; 3]) {
        use crate::unity::transform::set_world;
        self.with_component_instance(root, |g, inst, o| {
            for x in 0..inst.objects.len() {
                let key = inst.objects[x].components[0];
                let Some(q) = g.physicals.iter().position(|ph| ph.transform_id == key && !ph.destroyed) else { continue };
                if g.physicals[q].parent.is_some() != inst.objects[x].parent.is_some() {
                    unimplemented!("mod-only: SelectionController.FlipSelection: a body re-parented under another clone's Transform");
                }
                let t = &g.physicals[q].transform;
                let (lp, lr) = t.local.unwrap_or_else(|| set_world(&g.parent_chain(q), t.position, t.rotation).0);
                let trs = &mut inst.objects[x].trs;
                trs.t = [lp[0], lp[1], lp[2], trs.t[3]];
                trs.q = lr;
                trs.s = [t.localScale[0], t.localScale[1], t.localScale[2], trs.s[3]];
            }
            inst.set_parent_null_at(o, (position, rotation, scale));
            g.reparented_rigidbodies(inst, o);
        });
        let local = set_world(&[], position, rotation).0;
        match owner {
            Ok(p) => {
                let pose = &mut self.physicals[p].transform;
                (pose.position, pose.rotation, pose.local, pose.localScale) = (position, rotation, Some(local), scale);
                if let Some(n) = self.body_node(p) {
                    self.transform_nodes[n].trs.s = [scale[0], scale[1], scale[2], 0.0];
                    self.refresh_below_node(n);
                }
            }
            Err(n) => {
                self.transform_nodes[n].trs = crate::unity::transform::Trs { t: [position[0], position[1], position[2], 0.0], q: rotation, s: [scale[0], scale[1], scale[2], 0.0] };
                self.refresh_below_node(n);
            }
        }
        // PersonBehaviour.localScale is a copy of the root Transform's scale.
        for i in 0..self.people.len() {
            if let Some(&l) = self.people[i].Limbs.first() {
                if self.root_id(self.limbs[l].PhysicalBehaviour) == root {
                    self.people[i].localScale = scale;
                }
            }
        }
        self.unsync_instance_of(root);
    }

    /// SelectionController.FlipSelection's `root.EnumerateChildrenAndSelf()`: the PhysicalBehaviours on a root GameObject and its descendants.
    pub(crate) fn flip_hierarchy_physicals(&mut self, root: i32) -> Vec<usize> {
        self.with_component_instance(root, |g, inst, o| {
            let rooted = inst.objects[o].parent.is_some();
            let order = g.enumerate_children_and_self(inst, o);
            let mut out = Vec::new();
            for (i, t) in order.into_iter().enumerate() {
                let Some(q) = g.physicals.iter().position(|ph| ph.transform_id == t && !ph.destroyed) else { continue };
                // Each body below the root (an attached clone's root too) has a parent Transform.
                if g.physicals[q].parent.is_some() != (i > 0 || rooted) {
                    unimplemented!("mod-only: SelectionController.FlipSelection: a body re-parented under another clone's Transform");
                }
                out.push(q);
            }
            out
        })
    }

    /// Utils.EnumerateChildrenAndSelf on clone object `o`'s Transform: the Transform ids of it and then each child's own
    /// enumeration, depth first. A Transform's children in sibling order: its prefab children (Instantiate), the selection
    /// Outline PhysicalBehaviour.CreateOutlineObject parented to it at Awake, then the clones parented under it later.
    pub(crate) fn enumerate_children_and_self(&mut self, inst: &super::spawn::Instance, o: usize) -> Vec<i32> {
        let t = inst.objects[o].components[0];
        let mut out = vec![t];
        for c in (o + 1..inst.objects.len()).filter(|&c| inst.objects[c].parent == Some(o)) {
            out.extend(self.enumerate_children_and_self(inst, c));
        }
        // The Outline GameObject's Transform takes the id after it.
        if let Some(ph) = self.physicals.iter().find(|ph| ph.transform_id == t && !ph.destroyed && ph.ids.selectionOutlineObject != 0) {
            out.push(ph.ids.selectionOutlineObject - 2);
        }
        for c in self.runtime_clone_transforms.get(&t).into_iter().flatten() {
            out.extend(c.iter().map(|x| x.0));
        }
        for c in self.attached_clones.get(&t).cloned().unwrap_or_default() {
            if !self.destroyed_components.contains(&c.id) {
                out.extend(self.with_component_instance(c.id, |g, clone, o| g.enumerate_children_and_self(clone, o)));
            }
        }
        out
    }

    /// Instantiate(prefab, parent) of a runtime clone under Transform `parent`: its Transforms (`nodes`: each object's Transform
    /// id and parent object, prefab order, depth first) join `runtime_clone_transforms`. Its siblings there are in creation order (ids
    /// go down); the same clone made again (a recording's clone loaded over the linked map's) replaces the old entry.
    pub(crate) fn add_runtime_clone(&mut self, parent: i32, nodes: &[(i32, Option<usize>)]) {
        let under = |j: usize, i: usize| std::iter::successors(Some(j), |&x| nodes[x].1).any(|x| x == i);
        let clone: Vec<(i32, usize)> = (0..nodes.len()).map(|i| (nodes[i].0, (i + 1..nodes.len()).find(|&j| !under(j, i)).unwrap_or(nodes.len()))).collect();
        let v = self.runtime_clone_transforms.entry(parent).or_default();
        v.retain(|c| c[0].0 != clone[0].0);
        v.push(clone);
        v.sort_by_key(|c| std::cmp::Reverse(c[0].0));
    }

    /// Utils.EnumerateChildrenAndSelf on a runtime clone's Transform `t` (`runtime_clone_transforms`): it and its descendants.
    pub(crate) fn runtime_subtree(&self, t: i32) -> Option<Vec<i32>> {
        self.runtime_clone_transforms.values().flatten().find_map(|c| {
            let i = c.iter().position(|x| x.0 == t)?;
            Some(c[i..c[i].1].iter().map(|x| x.0).collect())
        })
    }

    /// ResistorBehaviour.OnFlip: flipped follows the new sign of lossyScale.x (what Start did).
    pub(crate) fn resistor_behaviour_on_flip(&mut self, i: usize) {
        self.resistor_behaviour_start(i);
    }

    /// BroadcastMessage("OnPreFlip" or "OnFlip", center) on a root: its active objects (a hidden object hides its
    /// descendants), each one's components in order. OnPreFlip has only the wires as receivers.
    pub(crate) fn flip_broadcast(&mut self, root: i32, pre: bool, center: [f32; 3]) {
        // (class, component id, the PersonBehaviour component id on the same object)
        let components: Vec<(String, i32, Option<i32>)> = self.with_component_instance(root, |g, inst, o| {
            let mut out = Vec::new();
            let mut hidden: Vec<usize> = Vec::new();
            for x in std::iter::once(o).chain(inst.descendants(o)) {
                if !inst.objects[x].active || inst.objects[x].parent.is_some_and(|up| hidden.contains(&up)) {
                    hidden.push(x);
                    continue;
                }
                let script = |c: &crate::assets::db::Component| match c {
                    crate::assets::db::Component::Script { class, .. } => Some(class.clone()),
                    _ => None,
                };
                let person = inst.prefab.objects[x].components.iter().position(|(_, c)| script(c).as_deref() == Some("PersonBehaviour")).map(|k| inst.objects[x].components[k]);
                for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                    if let Some(class) = script(c) {
                        let id = inst.objects[x].components[k];
                        // A wire the game lays is a component added after the prefab's.
                        let laid = g.blood_wires.iter().any(|w| w.id == id);
                        let root_wire = g.root_wires.iter().any(|w| w.id == id);
                        out.push((if laid { "#Wire".to_string() } else if root_wire { "#RootWire".to_string() } else { class }, id, person));
                    }
                }
                // The wires the tools added to this object, in the order they were added.
                let key = inst.objects[x].components[0];
                for w in &g.blood_wires {
                    if g.physicals.get(w.physical).is_some_and(|ph| ph.transform_id == key) && !g.destroyed_components.contains(&w.id) && !out.iter().any(|(_, id, _)| *id == w.id) {
                        out.push(("#Wire".to_string(), w.id, None));
                    }
                }
            }
            out
        });
        for (class, id, person) in components {
            let wire = class.ends_with("WireBehaviour");
            if class == "#Wire" {
                let w = self.blood_wires.iter().position(|x| x.id == id).expect("the object's wire");
                if !self.destroyed_components.contains(&id) {
                    if pre { self.blood_wire_on_pre_flip(w) } else { self.blood_wire_on_flip(w, center) }
                }
                continue;
            }
            if class == "#RootWire" {
                if !self.destroyed_components.contains(&id) {
                    self.root_wire_flip(id, pre, center);
                }
                continue;
            }
            if pre && class == "CrossbowBehaviour" {
                if let Some(c) = self.v45.crossbows.iter().position(|x| x.id == id) {
                    self.crossbow_on_pre_flip(c);
                }
                continue;
            }
            if pre {
                // The wires with a flip message that the rewrite did not lay.
                if wire && !matches!(class.as_str(), "SliderJointWireBehaviour") {
                    unimplemented!("mod-only: {class}.OnPreFlip");
                }
                continue;
            }
            match class.as_str() {
                "PhysicalBehaviour" => {
                    let q = self.physicals.iter().position(|ph| ph.id == id).expect("the object's PhysicalBehaviour");
                    self.physical_on_flip(q);
                }
                "PersonBehaviour" => {
                    let i = self.people.iter().position(|x| x.id == id).expect("the object's PersonBehaviour");
                    self.person_on_flip(i);
                }
                "LimbBehaviour" => {
                    let l = self.limbs.iter().position(|x| x.id == id).expect("the object's LimbBehaviour");
                    self.limb_on_flip(l);
                }
                // BlasterBehaviour.OnFlip: Start().
                "BlasterBehaviour" => {
                    let i = self.blasters.iter().position(|x| x.id == id).expect("the object's BlasterBehaviour");
                    self.blaster_start(i);
                }
                "PistonBehaviour" => {
                    let i = self.pistons.iter().position(|x| x.id == id).expect("the object's PistonBehaviour");
                    self.piston_on_flip(i, center);
                }
                "WinchBehaviour" => {
                    let w = self.winches.iter().position(|x| x.id == id).expect("the object's WinchBehaviour");
                    self.winch_update_limits(w);
                }
                // EnsureScaling
                "MaintainScaleSignBehaviour" => self.maintain_scale_start(id),
                "HingeJointLimitAutofixBehaviour" => match person {
                    Some(pid) => {
                        let i = self.people.iter().position(|x| x.id == pid).expect("the object's PersonBehaviour");
                        self.hinge_joint_limit_autofix_on_flip(i);
                    }
                    None => self.hinge_joint_limit_autofix_item_on_flip(id),
                },
                "EBikeBehaviour" => {
                    // OnFlip calls Start.
                    let i = self.tr.e_bike_behaviour.iter().position(|x| x.this_id == id).expect("the object's EBikeBehaviour");
                    self.e_bike_behaviour_start(i);
                }
                "ResistorBehaviour" => {
                    let i = self.tr.resistor_behaviour.iter().position(|x| x.this_id == id).expect("the object's ResistorBehaviour");
                    self.resistor_behaviour_on_flip(i);
                }
                "SliderJointWireBehaviour" => {}
                _ if wire => unimplemented!("mod-only: {class}.OnFlip"),
                _ => {}
            }
        }
    }

    /// DragTool.MoveToMouse: Rigidbody2D.AddForce (180c1bf80, ForceMode2D.Force) wakes the body and adds the force.
    pub(crate) fn move_to_mouse(&mut self) {
        let target = if self.input.held("snap") { self.mouse_pos_with_snap() } else { self.MousePosition };
        let t = self.tool.as_ref().unwrap();
        let mut v = sub3(target, t.position);
        let body = t.body;
        let k = if t.gentle {
            // GentleDragTool.MoveToMouse: the offset clamped to 0.5 long.
            // Vector3.magnitude: (float)Math.Sqrt(x * x + y * y + z * z), in double.
            let magnitude = (d(v[0]) * v[0] + d(v[1]) * v[1] + d(v[2]) * v[2]).0.sqrt() as f32;
            if magnitude > 0.5 {
                v = [v[0] / magnitude, v[1] / magnitude, v[2] / magnitude];
                v = [v[0] * 0.5, v[1] * 0.5, v[2] * 0.5];
            }
            (d(self.time.deltaTime) * 60.0f32 * 0.0001f32 * 19500000.0f32 * self.drag_tool_drag_strength(body)).f()
        } else {
            // Mono evaluates the chained product in double and rounds once, as GentleDragTool's above.
            (d(self.time.deltaTime) * 60.0f32 * 1950.0f32 * self.drag_tool_drag_strength(body)).f()
        };
        self.world.body_mut(body).apply_force_to_center(V::new(k * v[0], k * v[1]));
    }

    /// DragTool.OnFixedHold (nothing is Undraggable).
    pub(crate) fn drag_on_fixed_hold(&mut self) {
        let t = self.tool.as_ref().unwrap();
        if self.paused || t.ActiveSingleSelected.is_none_or(|p| self.physicals[p].destroyed) || t.selectedRigidbody.is_none() {
            return;
        }
        let d = sub3(self.MousePosition, t.position);
        // Vector3.Distance
        if ((d[0] * d[0] + d[1] * d[1]) + d[2] * d[2]).sqrt() < 1.0 {
            // Rigidbody2D.velocity *= GetDragFunction(): SetVelocity wakes only for a non-zero velocity.
            // DragTool.GetDragFunction: 0.9.
            // GentleDragTool.GetDragFunction: 0.2.
            let k = drag_function(t.gentle);
            let b = self.world.body_mut(t.body);
            let v = b.linear_velocity;
            b.set_linear_velocity(V::new(v.x * k, v.y * k));
        }
        self.drag_rotate();
    }

    /// DragTool.Rotate
    pub(crate) fn drag_rotate(&mut self) {
        let (left, right, fast) = (self.input.held(crate::il!(str "DragTool::Rotate" 0)), self.input.held(crate::il!(str "DragTool::Rotate" 2)), self.input.held(crate::il!(str "DragTool::Rotate" 1)));
        let t = self.tool.as_mut().unwrap();
        let Some(j) = t.dragJoint.as_mut() else { return };
        let (mut speed, mut torque) = (j.motorSpeed, j.maxMotorTorque);
        if t.controlRotation {
            torque = 1000.0;
            speed = 0.0;
        }
        let mut use_motor = None;
        if left {
            let num = if fast { 1000.0 } else { 250.0 };
            t.controlRotation = true;
            use_motor = Some(true);
            speed = (num as f64 * (t.rotationSpeed as f64 * 0.1f32 as f64)) as f32;
        }
        if right {
            let num = if fast { -1000.0 } else { -250.0 };
            t.controlRotation = true;
            use_motor = Some(true);
            speed = (num as f64 * (t.rotationSpeed as f64 * 0.1f32 as f64)) as f32;
        }
        let control = t.controlRotation;
        if let Some(on) = use_motor {
            self.drag_set_use_motor(on);
        }
        self.drag_set_motor(speed, torque);
        if !control {
            self.drag_set_use_motor(false);
        }
    }

    /// HingeJoint2D.useMotor = on (180bfdda0): b2RevoluteJoint::EnableMotor, which wakes both ends on a change.
    fn drag_set_use_motor(&mut self, on: bool) {
        let j = self.tool.as_mut().unwrap().dragJoint.as_mut().unwrap();
        j.useMotor = on;
        if let Some(id) = j.joint {
            self.revolute_enable_motor(id, on);
        }
    }

    fn revolute_enable_motor(&mut self, id: JointId, on: bool) {
        if self.world.revolute(id).def.enable_motor == on {
            return;
        }
        self.wake_joint_bodies(id);
        self.world.revolute_mut(id).def.enable_motor = on;
    }

    fn wake_joint_bodies(&mut self, id: JointId) {
        let j = self.world.joints[id].as_ref().unwrap();
        let (a, b) = (j.body_a, j.body_b);
        self.world.wake_body(a);
        self.world.wake_body(b);
    }

    /// HingeJoint2D.motor = (speed, torque) (180bfdc90): clamped, useMotor forced on, then EnableMotor,
    /// SetMotorSpeed (radians) and SetMaxMotorTorque, each waking both ends only on a change.
    fn drag_set_motor(&mut self, speed: f32, torque: f32) {
        let speed = if -1_000_000.0 <= speed && speed <= 1_000_000.0 { speed } else if -1_000_000.0 <= speed { 1_000_000.0 } else { -1_000_000.0 };
        let torque = if 0.0 <= torque && torque <= 1_000_000.0 { torque } else if 0.0 <= torque { 1_000_000.0 } else { 0.0 };
        let j = self.tool.as_mut().unwrap().dragJoint.as_mut().unwrap();
        (j.motorSpeed, j.maxMotorTorque, j.useMotor) = (speed, torque, true);
        let Some(id) = j.joint else { return };
        self.revolute_enable_motor(id, true);
        let rad = speed * DEG2RAD;
        if self.world.revolute(id).def.motor_speed != rad {
            self.wake_joint_bodies(id);
            self.world.revolute_mut(id).def.motor_speed = rad;
        }
        if self.world.revolute(id).def.max_motor_torque != torque {
            self.wake_joint_bodies(id);
            self.world.revolute_mut(id).def.max_motor_torque = torque;
        }
    }

    /// DragTool.OnDeselect
    pub(crate) fn drag_on_deselect(&mut self) {
        let t = self.tool.as_mut().unwrap();
        t.shouldSendContinuousUseMessage = false;
        t.controlRotation = false;
        self.destroy_drag_joint();
        self.tool_set_mass(9500.0);
        self.drag_end_pause_moves();
    }

    /// The tool's queued HingeJoint2D destroys due by now (DelayedCallManager times).
    pub fn tool_due_destroys(&self, now: f64) -> Vec<f64> {
        let Some(t) = self.tool.as_ref() else { return Vec::new() };
        t.destroying.iter().chain(&t.dragJoint).filter_map(|j| j.destroy_at).filter(|&at| at <= now).collect()
    }

    /// The delayed destroy of the drag joint queued at `at`: Joint2D::Cleanup's b2World::DestroyJoint; the
    /// `dragJoint` field then reads null.
    pub fn tool_run_destroy(&mut self, at: f64) {
        let t = self.tool.as_mut().unwrap();
        let joint = if t.dragJoint.as_ref().is_some_and(|j| j.destroy_at == Some(at)) {
            t.dragJoint.take().unwrap()
        } else {
            let i = t.destroying.iter().position(|j| j.destroy_at == Some(at)).unwrap();
            t.destroying.remove(i)
        };
        if let Some(id) = joint.joint {
            self.world.destroy_joint(id);
        }
    }

    /// The tool body's pose to its Transform after a step (awake, no interpolation, rotation fixed about z).
    pub fn tool_write_back(&mut self) {
        let Some(t) = self.tool.as_mut() else { return };
        let b = self.world.body(t.body);
        if b.flags & crate::b2::world::BODY_AWAKE == 0 {
            return;
        }
        let half = b.sweep.a * 0.5;
        t.position = [b.xf.p.x, b.xf.p.y, t.position[2]];
        if b.sweep.a != 0.0 {
            unimplemented!("mod-only: ToolController body turned: Transform rotation write-back");
        }
        let _ = half;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::{new_game, step};

    fn ctl(g: &mut Game, key: &str, down: bool, held: bool, up: bool) {
        g.input.controls.insert(key.into(), Control { down, held, up });
    }

    // SelectionController.RefreshOutlines with the drag tool: a selected object shows its outline until it is the
    // one held, and clearing the selection hides it.
    #[test]
    fn selection_outlines() {
        let Some((mut g, first)) = spawned("Brick") else { return };
        g.select(first, false);
        assert!(g.physicals[first].showOutline);
        g.tool.as_mut().unwrap().ActiveSingleSelected = Some(first);
        g.refresh_outlines();
        assert!(!g.physicals[first].showOutline);
        g.tool.as_mut().unwrap().ActiveSingleSelected = None;
        g.clear_selection();
        assert!(!g.physicals[first].showOutline);
        g.prefs.ShowOutlines = false;
        g.select(first, false);
        assert!(!g.physicals[first].showOutline);
    }

    // Global.Update's freezeSelection: Show, FreezeAction, Hide, only with something selected.
    #[test]
    fn freeze_hotkey_acts_on_the_selection() {
        let Some((mut g, first)) = spawned("Brick") else { return };
        assert!(!g.context_hotkey("Freeze", [0.0, 0.0]));
        g.select(first, false);
        assert!(g.context_hotkey("Freeze", [0.0, 0.0]));
        frames(&mut g, 1);
        assert!(g.freezes.iter().any(|f| f.physical == first));
    }

    // ContextMenuBehaviour's Freeze, Follow and Weightless buttons toggle over the selection.
    #[test]
    fn context_menu_static_buttons() {
        let Some((mut g, first)) = spawned("Brick") else { return };
        g.select(first, false);
        g.context_menu_show();
        assert!(g.context_condition("HasObject") && g.context_condition("CanDelete") && !g.context_condition("HasCopied"));
        assert!(g.context_static_click("Freeze", [0.0, 0.0]));
        frames(&mut g, 1);
        assert!(g.freezes.iter().any(|f| f.physical == first && f.velocity == [0.0, 0.0]));
        assert!(g.freeze_stack.contains_key(&first));
        g.context_static_click("Freeze", [0.0, 0.0]);
        frames(&mut g, 1);
        assert!(!g.freeze_stack.contains_key(&first));
        g.context_static_click("Follow", [0.0, 0.0]);
        assert_eq!(g.camera_control.CurrentlyFollowing, vec![first]);
        g.context_static_click("Follow", [0.0, 0.0]);
        assert!(g.camera_control.CurrentlyFollowing.is_empty());
        g.context_static_click("Gravity", [0.0, 0.0]);
        assert!(g.physicals[first].IsWeightless);
        g.context_static_click("Gravity", [0.0, 0.0]);
        assert!(!g.physicals[first].IsWeightless);
    }

    // FreezeBehaviour.CreateOutlineObject: the Reactor A5 map's frozen parts show the outline, Disappear hides it after a second.
    #[test]
    fn frozen_outline_disappears() {
        let Some(game) = crate::game_assets::find_game(None).ok() else { return };
        let mut g = new_game(&game, "Reactor A5", None, None, 0).unwrap();
        frames(&mut g, 1);
        assert!(g.freezes.iter().any(|f| f.outline));
        frames(&mut g, 70);
        assert!(g.freezes.iter().all(|f| !f.outline));
    }

    #[test]
    fn disintegrating_a_body_cleans_up_its_prefab_joints() {
        let Some((mut g, first)) = spawned("Car") else { return };
        // A part of the car whose body holds a prefab-cloned joint (its WheelJoint2Ds).
        let (p, slot) = (first..g.physicals.len())
            .find_map(|p| {
                let body = g.physicals[p].rigidbody.body;
                let joints = g.world.body(body).joints.clone();
                joints.iter().find_map(|&(_, j)| g.anchored_joints.iter().position(|a| a.joint == Some(j))).map(|s| (p, s))
            })
            .expect("a car body with a prefab joint");
        g.physical_disintegrate(p);
        assert!(g.anchored_joints[slot].joint.is_none());
        frames(&mut g, 60);
    }

    #[test]
    fn a_truck_part_starts_the_engine() {
        let Some((mut g, first)) = spawned("Truck") else { return };
        let car = g.cars.len() - 1;
        let bubbler = (first..g.physicals.len())
            .find(|&p| {
                let id = g.physicals[p].id;
                g.with_component_instance(id, |_, inst, o| {
                    inst.prefab.objects[o].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "UseBubbler"))
                })
            })
            .expect("a truck part with a UseBubbler");
        g.activate_direct(bubbler);
        assert!(g.cars[car].Activated);
        // One broadcast through the part's bubbler, so the next one toggles it back.
        g.activate_direct(bubbler);
        assert!(!g.cars[car].Activated);
    }

    fn frames(g: &mut Game, n: usize) {
        for _ in 0..n {
            step(g, 1.0 / 60.0).unwrap();
            for c in g.input.controls.values_mut() {
                c.down = false;
                c.up = false;
            }
        }
    }

    /// A Default map game with `item` spawned (the way a "spawnRight" release does) and settled. Returns None
    /// without the installed game.
    fn spawned(item: &str) -> Option<(Game, usize)> {
        let game = crate::game_assets::find_game(None).ok()?;
        let mut g = new_game(&game, "Default", Some(item), None, 0).unwrap();
        frames(&mut g, 30);
        let first = g.physicals.len();
        let c = g.BoundingBox.center;
        let ground = g.raycast_first(crate::b2::math::Vec2::new(c[0], c[1]), [0.0, -1.0], 1.0e4)?.point.y;
        g.input.mouse = [c[0], ground + 0.6, 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        ctl(&mut g, "spawnRight", false, false, false);
        assert!(g.physicals.len() > first, "{item}: nothing spawned");
        // It lands and comes to rest, so the mouse stays on it.
        frames(&mut g, 300);
        Some((g, first))
    }

    /// Picks the object up with the mouse on a point of its collider, then holds "delete" for `n` frames. Returns the
    /// physical the click selected (the one on top where an item has several).
    fn grab_and_delete(g: &mut Game, p: usize, n: usize) -> usize {
        let sel = grab(g, p);
        ctl(g, "delete", true, true, false);
        frames(g, n);
        ctl(g, "delete", false, false, true);
        sel
    }

    /// Picks the object up with the mouse on a point of its collider and keeps holding it.
    fn grab(g: &mut Game, p: usize) -> usize {
        // A point the object's collider contains: its Transform, its centre of mass or a collider's box centre.
        let mask = g.tool.as_ref().unwrap().SelectionLayer;
        let body = g.physicals[p].rigidbody.body;
        let mut points = vec![g.physicals[p].transform.position, {
            let c = g.world.body(body).sweep.c;
            [c.x, c.y, 0.0]
        }];
        for f in g.world.body(body).fixtures.clone() {
            let fx = g.world.fixture(f);
            let b = fx.shape.compute_aabb(&g.world.body(fx.body).xf, 0);
            points.push([(b.lower.x + b.upper.x) * 0.5, (b.lower.y + b.upper.y) * 0.5, 0.0]);
        }
        let c = points.into_iter().find(|&q| !g.overlap_point(q, mask, -0.9, 0.9).is_empty()).expect("a point inside the object");
        g.input.mouse = c;
        // Global.MousePosition follows Input.mousePosition at the end of the Update scripts.
        frames(g, 1);
        ctl(g, "drag", true, true, false);
        frames(g, 2);
        ctl(g, "drag", false, true, false);
        g.tool.as_ref().unwrap().ActiveSingleSelected.expect("the object was not picked up")
    }

    /// ExcaliburBehaviour.Use charges, CastSpell instantiates the fireball, and FireballBehaviour.OnCollisionEnter2D runs
    /// once on its first hit: it goes kinematic, FreezeAll, and its DeleteAfterTime ends it. Skipped without the game.
    #[test]
    fn excalibur_casts_a_fireball_that_explodes() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut g = new_game(&game, "Default", None, None, 0).unwrap();
        frames(&mut g, 30);
        // Not in the catalog: the Resources spawnable asset's prefab, placed above the ground like a catalog spawn.
        let c = g.BoundingBox.center;
        let ground = g.raycast_first(crate::b2::math::Vec2::new(c[0], c[1]), [0.0, -1.0], 1.0e4).unwrap().point.y;
        let mut db = g.assets.take().unwrap();
        let asset = db.resource("spawnableassets/Excalibur", 114).unwrap().expect("the Excalibur asset");
        let (_, _, v) = db.mono(asset).unwrap().unwrap();
        let prefab = db.prefab(db.resolve(asset.file, v.get("Prefab").and_then(crate::assets::mono::Value::pptr).unwrap()).unwrap()).unwrap();
        let (inst, linked) = g.spawn(&mut db, &prefab, [c[0], ground + 3.0, 0.0], [0.0, 0.0, 1.0, 0.0], None).unwrap();
        g.assets = Some(db);
        g.spawnable_asset_components(&prefab, &linked, "Excalibur", 4);
        g.unsynced.push(inst);
        frames(&mut g, 2);
        let e = g.excaliburs.len() - 1;
        g.excalibur_use(e);
        let before = g.fireballs.len();
        frames(&mut g, 90);
        assert_eq!(g.fireballs.len(), before + 1, "the cast instantiated the fireball");
        frames(&mut g, 300);
        assert!(g.fireballs[before].hitOnce, "the fireball hit something");
    }

    /// A Pumpkin's run reaches PlayAtRandom.Start (a Random.value draw, no stop) some seconds after it is spawned.
    #[test]
    fn pumpkin_runs_through_play_at_random() {
        let Some((mut g, _)) = spawned("Pumpkin") else { return };
        frames(&mut g, 400);
    }

    /// Shooting a Fire Extinguisher breaks it into its Broken fire extinguisher (a DangleBehaviour on a sprite).
    #[test]
    fn shot_extinguisher_breaks_into_dangle() {
        let Some((mut g, first)) = spawned("Fire Extinguisher") else { return };
        let collider = g.physicals[first].all_colliders[0];
        let c = g.world.body(g.physicals[first].rigidbody.body).sweep.c;
        g.send_shot_for_test(collider, first, [c.x, c.y], 25.0);
        frames(&mut g, 300);
    }

    /// DamagableMachineryBehaviour.SetAsBroken sets Behaviour.enabled = false on its BehavioursToToggle: every one of them
    /// reads as disabled afterwards, leaves its manager, and the object keeps running (no stop on any class).
    #[test]
    fn set_as_broken_disables_every_toggled_behaviour() {
        for item in ["Activator Electrode", "Boat Motor", "Button", "Decimator", "Fan", "Holographic Display", "Hover Thruster", "Key Trigger", "Lagbox", "Laser Receiver", "Metronome", "Pulse Drum", "Rotor", "Tesla Coil", "Timed Gate"] {
            let Some((mut g, first)) = spawned(item) else { return };
            let ms: Vec<usize> = (0..g.machinery.len()).filter(|&m| g.machinery[m].PhysicalBehaviour == first).collect();
            for m in ms {
                let ids: Vec<i32> = g.machinery[m].BehavioursToToggle.iter().map(|b| b.1).collect();
                g.machinery_set_as_broken(m, true);
                for id in ids {
                    assert!(g.script_disabled(id), "{item}: {id} still enabled ({:?})", g.machinery[m].BehavioursToToggle);
                }
            }
            frames(&mut g, 120);
        }
    }

    /// Break then repair every catalog machine: ActOnRepair enables each BehavioursToToggle class again (its own flag or
    /// disabled_ids, then its OnEnable) without a stop.
    #[test]
    #[ignore = "walks the whole catalog (12 minutes); ONLY_ITEM=<name> runs one item"]
    fn repair_enables_every_toggled_behaviour() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let items = std::fs::read_to_string("tools/pc/items.txt").unwrap();
        for item in items.lines().map(str::trim).filter(|x| !x.is_empty()) {
            if std::env::var("ONLY_ITEM").is_ok_and(|o| o != item) {
                continue;
            }
            eprintln!("repair {item}");
            let mut g = new_game(&game, "Default", Some(item), None, 0).unwrap();
            frames(&mut g, 2);
            let c = g.BoundingBox.center;
            g.input.mouse = [c[0], c[1], 0.0];
            ctl(&mut g, "spawnRight", false, false, true);
            frames(&mut g, 1);
            ctl(&mut g, "spawnRight", false, false, false);
            for m in 0..g.machinery.len() {
                let ids: Vec<i32> = g.machinery[m].BehavioursToToggle.iter().map(|b| b.1).collect();
                if ids.is_empty() {
                    continue;
                }
                g.machinery_set_as_broken(m, true);
                g.machinery[m].Health = 10.0;
                g.machinery_act_on_repair(m);
                for id in ids {
                    assert!(!g.script_disabled(id), "{item}: {id} still disabled after the repair");
                }
            }
            frames(&mut g, 2);
        }
    }

    /// A Decimator charged over 50 turns into a BlackHole; its DoAtTime enables the BlackHoleBehaviour, whose FixedUpdate
    /// then runs (camera shake, pull, deletion) without a stop.
    #[test]
    fn charged_decimator_becomes_a_black_hole() {
        let Some((mut g, first)) = spawned("Decimator") else { return };
        g.physicals[first].charge = 60.0;
        frames(&mut g, 2);
        assert_eq!(g.black_holes.len(), 1, "the Decimator left a BlackHole");
        frames(&mut g, 900);
        assert!(g.black_holes[0].enabled, "the DoAtTime enabled the BlackHoleBehaviour");
    }

    /// RaycastHit2D.transform of a collider whose Rigidbody2D sits on a GameObject without a PhysicalBehaviour (the Handgrenade's
    /// pin) is no physical: the lookup comes back empty instead of stopping.
    #[test]
    fn hit_on_a_node_rigidbody_has_no_physical() {
        let Some((mut g, _)) = spawned("Handgrenade") else { return };
        let n = g.node_rigidbodies.first().cloned().expect("the Handgrenade has a node Rigidbody2D");
        let body = n.rigidbody.body;
        let f = g.world.fixtures.iter().flatten().find(|f| f.body == body).expect("a collider on the node body");
        let c = (f.collider.id, f.collider.game_object);
        let hit = crate::game::explosion::RayHit2D { collider: c.0, game_object: c.1, body, point: crate::b2::math::Vec2::new(0.0, 0.0), normal: crate::b2::math::Vec2::new(0.0, 1.0), distance: 0.0, fraction: 0.0 };
        assert_eq!(g.hit_transform_physical(&hit), None);
    }

    /// BloodContainer.ForceCalculateComputedColor recomputes past a cached colour.
    #[test]
    fn force_calculate_computed_color_ignores_the_cache() {
        use super::super::container::Owner;
        let Some((mut g, _)) = spawned("Human") else { return };
        let l = (0..g.limbs.len()).find(|&l| g.bc(Owner::Circulation(l)).TotalLiquidAmount > 0.0).expect("a limb holding blood");
        let o = Owner::Circulation(l);
        let real = g.force_calculate_computed_color(o, [0.0; 4]);
        g.bc_mut(o).cachedComputedColour = [9.0; 4];
        assert_eq!(g.get_computed_color(o, [0.0; 4]), [9.0; 4]);
        assert_eq!(g.force_calculate_computed_color(o, [0.0; 4]), real);
        assert!(g.bc(o).isComputedColourCached);
    }

    /// BloodContainer.ForceSetAllLiquid scales every entry so the total is the amount, clamped to the limits; an empty
    /// container is left alone.
    #[test]
    fn force_set_all_liquid_scales_every_entry() {
        use super::super::container::Owner;
        let Some((mut g, _)) = spawned("Human") else { return };
        let l = (0..g.limbs.len()).find(|&l| g.bc(Owner::Circulation(l)).TotalLiquidAmount > 0.0).expect("a limb holding blood");
        let o = Owner::Circulation(l);
        let before: Vec<(usize, f32)> = g.bc(o).LiquidDistribution.iter().map(|(k, r)| (*k, r.raw)).collect();
        let total = g.bc(o).TotalLiquidAmount;
        let want = total * 0.5;
        g.force_set_all_liquid(o, want);
        assert!((g.bc(o).TotalLiquidAmount - want).abs() <= want * 1e-5, "total {} want {want}", g.bc(o).TotalLiquidAmount);
        for (k, raw) in before {
            let now = g.bc(o).LiquidDistribution.get(k).unwrap().raw;
            assert!((now - raw * 0.5).abs() <= raw * 1e-5, "entry {k}: {now} from {raw}");
        }
        assert!(!g.bc(o).isComputedColourCached);
        g.force_set_all_liquid(o, 1.0e30);
        let want = 1.0e30f32.min(g.upper_limit(o));
        assert!((g.bc(o).TotalLiquidAmount - want).abs() <= want * 1e-4, "clamped to the upper limit");
        g.clear_liquid(o);
        g.force_set_all_liquid(o, 3.0);
        assert_eq!(g.bc(o).TotalLiquidAmount, 0.0, "an empty container stays empty");
    }

    /// HeartMonitorBehaviour.Awake spreads the 20 line vertices from LineBounds.x to LineBounds.y and clears the history.
    #[test]
    fn heart_monitor_awake_lays_out_the_line() {
        let Some((g, _)) = spawned("Heart Monitor") else { return };
        let h = &g.heart_monitors[0];
        assert_eq!(h.vertices[0], [h.LineBounds[0], 0.0, 0.0]);
        assert_eq!(h.vertices[19], [h.LineBounds[1], 0.0, 0.0]);
        let mid = h.LineBounds[0] + (h.LineBounds[1] - h.LineBounds[0]) * (10.0 / 19.0);
        assert!((h.vertices[10][0] - mid).abs() < 1e-5);
        assert!(h.history.iter().all(|&x| x == 0.0));
    }

    /// GaugeBehaviour.Update: the arm rests at MinAngle with no charge, then swings toward the square root of the
    /// charge over MaxChargeDisplay as a damped spring, and falls back to rest when the machine is destroyed.
    #[test]
    fn power_gauge_arm_follows_the_charge() {
        let Some((mut g, _)) = spawned("Power Gauge") else { return };
        let gauge = 0;
        let p = g.power_gauges[gauge].PhysicalBehaviour.expect("the gauge's physical");
        frames(&mut g, 60);
        assert_eq!(g.power_gauges[gauge].armPos, 0.0);
        let (id, arm) = (g.power_gauges[gauge].id, g.power_gauges[gauge].arm);
        let arm_q = |g: &mut Game| g.with_component_instance(id, |_, inst, _| inst.objects[arm].trs.q);
        let min = g.power_gauges[gauge].MinAngle;
        assert_eq!(arm_q(&mut g), crate::game::menu::local_euler_z_rotation(crate::game::mathf::lerp(min, g.power_gauges[gauge].MaxAngle, 0.0)), "the arm rests at MinAngle");
        for _ in 0..600 {
            g.physicals[p].charge = g.power_gauges[gauge].MaxChargeDisplay * 0.25;
            frames(&mut g, 1);
        }
        let pos = g.power_gauges[gauge].armPos;
        assert!((pos - 0.5).abs() < 0.02, "armPos {pos} settles at sqrt(0.25)");
        let (min, max) = (g.power_gauges[gauge].MinAngle, g.power_gauges[gauge].MaxAngle);
        let want = crate::game::menu::local_euler_z_rotation(crate::game::mathf::lerp(min, max, pos));
        let got = arm_q(&mut g);
        assert!((0..4).all(|i| (got[i] - want[i]).abs() < 1e-5), "the Arm transform turned to z = {} ({got:?} want {want:?})", min + (max - min) * pos);
    }

    /// LiquidPumpBehaviour.NextMode and PreviousMode wrap through the three modes (channel 1 steps back); SetMode
    /// does nothing while the pump is disabled.
    #[test]
    fn liquid_pump_modes_wrap() {
        let mut g = Game::default();
        g.liquid_pumps.push(Default::default());
        g.liquid_pumps[0].enabled = true;
        g.liquid_pumps[0].Mode = 2;
        let display = crate::game::visual::SpriteAt::Renderer(0);
        let texture = |path_id| Some(crate::assets::db::ObjRef { file: 0, path_id });
        (g.liquid_pumps[0].display, g.liquid_pumps[0].Push, g.liquid_pumps[0].Pull, g.liquid_pumps[0].Idle) = (Some(display), texture(1), texture(2), texture(3));
        g.liquid_pump_use(0, 0);
        assert_eq!(g.liquid_pumps[0].Mode, 0, "next from 2 wraps to 0");
        assert_eq!(g.material_prop(display, "_LEDMultiply"), Some(&crate::game::props::Prop::Texture(texture(1).unwrap())), "Push shows");
        g.liquid_pump_use(0, 1);
        assert_eq!(g.liquid_pumps[0].Mode, 2, "previous from 0 wraps to 2");
        g.liquid_pump_use(0, 1);
        assert_eq!(g.liquid_pumps[0].Mode, 1);
        g.liquid_pumps[0].enabled = false;
        g.liquid_pump_use(0, 0);
        assert_eq!(g.liquid_pumps[0].Mode, 1, "disabled: unchanged");
    }

    /// JetEngineBehaviour.TryStartup starts nothing for a destroyed machine; TryShutoff always starts the shutoff.
    #[test]
    fn jet_engine_startup_needs_a_live_machine() {
        let Some((mut g, _)) = spawned("Jet Engine") else { return };
        let m = g.jet_engines[0].DamagableMachinery;
        g.machinery[m].Destroyed = true;
        g.jet_engine_try_startup(0);
        frames(&mut g, 3);
        assert_eq!(g.jet_engines[0].CurrentState, 0, "still idle");
        g.machinery[m].Destroyed = false;
        g.jet_engine_try_startup(0);
        frames(&mut g, 3);
        assert_eq!(g.jet_engines[0].CurrentState, 1, "starting");
    }

    /// A Jet Engine moved (Transform.position =) and then used: the exhaust collider is recreated by the next
    /// SyncTransforms, and ExhaustAffectObjects still finds its fixtures.
    #[test]
    fn moved_jet_engine_still_exhausts() {
        let Some((mut g, first)) = spawned("Jet Engine") else { return };
        let last = g.physicals.len();
        for q in first..last {
            g.move_physical_for_test(q, [0.0, 1.0, 0.0]);
        }
        frames(&mut g, 4);
        let c = g.world.body(g.physicals[first].rigidbody.body).sweep.c;
        g.input.mouse = [c.x, c.y, 0.0];
        ctl(&mut g, "activateDirect", true, true, false);
        frames(&mut g, 1);
        ctl(&mut g, "activateDirect", false, false, true);
        frames(&mut g, 400);
    }

    /// The Truck's Gastank heated past its limit explodes (its body goes); setAngle on it afterwards leaves the clone with
    /// no stale body for SyncTransforms to move.
    #[test]
    fn angle_of_an_exploded_gastank_is_harmless() {
        let Some((mut g, first)) = spawned("Truck") else { return };
        let tank = (first..g.physicals.len()).find(|&q| g.physicals[q].name == "Gastank").expect("the Truck's Gastank");
        g.physicals[tank].Temperature = 1500.0;
        frames(&mut g, 200);
        g.physical_set_rotation(tank, crate::game::menu::euler_z_rotation(1500.0));
        frames(&mut g, 60);
    }

    /// SelectionController.FlipSelection on a single root with a PhysicalBehaviour: localScale.x is negated and the
    /// OnFlip receivers run (PhysicalBehaviour, PistonBehaviour). Flipping back restores the scale.
    #[test]
    fn flip_mirrors_a_single_root() {
        let Some((mut g, first)) = spawned("Piston") else { return };
        grab(&mut g, first);
        let before = g.physicals[first].transform.localScale;
        ctl(&mut g, "flip", true, false, false);
        frames(&mut g, 1);
        assert_eq!(g.physicals[first].transform.localScale, [0.0 - before[0], before[1], before[2]]);
        ctl(&mut g, "flip", true, false, false);
        frames(&mut g, 1);
        assert_eq!(g.physicals[first].transform.localScale, before);
        frames(&mut g, 30);
    }

    /// SelectionController.FlipSelection on a Human (a root without a PhysicalBehaviour): the several-roots branch mirrors
    /// every limb about one x, the root's scale.x goes negative and the person's AngleOffset follows; twice restores it.
    #[test]
    fn flip_mirrors_a_human() {
        let Some((mut g, first)) = spawned("Human") else { return };
        grab(&mut g, first);
        let xs = |g: &Game| g.physicals[first..].iter().map(|p| p.transform.position).collect::<Vec<_>>();
        let before = xs(&g);
        ctl(&mut g, "flip", true, false, false);
        frames(&mut g, 1);
        let after = xs(&g);
        assert!(g.people[0].localScale[0] < 0.0 && g.people[0].AngleOffset == -1.0, "{:?} {} {:?}", g.people[0].localScale, g.people[0].AngleOffset, after.iter().zip(&before).map(|(a, b)| (b[0], a[0])).take(3).collect::<Vec<_>>());
        let sums: Vec<f32> = before.iter().zip(&after).map(|(b, a)| b[0] + a[0]).collect();
        assert!(sums.iter().all(|s| (s - sums[0]).abs() < 1e-3), "{sums:?}");
        assert!(before.iter().zip(&after).all(|(b, a)| (b[1] - a[1]).abs() < 1e-3));
        frames(&mut g, 30);
    }

    #[test]
    fn clear_everything_destroys_the_layers_and_reloads_the_map() {
        let Some((mut g, first)) = spawned("Stick") else { return };
        let old = g.map_root.expect("the map loader recorded its clone");
        g.tracer_pool.created = 3;
        g.tracer_pool.available = 3;
        g.ambient_grid.World.clear();
        g.clear_everything();
        frames(&mut g, 3);
        // Item layer: gone with its undo entry. The old map clone was destroyed and a new one stands.
        assert!(g.physicals[first].destroyed);
        assert!(g.undo.history.is_empty());
        assert!(g.destroyed_components.contains(&old));
        let new = g.map_root.unwrap();
        assert_ne!(new, old);
        assert!(!g.destroyed_components.contains(&new));
        assert_eq!((g.tracer_pool.created, g.tracer_pool.available), (0, 0));
        assert_eq!(g.notifications.last().unwrap(), crate::il!(str "ClearButtonBehaviour::ClearEverything" 0));
        frames(&mut g, 30);
    }

    // Clear everything on a map with water (Hybrid): the old map's water stays listed until its destroy at the frame's
    // end with its trigger disabled; bleeding's IsPointUnderWater must read it as not inside (it crashed).
    #[test]
    fn clear_everything_on_a_water_map() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut g = new_game(&game, "Hybrid", Some("Human"), None, 0).unwrap();
        frames(&mut g, 5);
        assert!(!g.water_list.is_empty(), "Hybrid has water");
        g.clear_everything();
        let _ = g.is_point_under_water([0.0, 0.0]);
        frames(&mut g, 30);
        g.clear_everything();
        frames(&mut g, 30);
    }

    // A fired Blaster / Archelix Caster starts its muzzle flash particles (they were never played).
    #[test]
    fn weapons_play_their_muzzle_flash() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        for item in ["Blaster", "Archelix Caster"] {
            let mut g = new_game(&game, "Default", None, None, 0).unwrap();
            let mut db = g.assets.take().expect("the game's data");
            let (inst, _) = g.catalog_spawn(&mut db, item, [0.0, 2.0, 0.0], false).expect(item);
            g.assets = Some(db);
            g.unsynced.push(inst);
            frames(&mut g, 5);
            g.ps_calls.clear();
            match item {
                "Blaster" => g.blaster_shoot(0),
                // The burst's first step runs in StartCoroutine; a frame would hand the calls to the particles.
                _ => g.archelix_caster_use(0),
            }
            assert!(g.ps_calls.iter().any(|c| matches!(c.1, super::super::PsCall::Play)), "{item}: no muzzle flash Play");
        }
    }

    // A Blaster bolt's TrailRenderer draws as a line behind it; a held Heatray shows its beam.
    #[test]
    fn blaster_bolt_trail_and_heatray_beam_draw() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        for item in ["Blaster", "Heatray"] {
            let mut g = new_game(&game, "Default", None, None, 0).unwrap();
            let mut db = g.assets.take().expect("the game's data");
            let first = g.physicals.len();
            let (inst, _) = g.catalog_spawn(&mut db, item, [0.0, 2.0, 0.0], false).expect(item);
            g.assets = Some(db);
            g.unsynced.push(inst);
            frames(&mut g, 5);
            let mut seen = 0;
            for _ in 0..10 {
                if item == "Blaster" { g.blaster_shoot(0) } else { let a = g.signal_direct(true, 0); g.broadcast_use_continuous(first, &a); }
                frames(&mut g, 1);
                seen = seen.max(g.lines.iter().filter(|l| l.visible && l.enabled && l.positions.len() >= 2).count());
            }
            assert!(seen > 0, "{item}: no line drawn");
            if item == "Blaster" {
                assert!(!g.trails.is_empty() && g.trails.iter().all(|t| t.owner.is_some() || !g.lines[t.line].visible));
            }
        }
    }

    // Survey: each energy weapon clicked and then held for a second; what effects it starts (cargo test -- --ignored --nocapture).
    #[test]
    #[ignore]
    fn energy_weapon_effects_survey() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        for item in ["Beam Repeater", "Beam Rifle", "Blaster", "Blaster Rifle", "Machine Blaster", "Detached Beam Cannon", "Detached Laser Cannon", "Detached Ray Cannon", "Disassembler", "Ion Cannon", "Accelerator", "Archelix Caster", "Pulse Drum", "Railgun", "Spike Rifle", "Stormbaan", "Stunner", "Heatray", "Freezeray", "Reconstructor", "Rigidifier", "Excalibur", "Energy Sword", "Worm Staff"] {
            let mut g = new_game(&game, "Default", None, None, 0).unwrap();
            let mut db = g.assets.take().expect("the game's data");
            let first = g.physicals.len();
            let Ok((inst, _)) = g.catalog_spawn(&mut db, item, [0.0, 2.0, 0.0], false) else { println!("FX {item}: no such item"); g.assets = Some(db); continue };
            g.assets = Some(db);
            let mut classes: Vec<String> = inst.prefab.objects.iter().flat_map(|o| o.components.iter().filter_map(|(_, c)| match c { crate::assets::db::Component::Script { class, .. } => Some(class.clone()), _ => None })).collect();
            classes.sort(); classes.dedup();
            println!("CLS {item}: {}", classes.join(" "));
            g.unsynced.push(inst);
            frames(&mut g, 5);
            let (lines0, fx0) = (g.lines.len(), g.effect_log.len());
            g.ps_calls.clear();
            let (mut seen_lines, base, mut peak) = (0, crate::particles::live_particles(&g), 0);
            g.activate_for_test(first);
            for k in 0..60 {
                for p in first..g.physicals.len() {
                    g.physicals[p].ContinuousActivationTracker[0].Value = if k < 50 { 1 } else { 0 };
                }
                // Held: the LateUpdate's BroadcastMessage("UseContinuous") on the item.
                if k < 50 {
                    let a = g.signal_direct(true, 0);
                    g.broadcast_use_continuous(first, &a);
                }
                frames(&mut g, 1);
                peak = peak.max(crate::particles::live_particles(&g));
                seen_lines = seen_lines.max(g.lines.iter().filter(|l| l.visible && l.enabled && l.positions.len() >= 2).count());
            }
            let _ = fx0;
            println!("FX {item}: particles before {base}, peak {peak}; lines {} (new {}, trails {}), visible lines max {seen_lines}", g.lines.len(), g.lines.len() - lines0, g.trails.len());
        }
    }

    /// The particles the machines' and bodies' scripts start: a head shot kill's KillShotParticles, the Thruster's effect
    /// once on, the Blood Tank's drain, the Energy Sword's boiling blade and Sizzle's smoke puff. Skipped without the game.
    #[test]
    fn machine_and_body_particles_play() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let spawn = |item: &str| {
            let mut g = new_game(&game, "Default", None, None, 0).unwrap();
            let mut db = g.assets.take().unwrap();
            let first = g.physicals.len();
            let (inst, _) = g.catalog_spawn(&mut db, item, [0.0, 2.0, 0.0], false).unwrap_or_else(|e| panic!("{item}: {e}"));
            g.assets = Some(db);
            g.unsynced.push(inst);
            frames(&mut g, 5);
            (g, first)
        };
        // The kill shot: a dead limb's exit shots until the 0.3 chance comes up.
        let (mut g, _) = spawn("Human");
        let l = g.limbs.iter().position(|l| l.KillShotParticles).expect("a limb with KillShotParticles");
        let id = g.limbs[l].id;
        let k = g.field_ps(id, "KillShotParticles").expect("the KillShotParticles system");
        assert!(!crate::particles::playing(&g, k));
        let at = g.physicals[g.limbs[l].PhysicalBehaviour].transform.position;
        for _ in 0..100 {
            if g.limbs[l].KillShotParticlesEmitted {
                break;
            }
            g.limbs[l].Health = 0.0;
            g.limb_exit_shot(l, [1.0, 0.0], [at[0], at[1]], 1.0);
        }
        assert!(g.limbs[l].KillShotParticlesEmitted);
        frames(&mut g, 2);
        assert!(crate::particles::playing(&g, k), "KillShotParticles plays");
        // Machines switched on: more live particles than the same frames before.
        for (item, on) in [("Thruster", 0), ("Blood Tank", 1), ("Energy Sword", 2)] {
            let (mut g, first) = spawn(item);
            frames(&mut g, 60);
            let base = crate::particles::live_particles(&g);
            match on {
                0 => g.activate_for_test(first),
                // Filled, then Idle to Drain.
                1 => {
                    g.add_liquid(crate::game::container::Owner::BloodTank(0), 0, 1.0);
                    g.blood_tanks[0].Mode = 2;
                    g.blood_tank_use(0, 0);
                }
                _ => g.energy_sword_use(0),
            }
            let mut peak = 0;
            for _ in 0..60 {
                frames(&mut g, 1);
                peak = peak.max(crate::particles::live_particles(&g));
            }
            assert!(peak > base, "{item}: particles before {base}, peak {peak}");
        }
        // Sizzle: SmokeParticleSystem.Emit(5).
        let (mut g, first) = spawn("Brick");
        frames(&mut g, 60);
        let base = crate::particles::live_particles(&g);
        g.sizzle(first, false);
        frames(&mut g, 1);
        assert!(crate::particles::live_particles(&g) >= base + 5, "the sizzle's smoke");
    }

    /// `item` spawned at (0, 2) and used continuously for `n` frames.
    fn used_for(item: &str, n: usize) -> Option<(Game, usize)> {
        let game = crate::game_assets::find_game(None).ok()?;
        let mut g = new_game(&game, "Default", None, None, 0).unwrap();
        let mut db = g.assets.take().expect("the game's data");
        let first = g.physicals.len();
        let (inst, _) = g.catalog_spawn(&mut db, item, [0.0, 2.0, 0.0], false).unwrap();
        g.assets = Some(db);
        g.unsynced.push(inst);
        frames(&mut g, 5);
        g.activate_for_test(first);
        for _ in 0..n {
            for p in first..g.physicals.len() {
                g.physicals[p].ContinuousActivationTracker[0].Value = 1;
            }
            frames(&mut g, 1);
        }
        Some((g, first))
    }

    /// LightningGunBehaviour.PerformLightning's TargetPosition is the ParticleSystemForceField the Stormbaan's bolt
    /// particles head for: with nothing in range it sits Range along the gun's right from the particle system.
    #[test]
    fn stormbaan_force_field_sits_at_the_bolt_end() {
        let Some((g, _)) = used_for("Stormbaan", 10) else { return };
        let (origin, dir, range) = (g.lightning_gun_ray_origin(0), g.lightning_gun_direction(0), g.lightning_guns[0].Range);
        let want = [origin[0] + dir[0] * range, origin[1] + dir[1] * range];
        let &id = g.force_fields.last().expect("the Stormbaan's force field");
        let (inst, o, _) = g.component_at(id).unwrap();
        let (a, chain) = g.object_anchor(inst, o);
        let m = crate::unity::transform::local_to_world(&g.anchored_nodes(a, &chain));
        let at = [m[12], m[13]];
        assert!((at[0] - want[0]).abs() < 1e-3 && (at[1] - want[1]).abs() < 1e-3, "{at:?} vs {want:?}");
    }

    /// WormStaffBehaviour: each worm's line has VertexCount points from the staff out to its end; SimpleLightningEmitter's
    /// pooled line (the Accelerator's arcs) and the Archelix bolts' trails show while they fire.
    #[test]
    fn worm_lightning_and_trail_lines_draw() {
        let shown = |g: &Game| g.lines.iter().filter(|l| l.visible && l.enabled && l.positions.len() >= 2).count();
        let Some((g, _)) = used_for("Worm Staff", 20) else { return };
        let worms = &g.worm_staffs[0];
        assert_eq!(worms.lines.len(), 3);
        for (w, &l) in worms.lines.iter().enumerate() {
            let p = &g.lines[l].positions;
            assert_eq!(p.len(), 16);
            assert_eq!(p[0], [0.0; 3]);
            let end = worms.worms[w].endPosition;
            assert!((p[15][0] - end[0] * 15.0 / 16.0).abs() < 0.2 && (p[15][1] - end[1] * 15.0 / 16.0).abs() < 0.2, "{:?} {end:?}", p[15]);
        }
        let mut seen = 0;
        let Some((mut g, first)) = used_for("Accelerator", 0) else { return };
        for k in 0..120 {
            for p in first..g.physicals.len() {
                g.physicals[p].ContinuousActivationTracker[0].Value = if k < 100 { 1 } else { 0 };
            }
            frames(&mut g, 1);
            if let Some(l) = g.lines.iter().find(|l| l.visible && l.enabled && l.positions.len() == crate::game::lightning::LIGHTNING_VERTICES) {
                assert_eq!(l.width, 0.04);
                seen += 1;
            }
        }
        assert!(seen > 0, "no lightning line");
        let Some((g, _)) = used_for("Archelix Caster", 15) else { return };
        assert!(g.archelix_bolts.iter().any(|b| b.trail_line.is_some_and(|l| g.lines[l].visible && g.lines[l].positions.len() >= 2)) && shown(&g) > 0);
    }

    /// DistanceJointWireBehaviour.OnPreFlip / OnFlip: a flipped wire keeps its length. The distance is remembered and
    /// autoConfigureDistance stays off through the flip, and the next fixed update turns it on again.
    #[test]
    fn flipping_a_wired_object_keeps_the_wire_length() {
        let Some((mut g, a, _)) = wired("CableWireTool") else { return };
        let f = g.blood_wires[0].joint;
        let length = g.distance_joint_distance(f);
        grab(&mut g, a);
        let root = g.root_id(a); let at = g.physicals[a].transform.position; g.flip_broadcast(root, true, at);
        assert!(!g.fixed_joints[f].distance.as_ref().unwrap().auto_configure_distance);
        assert_eq!(g.blood_wires[0].distanceBeforeFlip, length);
        g.flip_broadcast(root, false, at);
        assert_eq!(g.distance_joint_distance(f), length);
        assert!(!g.fixed_joints[f].distance.as_ref().unwrap().auto_configure_distance, "off until the next fixed update");
        frames(&mut g, 2);
        assert!(g.fixed_joints[f].distance.as_ref().unwrap().auto_configure_distance);
        assert_eq!(g.distance_joint_distance(f), length);
        // The real key flips the same way.
        ctl(&mut g, "flip", true, false, false);
        frames(&mut g, 3);
        assert_eq!(g.distance_joint_distance(f), length);
    }

    /// FixedJointWireBehaviour.OnPreFlip / OnFlip: a welded wire's joint is rebuilt on the flipped bodies and
    /// autoConfigureConnectedAnchor comes back on at the next fixed update.
    #[test]
    fn flipping_a_welded_object_rebuilds_its_fixed_wire() {
        let Some((mut g, a, b)) = wired("FixedCableTool") else { return };
        let f = g.blood_wires[0].joint;
        assert!(g.fixed_joints[f].connected.is_some());
        grab(&mut g, a);
        let root = g.root_id(a); let at = g.physicals[a].transform.position; g.flip_broadcast(root, true, at);
        assert!(!g.fixed_joints[f].state.auto_configure_connected_anchor);
        g.flip_broadcast(root, false, at);
        assert!(g.fixed_joints[f].connected.is_some() && g.fixed_joints[f].joint.is_some());
        assert!(!g.fixed_joints[f].state.auto_configure_connected_anchor);
        frames(&mut g, 2);
        assert!(g.fixed_joints[f].state.auto_configure_connected_anchor);
        frames(&mut g, 30);
        let _ = b;
    }

    /// SelectionController.Select(PhysicalBehaviour, multiple): a plain select replaces the selection, multiple adds,
    /// a selected object is not added twice, an unselectable one is ignored.
    #[test]
    fn select_replaces_unless_multiple() {
        let Some((mut g, a)) = spawned("Stick") else { return };
        let at = g.physicals[a].transform.position;
        let b = spawn_at(&mut g, "Stick", [at[0] + 1.5, at[1], 0.0]);
        frames(&mut g, 30);
        g.select(a, false);
        g.select(b, true);
        g.select(b, true);
        assert_eq!(g.tool.as_ref().unwrap().rawSelection, vec![a, b]);
        g.select(a, false);
        assert_eq!(g.tool.as_ref().unwrap().rawSelection, vec![a]);
        g.physicals[b].Selectable = false;
        g.select(b, true);
        g.select(b, false);
        assert_eq!(g.tool.as_ref().unwrap().rawSelection, vec![a], "an unselectable object changes nothing");
    }

    /// SelectionController.UserDeleteSelection: each Deletable selected object's root is destroyed once, an
    /// undeletable one stays, and the names are announced once each.
    #[test]
    fn user_delete_destroys_deletable_roots() {
        let Some((mut g, a)) = spawned("Stick") else { return };
        let at = g.physicals[a].transform.position;
        let b = spawn_at(&mut g, "Stick", [at[0] + 1.5, at[1], 0.0]);
        let c = spawn_at(&mut g, "Stick", [at[0] + 3.0, at[1], 0.0]);
        frames(&mut g, 30);
        g.physicals[c].Deletable = false;
        g.select(a, false);
        g.select(b, true);
        g.select(c, true);
        g.user_delete_selection();
        frames(&mut g, 2);
        assert!(g.physicals[a].destroyed && g.physicals[b].destroyed && !g.physicals[c].destroyed);
    }

    /// ToolControllerBehaviour.HandleTools + StopSelectionDrag: a drag that starts on empty ground and ends past
    /// sqrt(200) px away selects what the box overlaps, replacing the selection; a short drag selects nothing.
    #[test]
    fn dragging_a_box_selects_what_it_overlaps() {
        let Some((mut g, a)) = spawned("Stick") else { return };
        let at = g.physicals[a].transform.position;
        let b = spawn_at(&mut g, "Stick", [at[0] + 1.5, at[1], 0.0]);
        frames(&mut g, 30);
        let screen = |m: [f32; 3]| Some(super::Screen { mouse: [m[0] * 100.0, m[1] * 100.0], origin: [0.0; 2], scale: 0.01, view: None });
        let (pa, pb) = (g.physicals[a].transform.position, g.physicals[b].transform.position);
        let start = [pa[0] - 1.0, pa[1] + 2.0, 0.0];
        let end = [pb[0] + 1.0, pb[1] - 2.0, 0.0];
        g.select(a, false);
        g.input.mouse = start;
        g.input.screen = screen(start);
        frames(&mut g, 2);
        ctl(&mut g, crate::il!(str "ToolControllerBehaviour::HandleTools" 0), true, true, false);
        frames(&mut g, 1);
        assert!(g.tool.as_ref().unwrap().isSelecting && g.tool.as_ref().unwrap().rawSelection.is_empty());
        g.input.mouse = end;
        g.input.screen = screen(end);
        ctl(&mut g, crate::il!(str "ToolControllerBehaviour::HandleTools" 0), false, true, false);
        frames(&mut g, 2);
        ctl(&mut g, crate::il!(str "ToolControllerBehaviour::HandleTools" 0), false, false, true);
        frames(&mut g, 1);
        let t = g.tool.as_ref().unwrap();
        assert!(!t.isSelecting);
        assert!(t.rawSelection.contains(&a) && t.rawSelection.contains(&b), "{:?}", t.rawSelection);
        // A drag shorter than sqrt(200) px selects nothing.
        g.select(a, false);
        g.input.mouse = start;
        g.input.screen = screen(start);
        frames(&mut g, 2);
        ctl(&mut g, crate::il!(str "ToolControllerBehaviour::HandleTools" 0), true, true, false);
        frames(&mut g, 1);
        ctl(&mut g, crate::il!(str "ToolControllerBehaviour::HandleTools" 0), false, false, true);
        frames(&mut g, 1);
        assert!(g.tool.as_ref().unwrap().rawSelection.is_empty() && !g.tool.as_ref().unwrap().isSelecting);
    }

    /// ResistorBehaviour.OnFlip: flipped follows the sign of lossyScale.x, so a flip inverts ResistorPower.
    #[test]
    fn flipped_resistor_inverts_its_power() {
        let Some((mut g, first)) = spawned("Resistor") else { return };
        let r = g.tr.resistor_behaviour.len() - 1;
        let was = g.tr.resistor_behaviour[r].flipped;
        grab(&mut g, first);
        ctl(&mut g, "flip", true, false, false);
        frames(&mut g, 1);
        assert_eq!(g.tr.resistor_behaviour[r].flipped, !was);
        ctl(&mut g, "flip", true, false, false);
        frames(&mut g, 1);
        assert_eq!(g.tr.resistor_behaviour[r].flipped, was);
    }

    /// EBikeBehaviour.OnFlip runs Start again: the hinge motor speed takes the sign of the new lossyScale.x.
    #[test]
    fn flipped_bicycle_reverses_its_motor() {
        let Some((mut g, first)) = spawned("Bicycle") else { return };
        let b = g.tr.e_bike_behaviour.len() - 1;
        let h = g.tr.e_bike_behaviour[b].Hinge.unwrap();
        let before = g.hinges[h].motor[0];
        grab(&mut g, first);
        ctl(&mut g, "flip", true, false, false);
        frames(&mut g, 1);
        assert_eq!(g.hinges[h].motor[0], -before);
    }

    /// GorseEyeBehaviour (no catalog prefab carries it, so one is attached to the Gorse's drooling object): Awake draws the
    /// intervals inside their ranges, a live limb moves the object inside the Container, and an unconscious one stops it.
    #[test]
    fn gorse_eye_jumps_inside_its_container() {
        let Some((mut g, _)) = spawned("Gorse") else { return };
        let id = g.gorse_droolings[0].id;
        let limb = g.people.iter().position(|_| true).map(|_| g.limbs.iter().position(|l| !l.Frozen).unwrap());
        g.gorse_eyes.push(super::gorse::GorseEye {
            id,
            limb,
            container_position: [0.5, 0.25],
            container_size: [0.1, 0.2],
            SmallRandomRadius: 0.01,
            LargeRange: [0.1, 0.2],
            SmallRange: [0.05, 0.1],
            largeInterval: 0.0,
            smallInterval: 0.0,
            largeTimer: 0.0,
            smallTimer: 0.0,
            isRendering: false,
        });
        let e = g.gorse_eyes.len() - 1;
        g.gorse_eye_awake(e);
        let eye = &g.gorse_eyes[e];
        assert!((0.1..=0.2).contains(&eye.largeInterval) && (0.05..=0.1).contains(&eye.smallInterval));
        let pos = |g: &mut Game| g.with_component_instance(id, |_, inst, o| inst.objects[o].trs.t);
        let start = pos(&mut g);
        for _ in 0..40 {
            g.gorse_eye_update(e);
            g.time.deltaTime = 0.05;
        }
        let moved = pos(&mut g);
        assert_ne!(start, moved);
        assert!(g.gorse_eyes[e].isRendering);
        assert!((0.49..0.61).contains(&moved[0]) && (0.24..0.46).contains(&moved[1]), "{moved:?}");
        let l = g.gorse_eyes[e].limb.unwrap();
        g.people[g.limbs[l].Person].Consciousness = 0.0;
        g.gorse_eye_update(e);
        assert!(!g.gorse_eyes[e].isRendering);
        assert_eq!(pos(&mut g), moved);
    }

    /// BloodTankBehaviour.NextMode / PreviousMode wrap through Push, Pull, Idle, Drain; ConnectedNodeBehaviour.ResetValue
    /// puts a node back to 0 (a root to 1000000) and disconnects it.
    #[test]
    fn blood_tank_modes_wrap_and_nodes_reset() {
        let Some((mut g, _)) = spawned("Blood Tank") else { return };
        let t = g.blood_tanks.len() - 1;
        g.blood_tanks[t].Mode = 3;
        g.blood_tank_next_mode(t);
        assert_eq!(g.blood_tanks[t].Mode, 0);
        g.blood_tank_previous_mode(t);
        assert_eq!(g.blood_tanks[t].Mode, 3);
        g.blood_tank_previous_mode(t);
        assert_eq!(g.blood_tanks[t].Mode, 2);
        let Some((mut g, _)) = spawned("Human") else { return };
        for n in 0..g.nodes.len() {
            g.nodes[n].Value = 7;
            g.nodes[n].IsConnectedToRoot = true;
            g.connected_node_reset_value(n);
            assert!(!g.nodes[n].IsConnectedToRoot);
            assert_eq!(g.nodes[n].Value, if g.nodes[n].IsRoot { 1_000_000 } else { 0 });
        }
    }

    /// DamagableMachineryBehaviour.Break takes BreakDamage off Health, Slice zeroes it and destroys the machine; an
    /// Indestructible one ignores both. PhysicalBehaviour.IsBeingStabbed / LocalGridPointLength read their fields.
    #[test]
    fn machinery_break_and_slice() {
        let Some((mut g, first)) = spawned("Fan") else { return };
        let m = g.machinery.iter().position(|m| m.PhysicalBehaviour == first).unwrap();
        assert!(g.local_grid_point_length(first) > 0 && !g.is_being_stabbed(first));
        g.machinery[m].Indestructible = true;
        g.machinery[m].Health = 5.0;
        g.machinery_slice(m);
        g.machinery_break(m);
        assert_eq!(g.machinery[m].Health, 5.0);
        g.machinery[m].Indestructible = false;
        g.machinery[m].BreakDamage = 1.5;
        g.machinery_break(m);
        assert_eq!(g.machinery[m].Health, 3.5);
        g.machinery_slice(m);
        assert_eq!(g.machinery[m].Health, 0.0);
        assert!(g.machinery[m].Destroyed);
    }

    /// LiquidContainerController.NoNeedToSimulate is true for an empty or full container; OffsetRepulsionDistance grows with the fill.
    #[test]
    fn liquid_container_simulation_gate() {
        let Some((mut g, _)) = spawned("Flask") else { return };
        let Some(c) = g.liquid_containers.iter().position(|c| c.fancy) else { return };
        g.liquid_containers[c].FillPercentage = 0.0;
        assert!(g.liquid_no_need_to_simulate(c));
        g.liquid_containers[c].FillPercentage = 1.0;
        assert!(g.liquid_no_need_to_simulate(c));
        g.liquid_containers[c].FillPercentage = 0.5;
        assert!(!g.liquid_no_need_to_simulate(c));
        assert!((g.liquid_offset_repulsion_distance(c) - 0.11).abs() < 1e-6);
    }

    /// ToolControllerBehaviour.HandleToolSelectionInput / SetToolByTypeName / SetTool: "tool2" picks the second parentless
    /// entry (the Wire tool), pressing it again cycles its children from the last (Chain), and "tool1" is the DragTool again.
    #[test]
    fn tool_number_keys_pick_and_cycle() {
        let Some((mut g, _)) = spawned("Flask") else { return };
        let lib = g.tool_collection(0);
        assert_eq!((lib[0].ty.as_str(), lib[1].ty.as_str()), ("DragTool", "EnergyWireTool"));
        ctl(&mut g, "tool2", true, false, false);
        frames(&mut g, 1);
        assert!(g.wire_tool.chosen);
        assert_eq!((g.tool.as_ref().unwrap().parentCycleIndex, g.tool.as_ref().unwrap().childCount), (2, 5));
        ctl(&mut g, "tool2", true, false, false);
        frames(&mut g, 1);
        assert!(g.wire_tool.chosen);
        assert_eq!(g.tool.as_ref().unwrap().childCycleIndex, 0);
        assert!(g.wire_tool.kind == super::blood_wire::WireKind::of_tool("ChainWireTool").unwrap());
        ctl(&mut g, "tool1", true, false, false);
        frames(&mut g, 1);
        assert!(!g.wire_tool.chosen && g.tool.as_ref().unwrap().isActive);
    }

    /// ToolSelectionMode.Depth hovers the hit with the highest sorting layer, then the highest sorting order.
    #[test]
    fn depth_hovering_picks_the_top_sprite() {
        let Some((mut g, first)) = spawned("Human") else { return };
        g.prefs.ToolSelectionMode = SELECTION_DEPTH;
        let at = g.physicals[first + 1].transform.position;
        g.input.mouse = at;
        let mask = g.tool.as_ref().unwrap().SelectionLayer;
        let hits = g.overlap_point(g.MousePosition, mask, -0.9, 0.9);
        assert!(hits.len() > 1);
        let key = |g: &Game, p: usize| (crate::play::sorting_layer_index(g.physicals[p].sprite_layer), g.physicals[p].sprite.map_or(0, |s| s.2));
        let top = hits.iter().map(|&p| key(&g, p)).max().unwrap();
        frames(&mut g, 1);
        let hover = g.tool.as_ref().unwrap().hover.expect("something hovered");
        assert_eq!(key(&g, hover), top);
    }

    /// SelectionController.Select(IEnumerable) adds each selectable one once (replacing the selection unless multiple),
    /// Deselect removes one and ClearSelection empties it.
    #[test]
    fn selection_controller_select_many_deselect_clear() {
        let Some((mut g, first)) = spawned("Human") else { return };
        g.clear_selection();
        g.select_many(vec![first, first + 1, first + 1], false);
        assert_eq!(g.tool.as_ref().unwrap().rawSelection, vec![first, first + 1]);
        g.select_many(vec![first + 2], true);
        assert_eq!(g.tool.as_ref().unwrap().rawSelection, vec![first, first + 1, first + 2]);
        g.physicals[first + 3].Selectable = false;
        g.select_many(vec![first + 3], true);
        assert_eq!(g.tool.as_ref().unwrap().rawSelection.len(), 3);
        g.deselect(first + 1);
        assert_eq!(g.tool.as_ref().unwrap().rawSelection, vec![first, first + 2]);
        g.select_many(vec![first + 4], false);
        assert_eq!(g.tool.as_ref().unwrap().rawSelection, vec![first + 4]);
        g.clear_selection();
        assert!(g.tool.as_ref().unwrap().rawSelection.is_empty());
    }

    #[test]
    fn delete_destroys_the_selected_object_through_object_destroy() {
        for item in ["Human", "Flask", "Pistol", "Generator", "Fan", "Handgrenade", "Piston"] {
            let Some((mut g, first)) = spawned(item) else { return };
            assert!(g.undo.history.len() == 1, "{item}");
            let p = grab_and_delete(&mut g, first, 3);
            let (id, root) = (g.physicals[p].id, g.root_id(p));
            // Object.Destroy ran: the component reads null, OnDestroy dropped the undo entry and the object left the loops.
            assert!(g.physicals[p].destroyed && !g.physicals[p].enabled, "{item}: still alive");
            // The whole root GameObject went: every physical object of the item, and a person with its limbs and joints.
            assert!(g.physicals[first..].iter().all(|q| q.destroyed), "{item}: a part survived");
            assert!(g.people.iter().all(|q| q.destroyed) && g.hinges.iter().all(|h| h.joint.is_none()), "{item}: person");
            assert!(g.destroyed_components.contains(&id) && g.destroyed_components.contains(&root), "{item}");
            assert!(g.undo.history.is_empty() && g.undo.currentHistoryPosition == -1, "{item}: DeregisterBehaviour.OnDestroy");
            assert!(g.notifications.contains(&format!("<b>{}</b> removed", item.to_uppercase())), "{item}: {:?}", g.notifications);
            // The held object is gone: the drag joint went with its body and the next frames run on.
            frames(&mut g, 30);
        }
    }

    #[test]
    fn clear_living_and_clear_mess() {
        let Some((mut g, first)) = spawned("Human") else { return };
        g.clear_debris();
        g.clear_living();
        frames(&mut g, 3);
        assert!(g.physicals[first..].iter().all(|q| q.destroyed) && g.people.iter().all(|p| p.destroyed));
        assert!(g.undo.history.is_empty());
        assert_eq!(g.notifications, [crate::il!(str "ClearDebrisBehaviour::Clear" 0), crate::il!(str "ClearLivingBehaviour::Clear" 0)]);
        frames(&mut g, 30);
    }

    #[test]
    fn undo_destroys_the_last_spawn() {
        let Some((mut g, first)) = spawned("Flask") else { return };
        ctl(&mut g, "undo", false, false, true);
        frames(&mut g, 2);
        assert!(g.physicals[first].destroyed);
        assert!(g.notifications.contains(&"<b>FLASK</b> removed".to_string()));
        assert!(g.undo.history.is_empty());
    }

    #[test]
    fn bypass_machine_use_shows_and_hides_its_cosmetics() {
        let Some((mut g, first)) = spawned("Cardiopulmonary Bypass Machine") else { return };
        let (id, cosmetics) = (g.cardiopulmonary_bypasses[0].id, g.cardiopulmonary_bypasses[0].Cosmetics);
        let shown = |g: &mut Game| g.with_component_instance(id, |_, inst, _| inst.objects[cosmetics].active);
        // Start: Activated is false, so Cosmetics is hidden.
        assert!(!g.cardiopulmonary_bypasses[0].Activated && !shown(&mut g));
        { let a = g.signal_direct(true, 0); g.broadcast_use(first, &a); }
        assert!(g.cardiopulmonary_bypasses[0].Activated && shown(&mut g));
        { let a = g.signal_direct(true, 0); g.broadcast_use(first, &a); }
        assert!(!g.cardiopulmonary_bypasses[0].Activated && !shown(&mut g));
    }

    /// A point on a physical's collider that the selection mask overlaps.
    fn point_on(g: &mut Game, p: usize) -> Vec<[f32; 3]> {
        let mask = g.tool.as_ref().unwrap().SelectionLayer;
        let body = g.physicals[p].rigidbody.body;
        let mut points = vec![g.physicals[p].transform.position];
        for f in g.world.body(body).fixtures.clone() {
            let fx = g.world.fixture(f);
            let b = fx.shape.compute_aabb(&g.world.body(fx.body).xf, 0);
            points.push([(b.lower.x + b.upper.x) * 0.5, (b.lower.y + b.upper.y) * 0.5, 0.0]);
        }
        points.into_iter().filter(|&q| !g.overlap_point(q, mask, -0.9, 0.9).is_empty()).collect()
    }

    /// Drags a liquid conduit with the BloodWireTool from `from` to `to`.
    fn lay_wire(g: &mut Game, from: usize, to: usize) {
        lay_with(g, "BloodWireTool", from, to);
    }

    /// Two wooden boards beside each other, and a wire of `tool` from the first to the second.
    fn wired(tool: &str) -> Option<(Game, usize, usize)> {
        let (mut g, a) = spawned("Stick")?;
        let at = g.physicals[a].transform.position;
        let b = spawn_at(&mut g, "Stick", [at[0] + 1.5, at[1], 0.0]);
        frames(&mut g, 30);
        lay_with(&mut g, tool, a, b);
        Some((g, a, b))
    }

    #[test]
    fn every_wire_tool_lays_its_own_wire() {
        use crate::game::blood_wire::WireKind;
        let tools = [
            ("CableWireTool", WireKind::Cable, "cable", 0.05, 12, true),
            ("RopeTool", WireKind::Rope, "rope", 0.05, 12, true),
            ("ChainWireTool", WireKind::Chain, "chain", 0.1, 12, true),
            ("CopperWireTool", WireKind::Copper, crate::il!(str "CopperWireTool::OnJointCreate" 0), 0.05, 12, true),
            ("EnergyWireTool", WireKind::Energy, crate::il!(str "EnergyWireTool::OnJointCreate" 0), 0.05, 12, true),
            ("HeatPipeTool", WireKind::HeatPipe, crate::il!(str "HeatPipeTool::OnJointCreate" 0), 0.1, 1, false),
            ("RigidWireTool", WireKind::Rigid, crate::il!(str "RigidWireTool::OnJointCreate" 0), 0.08, 1, false),
        ];
        for (tool, kind, name, width, vertices, max_only) in tools {
            let Some((g, a, b)) = wired(tool) else { return };
            assert_eq!(g.blood_wires.len(), 1, "{tool}");
            let w = &g.blood_wires[0];
            assert!(w.started && w.kind == kind && w.WireWidth == width && kind.vertex_count() == vertices, "{tool}");
            assert_eq!((w.physical, w.other), (a, Some(b)));
            assert_eq!(g.undo.history.last().map(|x| x.display_name()), Some(name));
            let j = &g.fixed_joints[w.joint];
            assert!(j.joint.is_some() && j.state.enable_collision, "{tool}");
            assert_eq!(j.distance.as_ref().unwrap().max_distance_only, max_only, "{tool}");
        }
    }

    #[test]
    fn fixed_wires_weld_and_bindings_burn_through() {
        for (tool, strength) in [("FixedCableTool", f32::INFINITY), ("SteelBindingTool", 6000.0f32), ("WoodenBindingTool", 4000.0)] {
            let Some((mut g, a, b)) = wired(tool) else { return };
            frames(&mut g, 2);
            let f = g.blood_wires[0].joint;
            let j = &g.fixed_joints[f];
            assert!(j.joint.is_some() && !j.state.enable_collision && j.distance.is_none(), "{tool}");
            assert_eq!(g.blood_wires[0].other, Some(b));
            let mass = mathf::max(g.physicals[a].rigidbody.mass(&g.world), g.physicals[b].rigidbody.mass(&g.world));
            let force = strength * (mass / 5.0);
            assert_eq!((j.state.break_force, j.state.break_torque), (force, force), "{tool}");
            if strength.is_finite() {
                g.physicals[b].BurnProgress = 0.5;
                frames(&mut g, 2);
                assert_eq!(g.fixed_joints[f].state.break_force, force * 0.5);
                g.blood_wire_slice(a);
                frames(&mut g, 3);
                assert!(g.destroyed_components.contains(&g.blood_wires[0].id), "{tool} still there");
            }
        }
    }

    #[test]
    fn a_wood_strut_keeps_its_length_until_it_snaps() {
        let Some((mut g, a, b)) = wired("WoodStrutTool") else { return };
        frames(&mut g, 2);
        let f = g.blood_wires[0].joint;
        assert!(!g.fixed_joints[f].distance.as_ref().unwrap().max_distance_only);
        let mass = mathf::max(g.physicals[a].rigidbody.mass(&g.world), g.physicals[b].rigidbody.mass(&g.world));
        assert_eq!(g.fixed_joints[f].state.break_force, 700.0 * mass);
        g.blood_wire_slice(a);
        frames(&mut g, 3);
        assert!(g.fixed_joints[f].destroyed);
    }

    #[test]
    fn rope_and_copper_break_by_the_mass_and_burn_through() {
        for (tool, strength) in [("RopeTool", 350.0f32), ("CopperWireTool", 250.0)] {
            let Some((mut g, a, b)) = wired(tool) else { return };
            let w = 0;
            let f = g.blood_wires[w].joint;
            let mass = mathf::max(g.physicals[a].rigidbody.mass(&g.world), g.physicals[b].rigidbody.mass(&g.world));
            let force = strength * mathf::lerp(mass, 1.0, 0.5);
            assert_eq!(g.blood_wires[w].breakForceBuffer, force);
            assert_eq!(g.fixed_joints[f].state.break_force, force);
            g.physicals[b].BurnProgress = 0.5;
            frames(&mut g, 2);
            assert_eq!(g.fixed_joints[f].state.break_force, force * 0.5);
            // Slice only clears the buffer: the next Tick sets a break force of 0 and the joint goes.
            let draws = g.random.clone();
            g.blood_wire_slice(a);
            assert_eq!(g.fixed_joints[f].state.break_force, force * 0.5);
            frames(&mut g, 3);
            assert!(g.destroyed_components.contains(&g.blood_wires[w].id), "{tool} still there");
            assert!(g.fixed_joints[f].destroyed);
            assert_ne!(format!("{draws:?}"), format!("{:?}", g.random), "JointBroken's two draws");
        }
    }

    #[test]
    fn energy_wire_carries_charge_and_flickers_cyan() {
        let Some((mut g, a, b)) = wired("EnergyWireTool") else { return };
        g.physicals[a].charge = 2.0;
        frames(&mut g, 2);
        assert!(g.physicals[b].charge > 0.5, "{}", g.physicals[b].charge);
        let w = &g.blood_wires[0];
        assert_ne!(w.start_color, w.WireColor);
        assert_ne!(w.start_color, w.end_color);
        g.physicals[a].charge = 0.0;
        g.physicals[b].charge = 0.0;
        frames(&mut g, 2);
        assert_eq!(g.blood_wires[0].start_color, g.blood_wires[0].WireColor);
    }

    #[test]
    fn heat_pipe_evens_the_temperature_and_cools_to_the_air() {
        let Some((mut g, a, b)) = wired("HeatPipeTool") else { return };
        g.physicals[a].SimulateTemperature = true;
        g.physicals[b].SimulateTemperature = true;
        g.physicals[a].Temperature = 500.0;
        frames(&mut g, 20);
        let (ta, tb) = (g.physicals[a].Temperature, g.physicals[b].Temperature);
        assert!(ta < 500.0 && tb > 25.0, "{ta} {tb}");
        assert!(g.blood_wires[0].fakeCableTemperature > g.AmbientTemperature);
    }

    #[test]
    fn a_wire_may_end_in_the_air_but_a_conduit_may_not() {
        let Some((mut g, a)) = spawned("Stick") else { return };
        let at = g.physicals[a].transform.position;
        lay_to_air(&mut g, "CableWireTool", a, [at[0] + 2.0, at[1] + 2.0, 0.0]);
        assert_eq!(g.blood_wires.len(), 1);
        assert!(g.fixed_joints[g.blood_wires[0].joint].connected.is_none());
        assert_eq!(g.blood_wires[0].other, None);
        lay_to_air(&mut g, "BloodWireTool", a, [at[0] + 2.0, at[1] + 2.0, 0.0]);
        assert_eq!(g.blood_wires.len(), 1, "a liquid conduit needs both ends");
    }

    #[test]
    fn springs_pull_back_and_snap_when_sliced() {
        use crate::game::blood_wire::WireKind;
        for (tool, kind, name, frequency, breaks) in [
            ("SpringWireTool", WireKind::Spring, crate::il!(str "DestructibleSpringWireTool::OnJointCreate" 0), 2.0, false),
            ("StrongSpringWireTool", WireKind::StrongSpring, crate::il!(str "StrongSpringWireTool::OnJointCreate" 0), 8.0, false),
            ("DestructibleSpringWireTool", WireKind::DestructibleSpring, crate::il!(str "DestructibleSpringWireTool::OnJointCreate" 0), 2.0, true),
        ] {
            let Some((mut g, a, b)) = wired(tool) else { return };
            let w = &g.blood_wires[0];
            let f = w.joint;
            assert!(w.started && w.kind == kind && w.WireWidth == 0.2, "{tool}");
            assert_eq!(g.undo.history.last().map(|x| x.display_name()), Some(name));
            let j = &g.fixed_joints[f];
            let spring = j.spring.as_ref().unwrap();
            assert!(!spring.auto_configure_distance && j.joint.is_some() && j.state.enable_collision);
            assert_eq!(spring.frequency, frequency, "{tool}");
            assert_eq!(w.vertex_count, (spring.distance as f64).ceil().max(2.0) as usize - 1);
            assert_eq!(w.vertices.len(), w.vertex_count + 1);
            assert_eq!(g.fixed_joints[f].state.break_force.is_infinite(), !breaks, "{tool}");
            g.physicals[b].BurnProgress = 0.5;
            let before = g.fixed_joints[f].state.break_force;
            frames(&mut g, 2);
            assert_eq!(g.fixed_joints[f].state.break_force, if breaks { before * 0.5 } else { before }, "only a finite spring burns");
            g.blood_wire_slice(a);
            frames(&mut g, 3);
            assert!(g.fixed_joints[f].destroyed, "{tool} survived the slice");
        }
    }

    #[test]
    fn bandage_stops_bleeding_heals_and_soaks() {
        let Some((mut g, _)) = spawned("Human") else { return };
        let l = 0;
        let p = g.limbs[l].PhysicalBehaviour;
        let person = g.limbs[l].Person;
        let adrenaline = g.people[person].AdrenalineLevel;
        g.limbs[l].Health = 1.0;
        lay_with(&mut g, "BandageWireTool", p, p);
        assert_eq!(g.blood_wires.len(), 1);
        let w = &g.blood_wires[0];
        assert_eq!(g.undo.history.last().map(|x| x.display_name()), Some("bandage"));
        assert_eq!(w.limb, Some(l));
        assert_eq!(w.other, Some(p), "the bandage's end goes back on the start limb");
        assert!(g.fixed_joints[w.joint].joint.is_none(), "Unity refuses a joint from a body to itself");
        assert_eq!(g.fixed_joints[w.joint].spring.as_ref().unwrap().frequency, 3.0);
        assert!(g.people[person].AdrenalineLevel >= adrenaline + 0.2, "{}", g.people[person].AdrenalineLevel);
        g.circulations[l].BleedingRate = 4.0;
        frames(&mut g, 10);
        let (rate, health, soak) = (g.circulations[l].BleedingRate, g.limbs[l].Health, g.blood_wires[0].SoakAmount);
        assert!(rate < 2.0, "{rate}");
        assert!(health > 1.0, "{health}");
        // 1.28.45: SoakAmount is Obsolete (never grows); SoakColour moves toward the blood instead.
        if crate::assets::il::is_1_28_45() {
            assert_eq!(soak, 0.0);
            assert_ne!(g.blood_wires[0].SoakColour, [1.0; 4]);
        } else {
            assert!(soak > 0.0, "{soak}");
        }
        assert_ne!(g.blood_wires[0].start_color, [1.0; 4]);
        // A bandage's FixedUpdate hides the Tick: a slice does nothing.
        g.blood_wire_slice(p);
        frames(&mut g, 3);
        assert!(!g.destroyed_components.contains(&g.blood_wires[0].id));
    }

    #[test]
    fn a_pin_holds_a_limb_until_it_is_deleted() {
        let Some((mut g, first)) = spawned("Human") else { return };
        frames(&mut g, 2);
        let at = g.physicals[first].transform.position;
        g.input.mouse = [at[0], at[1], 0.0];
        g.set_tool_by_type_name("WoodPinTool");
        frames(&mut g, 2);
        ctl(&mut g, "drag", true, true, false);
        frames(&mut g, 1);
        ctl(&mut g, "drag", false, true, false);
        frames(&mut g, 1);
        ctl(&mut g, "drag", false, false, true);
        frames(&mut g, 1);
        assert_eq!(g.pins.len(), 1);
        assert_eq!(g.undo.history.last().map(|x| x.display_name()), Some("pin"));
        let f = g.pins[0].Joint.unwrap();
        let id = g.fixed_joints[f].joint.unwrap();
        assert!(g.fixed_joints[f].state.break_force.is_finite(), "a wood pin breaks");
        assert!(matches!(g.world.joints[id].as_ref().unwrap().kind, crate::b2::world::JointKind::Revolute(_)));
        let r = g.pins[0].renderer.unwrap();
        frames(&mut g, 60);
        let [x, y, ..] = g.renderers[r].chain[0].t;
        assert!((x - at[0]).abs() < 0.3 && (y - at[1]).abs() < 0.3, "the pin stays put: {x},{y} from {at:?}");
        ctl(&mut g, "delete", false, true, false);
        g.input.mouse = [x, y, 0.0];
        frames(&mut g, 3);
        assert!(g.destroyed_components.contains(&g.pins[0].id));
        assert!(g.fixed_joints[f].joint.is_none() && !g.renderers[r].visible);
    }

    #[test]
    fn powers_move_a_human_and_follow_the_button() {
        let Some((mut g, first)) = spawned("Human") else { return };
        frames(&mut g, 2);
        let start = g.physicals[first].transform.position;
        g.input.mouse = [start[0], start[1] + 3.0, 0.0];
        g.set_tool_by_type_name("LiftPowerTool");
        assert!(g.powers[0].playing, "the effect plays on awake");
        frames(&mut g, 2);
        ctl(&mut g, "drag", true, true, false);
        frames(&mut g, 1);
        ctl(&mut g, "drag", false, true, false);
        frames(&mut g, 30);
        assert!(g.physicals[first].transform.position[1] > start[1] + 0.5);
        ctl(&mut g, "drag", false, false, true);
        frames(&mut g, 1);
        assert!(!g.powers[0].playing);
        let added = g.powers.len();
        g.set_tool_by_type_name("LiftPowerTool");
        assert_eq!(g.powers.len(), added, "a tool is added once");
        g.set_tool_by_type_name("DragTool");
        assert!(g.power.is_none() && g.tool.as_ref().unwrap().isActive);
    }

    #[test]
    fn lightning_charges_under_the_mouse_and_a_flick_casts_a_bolt() {
        let Some((mut g, first)) = spawned("Human") else { return };
        frames(&mut g, 2);
        let at = g.physicals[first].transform.position;
        let screen = |m: [f32; 3]| Some(super::Screen { mouse: [m[0] * 100.0, m[1] * 100.0], origin: [0.0; 2], scale: 0.01, view: None });
        g.input.mouse = at;
        g.input.screen = screen(at);
        g.set_tool_by_type_name("LightningTool");
        frames(&mut g, 2);
        ctl(&mut g, "drag", true, true, false);
        frames(&mut g, 1);
        ctl(&mut g, "drag", false, true, false);
        frames(&mut g, 10);
        assert!(g.physicals[first].charge > 0.0, "the entity charges what is under the mouse");
        // A flick down from above the body: the bolt starts its cooldown.
        g.input.mouse = [at[0], at[1] + 2.0, 0.0];
        frames(&mut g, 2);
        g.input.screen = screen([at[0], at[1] + 2.0, 0.0]);
        frames(&mut g, 1);
        g.input.screen = screen([at[0], at[1] - 1.0, 0.0]);
        frames(&mut g, 1);
        assert!(g.lightning_entities[0].cooldown > 0.0, "the flick casts a bolt");
        ctl(&mut g, "drag", false, false, true);
        frames(&mut g, 30);
        assert!(!g.lightning_entities[0].isHeld);
        g.set_tool_by_type_name("DragTool");
        g.set_tool_by_type_name("LightningTool");
        assert_eq!(g.lightning_entities.len(), 1, "a tool is added once");
    }

    #[test]
    fn pyro_sets_fire_under_the_mouse_and_a_sweep_blows_it_away() {
        let Some((mut g, first)) = spawned("Human") else { return };
        frames(&mut g, 2);
        let at = g.physicals[first].transform.position;
        g.input.mouse = at;
        g.set_tool_by_type_name("PyroPowerTool");
        frames(&mut g, 2);
        ctl(&mut g, "drag", true, true, false);
        frames(&mut g, 1);
        ctl(&mut g, "drag", false, true, false);
        frames(&mut g, 5);
        assert!(g.physicals[first].OnFire, "the entity ignites what is under the mouse");
        // A fast sweep from left of the body: the cone pushes it right.
        g.input.mouse = [at[0] - 4.0, at[1], 0.0];
        frames(&mut g, 3);
        g.input.mouse = at;
        frames(&mut g, 3);
        assert!(g.physicals[first].transform.position[0] > at[0] + 0.5, "the sweep blows the body along");
        ctl(&mut g, "drag", false, false, true);
        frames(&mut g, 2);
        g.set_tool_by_type_name("DragTool");
        assert!(!g.pyro_entities[0].IsCurrentTool && !g.pyro_entities[0].isHeld);
        g.set_tool_by_type_name("PyroPowerTool");
        assert_eq!(g.pyro_entities.len(), 1, "a tool is added once");
    }

    #[test]
    fn serpentine_belt_couples_spins_and_goes_with_its_other_end() {
        let Some((mut g, a, b)) = wired("SerpentineBeltTool") else { return };
        frames(&mut g, 2);
        assert_eq!(g.belts.len(), 1);
        assert!(g.belts[0].started && g.belts[0].Self_ == a && g.belts[0].Other == b);
        assert!((g.belts[0].gearRatio - 1.0).abs() < 0.05, "two equal boards: {} {} {}", g.belts[0].gearRatio, g.physicals[a].Circumference, g.physicals[b].Circumference);
        g.physicals[a].rigidbody.set_angular_velocity(&mut g.world, 360.0);
        frames(&mut g, 3);
        let (wa, wb) = (g.physicals[a].rigidbody.angular_velocity(&g.world), g.physicals[b].rigidbody.angular_velocity(&g.world));
        assert!(wb > 10.0 && (wa - wb).abs() < 2.0, "the belt shares the spin: {wa} {wb}");
        let id = g.belts[0].id;
        let other = g.physicals[b].id;
        g.destroy_object(other);
        frames(&mut g, 3);
        assert!(g.destroyed_components.contains(&id), "the belt goes with its other end");
    }

    /// `from` wired to a metronome with `tool`, and the metronome's index.
    fn use_wired(from: &str, tool: &str) -> Option<(Game, usize, usize, usize)> {
        let (mut g, a) = spawned(from)?;
        let at = g.physicals[a].transform.position;
        let m = g.metronomes.len();
        let b = spawn_at(&mut g, "Metronome", [at[0] + 1.5, at[1], 0.0]);
        frames(&mut g, 30);
        lay_with(&mut g, tool, a, b);
        g.set_tool_by_type_name("DragTool");
        frames(&mut g, 2);
        Some((g, a, b, m))
    }

    #[test]
    fn a_use_wire_passes_an_activation_on_after_the_delay() {
        use crate::game::blood_wire::WireKind;
        let Some((mut g, a, b, m)) = use_wired("Stick", "RedUseWireTool") else { return };
        let w = g.blood_wires.iter().position(|w| w.kind == WireKind::Use(1, false)).expect("a red use wire");
        assert!(g.blood_wires[w].started && g.blood_wires[w].physical == a && g.blood_wires[w].target == Some(b));
        assert!(g.fixed_joints[g.blood_wires[w].joint].state.break_force.is_infinite());
        assert_eq!(g.undo.history.last().map(|x| x.display_name()), Some(crate::il!(str "UseWireTool::CreateWire" 0)));
        let n = g.assets.as_ref().unwrap().resources_in(crate::il!(str "NewUseWireBehaviour::JointBroken" 0), 83).unwrap().len();
        assert_eq!(n, crate::game::use_wire::USE_WIRE_BREAK_CLIPS);
        let before = g.metronomes[m].Activated;
        g.activate_direct(a);
        frames(&mut g, 3);
        assert_eq!(g.metronomes[m].Activated, before, "not before ActivationPropagationDelay (0.1 s)");
        frames(&mut g, 6);
        assert_ne!(g.metronomes[m].Activated, before, "the wire passed it on");
        // A non-direct signal on another channel stops at the wire.
        let root = g.root_id(a);
        let green = g.signal_from(root, 0);
        g.broadcast_use(a, &green);
        frames(&mut g, 10);
        assert_ne!(g.metronomes[m].Activated, before);
        // A held activation (DragTool.OnContinuousUpdate's direct signal): it arrives on the wire's channel.
        let mut seen = false;
        for _ in 0..30 {
            let held = g.signal_direct(true, 0);
            g.broadcast_use_continuous(a, &held);
            frames(&mut g, 1);
            seen |= g.used_continuously_on(b, 1);
            assert!(!g.used_continuously_on(b, 0));
        }
        assert!(seen, "UseContinuous on channel 1 reached the metronome");
    }

    #[test]
    fn a_charged_activation_transformer_activates_through_its_wire() {
        let Some((mut g, a, b, m)) = use_wired("Activation Transformer", "UseWireTool") else { return };
        let t = g.activation_transformers.iter().position(|x| x.physical == a).expect("the transformer");
        let before = g.metronomes[m].Activated;
        let mut seen = false;
        for _ in 0..30 {
            g.physicals[a].charge = 10.0;
            frames(&mut g, 1);
            seen |= g.used_continuously_on(b, 0);
        }
        assert!(g.activation_transformers[t].shouldSend);
        assert_ne!(g.metronomes[m].Activated, before, "the rise sent Use once");
        assert!(seen, "UseContinuous on the wire's channel while charged");
    }

    #[test]
    fn a_human_limbs_context_menu_poses_and_breaks_bones() {
        let Some((mut g, first)) = spawned("Human") else { return };
        let l = g.limbs.iter().position(|x| x.PhysicalBehaviour >= first && g.physicals[x.PhysicalBehaviour].name == "UpperBody").expect("UpperBody");
        let p = g.limbs[l].PhysicalBehaviour;
        let mut open = |g: &mut Game| {
            let mut found = None;
            for q in point_on(g, p) {
                g.input.mouse = q;
                frames(g, 1);
                if g.tool.as_ref().unwrap().hover == Some(p) {
                    found = Some(q);
                    break;
                }
            }
            assert!(found.is_some(), "no point over UpperBody");
            ctl(g, "context", true, true, false);
            frames(g, 1);
            ctl(g, "context", false, false, false);
            g.context_menu.take().expect("the menu opened")
        };
        let ids = |b: &[crate::game::menu::ShownButton]| b.iter().map(|x| x.Identity).collect::<Vec<_>>();
        let menu = open(&mut g);
        assert_eq!(g.tool.as_ref().unwrap().rawSelection, vec![p], "the right-click selected the limb");
        assert_eq!(ids(&menu), ["setTemperature", "setAngle", "inspectLimb", "breakBones", "startStumbling", "startWalking", "startProtect", "startSit", "startPetrified"]);
        let walk = menu.iter().find(|b| b.Identity == "startWalking").unwrap();
        g.context_menu_click(&walk.actions);
        assert_eq!(g.people[g.limbs[l].Person].OverridePoseIndex, 6);
        let menu = open(&mut g);
        assert!(ids(&menu).contains(&"clearOverride") && !ids(&menu).contains(&"startWalking"));
        let bone = menu.iter().find(|b| b.Identity == "breakBones").unwrap();
        assert_eq!(bone.label, crate::il_v!(str "LimbBehaviour::<CreateContextMenuOptions>b__100_1" 0, "LimbBehaviour::<CreateContextMenuOptions>b__102_1" 0));
        g.context_menu_click(&bone.actions);
        assert!(g.limbs[l].Broken);
        assert_eq!(open(&mut g).iter().find(|b| b.Identity == "breakBones").unwrap().label, crate::il_v!(str "LimbBehaviour::<CreateContextMenuOptions>b__100_1" 1, "LimbBehaviour::<CreateContextMenuOptions>b__102_1" 1));
    }

    #[test]
    fn a_destructible_use_wire_can_break() {
        let Some((g, ..)) = use_wired("Stick", "DestructibleBlueUseWireTool") else { return };
        let w = g.blood_wires.last().unwrap();
        assert!(w.CanBreak && w.Channel == 2 && w.breakForceBuffer.is_finite());
    }

    #[test]
    fn delete_held_over_a_serpentine_belt_removes_it() {
        let Some((mut g, _, _)) = wired("SerpentineBeltTool") else { return };
        g.set_tool_by_type_name("DragTool");
        frames(&mut g, 2);
        let hull = g.belts[0].hull.clone().expect("UpdateBelt ran");
        assert!(hull.len() >= 4 && hull.first() == hull.last(), "a closed hull: {hull:?}");
        let id = g.belts[0].id;
        // Nothing selected (delete would remove the boards); away from the belt nothing happens.
        g.tool.as_mut().unwrap().rawSelection.clear();
        g.input.mouse = [hull[0][0] + 5.0, hull[0][1] + 5.0, 0.0];
        frames(&mut g, 1);
        ctl(&mut g, "delete", false, true, false);
        frames(&mut g, 2);
        assert!(!g.destroyed_components.contains(&id), "delete away from the belt");
        let m = [(hull[0][0] + hull[1][0]) / 2.0, (hull[0][1] + hull[1][1]) / 2.0];
        g.input.mouse = [m[0], m[1], 0.0];
        frames(&mut g, 2);
        assert!(g.destroyed_components.contains(&id), "delete over the belt destroys it");
    }

    /// Two sticks joined by a tool, packed with ObjectStateConverter.Convert, pasted and packed again: the joining component comes back
    /// with the same stored members, a joint built from them on the pasted pair, and the same prototype bar the identities.
    #[test]
    fn tool_components_round_trip_through_object_states() {
        use crate::game::save::MonoBehaviourPrototype;
        for tool in ["RopeTool", "FixedCableTool", "WoodStrutTool", "SpringWireTool", "SteelBindingTool", "EnergyWireTool", "PhaseLinkTool"] {
            let Some((mut g, a, b)) = wired(tool) else { return };
            frames(&mut g, 5);
            let at = g.physicals[a].transform.position;
            let states = g.object_states_of(&[a, b], [0.0; 3]);
            let joined = |s: &[crate::game::save::ObjectState]| -> Vec<MonoBehaviourPrototype> {
                let mut v: Vec<MonoBehaviourPrototype> = s.iter().flat_map(|x| x.RootComponentData.iter()).filter(|m| crate::game::save::is_tool_component(&m.Type)).cloned().collect();
                v.iter_mut().for_each(|m| m.storedReferenceFields.iter_mut().for_each(|(_, l)| l.UniqueIdentity = 0));
                v
            };
            let first = joined(&states);
            assert_eq!(first.len(), 1, "{tool}: {:?}", states[0].RootComponentData.iter().map(|m| &m.Type).collect::<Vec<_>>());
            let refs: Vec<_> = first[0].storedReferenceFields.iter().map(|(n, _)| n.as_str()).collect();
            assert!(refs.contains(&"otherPhysicalBehaviour") || refs.contains(&"Other"), "{tool}: {refs:?}");
            let (wires, links, count) = (g.blood_wires.len(), g.phase_links.len(), g.physicals.len());
            g.paste_states(&states, [at[0] + 3.0, at[1] + 2.0, 0.0], false, "Paste");
            frames(&mut g, 3);
            let pasted: Vec<usize> = (count..g.physicals.len()).filter(|&p| g.object_names.contains_key(&g.root_id(p))).collect();
            assert_eq!(pasted.len(), 2, "{tool}");
            if tool == "PhaseLinkTool" {
                assert_eq!(g.phase_links.len(), links + 1);
                let l = &g.phase_links[links];
                assert!(l.started && l.Other == Some(pasted[1]) && l.LocalOffset == g.phase_links[0].LocalOffset, "{tool}");
            } else {
                assert_eq!(g.blood_wires.len(), wires + 1, "{tool}");
                let (w0, w1) = (&g.blood_wires[0], &g.blood_wires[wires]);
                assert!(w1.started && w1.kind == w0.kind && w1.physical == pasted[0] && w1.other == Some(pasted[1]), "{tool}");
                let (j0, j1) = (&g.fixed_joints[w0.joint].state, &g.fixed_joints[w1.joint].state);
                // A fixed wire's joint auto-configures its connected anchor from the pasted pose (autoConfigureConnectedAnchor = true).
                assert!(g.fixed_joints[w1.joint].joint.is_some() && j0.anchor == j1.anchor && (w0.kind.is_fixed() || j0.connected_anchor == j1.connected_anchor), "{tool}");
            }
            let again = g.object_states_of(&pasted, [0.0; 3]);
            assert_eq!(joined(&again), first, "{tool}");
        }
    }

    #[test]
    fn phase_link_ignores_collisions_until_it_is_destroyed() {
        let Some((mut g, a, b)) = wired("PhaseLinkTool") else { return };
        frames(&mut g, 2);
        assert_eq!(g.phase_links.len(), 1);
        assert!(g.phase_links[0].started && g.phase_links[0].physical == a && g.phase_links[0].Other == Some(b));
        let pairs = g.ignore_stack.len();
        assert!(pairs > 0, "the boards' colliders ignore each other");
        let id = g.phase_links[0].id;
        g.tr_destroy_component(id, 0.0);
        frames(&mut g, 2);
        assert!(g.destroyed_components.contains(&id));
        assert_eq!(g.ignore_stack.len(), 0, "OnDestroy stops ignoring");
        // A second link; the other board's object goes, and the link destroys itself.
        g.set_tool_by_type_name("DragTool");
        lay_with(&mut g, "PhaseLinkTool", a, b);
        frames(&mut g, 2);
        let id = g.phase_links[1].id;
        let other = g.physicals[b].id;
        g.destroy_object(other);
        frames(&mut g, 3);
        assert!(g.destroyed_components.contains(&id), "the link goes with its target");
    }

    fn lay_to_air(g: &mut Game, tool: &str, from: usize, to: [f32; 3]) {
        g.set_tool_by_type_name(tool);
        let start = point_on(g, from).into_iter().find(|&q| {
            g.input.mouse = q;
            frames(g, 1);
            g.tool.as_ref().unwrap().hover == Some(from)
        });
        assert!(start.is_some());
        ctl(g, "drag", true, true, false);
        frames(g, 2);
        ctl(g, "drag", false, true, false);
        g.input.mouse = to;
        frames(g, 2);
        ctl(g, "drag", false, false, true);
        frames(g, 2);
    }

    fn lay_with(g: &mut Game, tool: &str, from: usize, to: usize) {
        g.set_tool_by_type_name(tool);
        let mut start = None;
        for q in point_on(g, from) {
            g.input.mouse = q;
            frames(g, 1);
            if g.tool.as_ref().unwrap().hover == Some(from) {
                start = Some(q);
                break;
            }
        }
        assert!(start.is_some(), "no point where the mouse hovers the start object");
        ctl(g, "drag", true, true, false);
        frames(g, 2);
        ctl(g, "drag", false, true, false);
        let mut end = None;
        for q in point_on(g, to) {
            g.input.mouse = q;
            frames(g, 2);
            if g.tool.as_ref().unwrap().hover == Some(to) {
                end = Some(q);
                break;
            }
        }
        assert!(end.is_some(), "no point where the mouse hovers the end object");
        ctl(g, "drag", false, false, true);
        frames(g, 2);
    }

    #[test]
    fn bypass_wire_pumps_a_human_whose_heart_stopped() {
        let Some((mut g, machine)) = spawned("Cardiopulmonary Bypass Machine") else { return };
        let at = g.physicals[machine].transform.position;
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        g.input.mouse = [at[0] + 3.0, at[1] + 0.5, 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        ctl(&mut g, "spawnRight", false, false, false);
        frames(&mut g, 300);
        // The heart stops: the pump limb no longer beats and nothing flows.
        let heart = (first_limb..g.limbs.len()).find(|&l| g.circulations[l].WasInitiallyPumping).expect("a pumping limb");
        for l in first_limb..g.limbs.len() {
            g.circulations[l].IsPump = false;
            g.circulations[l].BloodFlow = 0.0;
        }
        let heart_physical = g.limbs[heart].PhysicalBehaviour;
        lay_wire(&mut g, machine, heart_physical);
        assert_eq!(g.blood_wires.len(), 1);
        assert_eq!(g.undo.history.last().map(|a| a.display_name()), Some(crate::il!(str "BloodWireTool::OnJointCreate" 0)));
        let w = &g.blood_wires[0];
        assert!(w.IsCardiopulmonaryBypassWire && !w.IsHeartRateWire && w.started, "{w:?}");
        assert_eq!(g.cardiopulmonary_bypasses[0].ValidConnectionCount, 1);
        // The machine is off: no pumping.
        frames(&mut g, 60);
        assert_eq!(g.circulations[heart].ArtificialHeartbeat, 0.0);
        assert!(g.circulations[heart].BloodFlow < 0.5, "{}", g.circulations[heart].BloodFlow);
        // On: the wire sets the heartbeat and keeps the blood flowing.
        { let a = g.signal_direct(true, 0); g.broadcast_use(machine, &a); }
        assert!(g.cardiopulmonary_bypasses[0].Activated);
        frames(&mut g, 120);
        assert_eq!(g.circulations[heart].ArtificialHeartbeat, g.cardiopulmonary_bypasses[0].TargetHeartrate);
        assert!(g.circulations[heart].BloodFlow > 0.5, "{}", g.circulations[heart].BloodFlow);
        assert!(g.people[g.limbs[heart].Person].OxygenLevel >= 0.5);
        // Undo destroys the wire: OnDestroy stops the heartbeat and drops the connection.
        ctl(&mut g, "undo", false, false, true);
        frames(&mut g, 3);
        assert!(g.destroyed_components.contains(&g.blood_wires[0].id));
        assert!(g.fixed_joints[g.blood_wires[0].joint].destroyed);
        assert_eq!(g.circulations[heart].ArtificialHeartbeat, 0.0);
        assert_eq!(g.cardiopulmonary_bypasses[0].ValidConnectionCount, 0);
        assert!(g.undo.history.iter().all(|a| a.display_name() != crate::il!(str "BloodWireTool::OnJointCreate" 0) && a.display_name() != "LIQUID CONDUIT" || a.relevant_objects().is_empty()));
    }

    #[test]
    fn defibrillator_restarts_the_circulation_under_its_pads() {
        let Some((mut g, _)) = spawned("Defibrillator") else { return };
        let (pad, layer) = (g.defibrillators[0].Pad1, g.defibrillators[0].PadLayer);
        let at = g.transform_point3(pad, [-0.1, 0.0, 0.0]);
        // A human spawned on the pad.
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        g.input.mouse = [at[0], at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        let at = g.transform_point3(pad, [-0.1, 0.0, 0.0]);
        for l in first_limb..g.limbs.len() {
            g.circulations[l].IsPump = false;
            g.circulations[l].BloodFlow = 0.0;
            g.people[g.limbs[l].Person].Consciousness = 0.2;
        }
        let charge = g.physicals[pad].Charge();
        g.defibrillate(0, pad, layer);
        assert_eq!(g.physicals[pad].Charge(), charge + 0.2);
        let hit: Vec<usize> = (first_limb..g.limbs.len()).filter(|&l| g.circulations[l].BloodFlow == 1.0).collect();
        assert!(!hit.is_empty(), "no limb under the pad at {at:?}");
        for l in hit {
            assert_eq!(g.circulations[l].IsPump, g.circulations[l].WasInitiallyPumping);
            assert_eq!(g.people[g.limbs[l].Person].Consciousness, 1.0);
            assert!(g.people[g.limbs[l].Person].AdrenalineLevel >= 20.0);
        }
    }

    #[test]
    fn running_chainsaw_saws_a_human_on_its_blade() {
        let Some((mut g, _)) = spawned("Chainsaw") else { return };
        let saw = g.chainsaws[0].physical;
        let at = g.transform_point3(saw, [0.2, 0.0, 0.0]);
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        g.input.mouse = [at[0], at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no human spawned");
        let health: f32 = (first_limb..g.limbs.len()).map(|l| g.limbs[l].Health).sum();
        let (mut connected, mut joints) = (false, 0);
        for _ in 0..120 {
            g.chainsaws[0].rotationSpeed = 10.0;
            frames(&mut g, 1);
            connected |= !g.chainsaws[0].softConnections.keys().is_empty();
            joints = joints.max(g.fixed_joints.iter().filter(|f| f.friction.is_some() && !f.destroyed).count());
        }
        let after: f32 = (first_limb..g.limbs.len()).map(|l| g.limbs[l].Health).sum();
        assert!(connected && joints > 0, "the blade never soft-connected a limb");
        assert!(after < health, "no limb took damage ({health} -> {after})");
    }

    #[test]
    fn emp_generator_zeroes_an_android_limb() {
        let Some((mut g, _)) = spawned("EMP Generator") else { return };
        let emp = g.emps[0].physical;
        let at = g.physicals[emp].transform.position;
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Android".into());
        g.input.mouse = [at[0] + 2.0, at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no android spawned");
        frames(&mut g, 120);
        let androids: Vec<usize> = (first_limb..g.limbs.len()).filter(|&l| g.limbs[l].IsAndroid).collect();
        assert!(!androids.is_empty(), "the spawned Android has no android limbs");
        assert!(androids.iter().all(|&l| g.limbs[l].Health > 0.0), "an android limb starts with no health");
        g.activate_direct(emp);
        frames(&mut g, 1);
        assert!(androids.iter().any(|&l| g.limbs[l].Health == 0.0), "the EMP hit no android limb");
    }

    #[test]
    fn foliage_renders_in_the_games_order() {
        let Some((g, _)) = spawned("Small Bush") else { return };
        let first = g.foliages[0].id;
        let mut order: Vec<usize> = (0..g.foliages.len()).collect();
        order.sort_by_key(|&f| g.renderer_nodes.iter().position(|&n| n == g.renderers[g.foliages[f].renderer].id));
        // The OnWillRenderObject order in reference-probe/game/foliage.txt, as ids below the first leaf's.
        let ids: Vec<i32> = order.iter().map(|&f| g.foliages[f].id - first).collect();
        assert_eq!(ids, [-144, -78, -14, -40, 0, -130, -92]);
    }

    #[test]
    fn foliage_deactivating_in_one_frame_swaps_the_last_node_in() {
        let Some((mut g, _)) = spawned("Small Bush") else { return };
        let node = |g: &Game, f: usize| g.renderer_nodes.iter().position(|&n| n == g.renderers[g.foliages[f].renderer].id).unwrap();
        let mut order: Vec<usize> = (0..g.foliages.len()).collect();
        order.sort_by_key(|&f| node(&g, f));
        // RendererScene::RemoveRenderer moves the last node into the hole: the two leaves removed, in turn.
        let mut expected = g.renderer_nodes.clone();
        let mut kept = expected.clone();
        for &f in &order[..2] {
            let id = g.renderers[g.foliages[f].renderer].id;
            expected.swap_remove(expected.iter().position(|&n| n == id).unwrap());
            kept.retain(|&n| n != id);
        }
        for &f in &order[..2] {
            let id = g.foliages[f].id;
            g.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, false));
        }
        assert_eq!(g.renderer_nodes, expected);
        assert_ne!(g.renderer_nodes, kept, "the last node moved into a hole");
    }

    #[test]
    fn foliage_deactivating_in_the_will_render_window_is_removed_highest_first_at_its_end() {
        let Some((mut g, _)) = spawned("Small Bush") else { return };
        let mut order: Vec<usize> = (0..g.foliages.len()).collect();
        order.sort_by_key(|&f| g.renderer_nodes.iter().position(|&n| n == g.renderers[g.foliages[f].renderer].id));
        let (a, b) = (order[0], order[1]);
        let before = g.renderer_nodes.clone();
        let (ia, ib) = (g.renderer_nodes.iter().position(|&n| n == g.renderers[g.foliages[a].renderer].id).unwrap(), g.renderer_nodes.iter().position(|&n| n == g.renderers[g.foliages[b].renderer].id).unwrap());
        g.renderer_cull_begin();
        for f in [b, a] {
            let id = g.foliages[f].id;
            g.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, false));
        }
        assert_eq!(g.renderer_nodes, before, "CullScene's OnWillRenderObject window defers RemoveRenderer");
        g.renderer_cull_end();
        // ApplyPendingAddRemoveNodes: sorted, removed from the highest node down.
        let mut expected = before;
        for i in [ia.max(ib), ia.min(ib)] {
            expected.swap_remove(i);
        }
        assert_eq!(g.renderer_nodes, expected);
    }

    #[test]
    fn atom_bomb_heats_a_human_and_leaves() {
        let Some((mut g, bomb)) = spawned("Atom Bomb") else { return };
        let at = g.physicals[bomb].transform.position;
        let first_limb = g.limbs.len();
        spawn_at(&mut g, "Human", [at[0] + 6.0, at[1] + 1.0, 0.0]);
        frames(&mut g, 30);
        let limbs: Vec<usize> = (first_limb..g.limbs.len()).map(|l| g.limbs[l].PhysicalBehaviour).collect();
        assert!(limbs.iter().all(|&q| g.physicals[q].Temperature < 500.0), "the human starts hot");
        g.activate_direct(bomb);
        frames(&mut g, 2);
        assert_eq!(g.atom_explosions.len(), 1, "Use instantiated no explosion");
        assert!(g.physicals[bomb].destroyed, "the bomb outlived its Use");
        // Limbs absorb lasers: the ones in line of sight are heated to at least 500, the ones behind them are shaded.
        assert!(limbs.iter().any(|&q| g.physicals[q].Temperature >= 500.0), "no limb was heated");
        frames(&mut g, 30);
        assert!(g.atom_explosions[0].time > 0.0, "the shockwave never grew");
    }

    #[test]
    fn worm_staff_cuts_a_human() {
        let Some((mut g, _)) = spawned("Worm Staff") else { return };
        let staff = g.worm_staffs[0].physical;
        let at = g.physicals[staff].transform.position;
        let first_limb = g.limbs.len();
        spawn_at(&mut g, "Human", [at[0], at[1] + 1.0, 0.0]);
        let limbs: Vec<usize> = (first_limb..g.limbs.len()).collect();
        assert_eq!(g.worm_staffs[0].worms.len(), 3, "Start made no worms");
        let mut frame = 0;
        while frame < 1200 && limbs.iter().all(|&l| g.circulations[l].StabWoundCount == 0) {
            frames(&mut g, 1);
            frame += 1;
        }
        assert!(limbs.iter().any(|&l| g.circulations[l].StabWoundCount > 0), "no worm cut the human in 1200 frames");
    }

    #[test]
    fn tesla_coil_bolts_charge_a_human() {
        let Some((mut g, _)) = spawned("Tesla Coil") else { return };
        let coil = g.tesla_coils[0].physical;
        let at = g.physicals[coil].transform.position;
        let first_limb = g.limbs.len();
        spawn_at(&mut g, "Human", [at[0] + 2.0, at[1] + 1.0, 0.0]);
        frames(&mut g, 120);
        let limbs: Vec<usize> = (first_limb..g.limbs.len()).map(|l| g.limbs[l].PhysicalBehaviour).collect();
        assert!(g.tesla_bolts.iter().all(|b| !b.active), "the bolts are on before Use");
        g.tesla_coil_use(0);
        let mut charged = false;
        for _ in 0..60 {
            frames(&mut g, 1);
            charged |= g.tesla_bolts.iter().any(|b| b.target.is_some_and(|q| limbs.contains(&q) && g.physicals[q].charge > 0.0));
        }
        assert!(charged, "no bolt charged a limb of the human");
        g.tesla_coil_use(0);
        assert!(g.tesla_bolts.iter().all(|b| !b.active), "the bolts stay on after the second Use");
    }

    #[test]
    fn ceiling_turret_stays_frozen_in_place() {
        let Some((mut g, _)) = spawned("Ceiling Turret") else { return };
        // The turret's FreezeBehaviour is the last one added, after the Default map's.
        let turret = g.freezes.last().expect("a FreezeBehaviour").physical;
        assert_eq!(g.physicals[turret].rigidbody.bodyType, 2, "the frozen turret's body is not static");
        let at = g.physicals[turret].transform.position;
        frames(&mut g, 120);
        assert_eq!(g.physicals[turret].transform.position, at, "the frozen turret moved");
    }

    #[test]
    fn ceiling_turret_shoots_a_human_below_it() {
        let Some((mut g, _)) = spawned("Ceiling Turret") else { return };
        let firearm = g.firearms[g.ceiling_turrets.last().expect("a CeilingTurretBehaviour").firearm].physical.expect("a firearm physical");
        let at = g.physicals[firearm].transform.position;
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        g.input.mouse = [at[0], at[1] - 4.0, 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no human spawned");
        let health = |g: &Game| -> f32 { (first_limb..g.limbs.len()).map(|l| g.limbs[l].Health).sum() };
        let before = health(&g);
        let mut hit = false;
        for _ in 0..400 {
            frames(&mut g, 1);
            if health(&g) < before {
                hit = true;
                break;
            }
        }
        assert!(hit, "the turret never hit the human ({before} health)");
    }

    #[test]
    fn laser_sentry_turret_shoots_a_human_in_front() {
        let Some((mut g, _)) = spawned("Laser sentry turret") else { return };
        let laser = g.laser_turrets.last().expect("a LaserTurretBehaviour").physical;
        let at = g.physicals[laser].transform.position;
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        // The barrel points along +x: the human stands 8 units in front of it.
        g.input.mouse = [at[0] + 8.0, at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no human spawned");
        let health = |g: &Game| -> f32 { (first_limb..g.limbs.len()).map(|l| g.limbs[l].Health).sum() };
        let before = health(&g);
        let mut hit = false;
        for _ in 0..400 {
            frames(&mut g, 1);
            if health(&g) < before {
                hit = true;
                break;
            }
        }
        assert!(hit, "the laser turret never hit the human ({before} health)");
    }

    #[test]
    fn sentry_turret_shoots_a_human_in_front() {
        let Some((mut g, _)) = spawned("Sentry Turret") else { return };
        let gun = g.sentry_guns.last().expect("a SentryTurretGunBehaviour").node;
        let at = g.node_pose(gun).0;
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        // The gun's barrel points out of its node: the human stands 8 units in front of it.
        g.input.mouse = [at[0] + 8.0, at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no human spawned");
        let health = |g: &Game| -> f32 { (first_limb..g.limbs.len()).map(|l| g.limbs[l].Health).sum() };
        let before = health(&g);
        let mut hit = false;
        for _ in 0..400 {
            frames(&mut g, 1);
            if health(&g) < before {
                hit = true;
                break;
            }
        }
        assert!(hit, "the sentry turret never hit the human ({before} health)");
    }

    #[test]
    fn sentry_turret_explodes_into_debris() {
        let Some((mut g, _)) = spawned("Sentry Turret") else { return };
        let s = g.sentry_turrets.len() - 1;
        let turret = g.sentry_turrets[s].phys;
        let live = |g: &Game| g.physicals.iter().filter(|p| !p.destroyed && p.in_world).count();
        let before = live(&g);
        g.sentry_turret_explode(s);
        frames(&mut g, 2);
        assert!(g.physicals[turret].destroyed || !g.physicals[turret].in_world, "the turret was not destroyed");
        assert!(live(&g) >= before, "the debris did not replace the turret ({before} -> {})", live(&g));
        assert!(g.sentry_turrets[s].exploded, "Explode did not run");
    }

    /// A map light's OnGroup is switched on when the light activates (MapLightBehaviour.UpdateActivated).
    #[test]
    fn map_light_group_turns_on_when_its_light_activates() {
        let Some(game) = crate::game_assets::find_game(None).ok() else { return };
        let mut g = new_game(&game, "Default", None, None, 0).unwrap();
        frames(&mut g, 900);
        let light = g.map_lights.iter().position(|m| m.Activated).expect("a map light activated");
        let id = g.map_lights[light].on_group.expect("an OnGroup");
        assert!(g.with_component_instance(id, |_, inst, o| inst.objects[o].active), "the OnGroup stayed off");
    }

    #[test]
    fn sword_side_blade_soft_connects_a_human() {
        let Some((mut g, _)) = spawned("Sword") else { return };
        if crate::assets::il::is_1_28_45() {
            return sword_slashes_a_human(g);
        }
        let blade = g.sharp_on_all_sides[0].physical;
        let at = g.physicals[blade].transform.position;
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        g.input.mouse = [at[0], at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no human spawned");
        g.sharp_on_all_sides_process(0, true, 10.0);
        let keys = g.sharp_on_all_sides[0].softConnections.keys();
        assert!(!keys.is_empty(), "the blade connected nothing");
        for k in keys {
            let f = g.sharp_on_all_sides[0].softConnections.get(k).unwrap().joint;
            assert_eq!(g.fixed_joints[f].friction, Some((10.0, 10.0)));
        }
        frames(&mut g, 5);
    }

    /// 1.28.45's sword has no SharpOnAllSides: its PhysicalBehaviour.SlashColliders slash. A slash on a limb joins the
    /// two with a FrictionJoint2D, ignores the blade collider against the limb's colliders, and lives through frames.
    fn sword_slashes_a_human(mut g: Game) {
        assert!(g.sharp_on_all_sides.is_empty());
        let blade = (0..g.physicals.len()).find(|&p| g.physicals[p].SlashColliders.as_ref().is_some_and(|s| s.iter().any(|c| *c != 0))).expect("no SlashColliders");
        let collider = *g.physicals[blade].SlashColliders.as_ref().unwrap().iter().find(|c| **c != 0).unwrap();
        let at = g.physicals[blade].transform.position;
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        g.input.mouse = [at[0], at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no human spawned");
        let victim = g.limbs[first_limb].PhysicalBehaviour;
        let victim_collider = g.physicals[victim].colliders[0];
        let s = g.do_slash(blade, [at[0], at[1]], victim, collider, victim_collider);
        let slash = g.physicals[blade].SlashConnections[s].clone();
        assert_eq!(slash.Victim, victim);
        assert!(slash.Strength > 0.0);
        assert_eq!(g.fixed_joints[slash.Joint].friction, Some((2.0, 2.0)));
        // OnBeforeSerialise: the victim's identity and the strength.
        let saved = g.physical_on_before_serialise(blade);
        assert_eq!(saved.slash_strength, vec![slash.Strength]);
        assert_ne!(saved.slash_victims, vec![0]);
        frames(&mut g, 5);
    }

    #[test]
    fn destroy_disables_behaviours_before_the_delayed_destroy() {
        let Some((mut g, _)) = spawned("Chainsaw") else { return };
        let id = g.physicals[g.chainsaws[0].physical].id;
        let count = |g: &mut Game| g.with_component_instance(id, |_, inst, _| inst.fixtures.values().map(|f| f.len()).sum::<usize>());
        assert!(count(&mut g) > 0);
        g.destroy_object(id);
        assert_eq!(count(&mut g), 0, "the colliders go with the Destroy call");
        assert!(!g.destroyed_components.contains(&id), "the object itself goes in the delayed destroy");
        assert!(g.destroy_disabled.contains(&id), "OnDisable ran");
        g.run_object_destroy(id);
        assert!(g.destroyed_components.contains(&id));
        assert!(!g.destroy_disabled.contains(&id), "no second OnDisable");
    }

    #[test]
    fn decimator_deletes_a_human_in_its_trigger() {
        let Some((mut g, _)) = spawned("Decimator") else { return };
        let (id, key) = (g.decimators[0].id, g.decimators[0].DecimateTrigger);
        let fixtures = |g: &mut Game| g.with_component_instance(id, |_, inst, _| inst.fixtures.get(&key).map_or(0, |f| f.len()));
        assert_eq!(fixtures(&mut g), 0, "Start leaves the trigger disabled");
        g.decimator_use(0);
        assert!(fixtures(&mut g) > 0, "Use enables the trigger");
        let at = g.with_component_instance(id, |_, inst, _| inst.position(key.0));
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        g.input.mouse = [at[0], at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no human spawned");
        frames(&mut g, 10);
        let head = g.limbs[first_limb].PhysicalBehaviour;
        assert!(g.physicals[head].destroyed, "the human was not deleted");
        g.decimator_use(0);
        assert_eq!(fixtures(&mut g), 0, "Use again disables the trigger");
        frames(&mut g, 5);
    }

    #[test]
    fn immobility_field_holds_a_human_until_switched_off() {
        let Some((mut g, _)) = spawned("Immobility Field") else { return };
        let (id, key, p) = (g.immobility_fields[0].id, g.immobility_fields[0].Trigger, g.immobility_fields[0].physical);
        g.immobility_field_use(0);
        // The field lies beside the device: spawn between its near and far edges.
        let tid = g.with_component_instance(id, |_, inst, _| inst.objects[key.0].components[key.1]);
        let o = g.with_component_instance(id, |_, inst, _| inst.position(key.0));
        let near = g.world.collider_closest_point(tid, V::new(o[0], o[1]));
        let far = g.world.collider_closest_point(tid, V::new(2.0 * near.x - o[0] + 20.0 * (near.x - o[0]), near.y));
        let at = [(near.x + far.x) / 2.0, near.y, 0.0];
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        g.input.mouse = [at[0], at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no human spawned");
        frames(&mut g, 10);
        let held = |g: &Game| g.fixed_joints.iter().filter(|f| f.friction.is_some() && !f.destroyed && matches!(f.attached, BodyOwner::Physical(a) if a == p)).count();
        assert!(!g.immobility_fields[0].joints.keys().is_empty() && held(&g) > 0, "the field caught nothing");
        g.immobility_field_use(0);
        assert!(g.immobility_fields[0].joints.keys().is_empty(), "switching off keeps the captures");
        frames(&mut g, 2);
        assert_eq!(held(&g), 0, "switching off keeps the joints");
    }

    #[test]
    fn energy_sword_cuts_a_human_and_switches_off() {
        let Some((mut g, _)) = spawned("Energy Sword") else { return };
        let sword = g.energy_swords[0].physical;
        g.energy_sword_use(0);
        frames(&mut g, 1);
        assert!(g.with_component_instance(g.energy_swords[0].id, |g, inst, _| inst.objects[g.energy_swords[0].BladeObject].active), "the blade did not switch on");
        let line = g.energy_swords[0].LocalBladeLine;
        let mid = [(line[0][0] + line[1][0]) / 2.0, (line[0][1] + line[1][1]) / 2.0];
        let at = g.transform_point3(sword, [mid[0], mid[1], 0.0]);
        let first_limb = g.limbs.len();
        g.catalog_selected = Some("Human".into());
        g.input.mouse = [at[0], at[1], 0.0];
        ctl(&mut g, "spawnRight", false, false, true);
        frames(&mut g, 1);
        assert!(g.limbs.len() > first_limb, "no human spawned");
        let health: f32 = (first_limb..g.limbs.len()).map(|l| g.limbs[l].Health).sum();
        let mut connected = false;
        for _ in 0..60 {
            frames(&mut g, 1);
            connected |= !g.energy_swords[0].softConnections.keys().is_empty();
        }
        let after: f32 = (first_limb..g.limbs.len()).map(|l| g.limbs[l].Health).sum();
        assert!(connected, "the blade never connected a limb");
        assert!(after < health, "no limb took damage ({health} -> {after})");
        g.energy_sword_use(0);
        frames(&mut g, 2);
        assert!(g.energy_swords[0].softConnections.keys().is_empty(), "switching off left connections");
        let id = g.energy_swords[0].id;
        g.destroy_object(id);
        frames(&mut g, 2);
    }

    /// Spawns `item` at `at` and returns its first physical.
    fn spawn_at(g: &mut Game, item: &str, at: [f32; 3]) -> usize {
        let first = g.physicals.len();
        g.catalog_selected = Some(item.into());
        g.input.mouse = at;
        ctl(g, "spawnRight", false, false, true);
        frames(g, 1);
        ctl(g, "spawnRight", false, false, false);
        assert!(g.physicals.len() > first, "{item}: nothing spawned");
        first
    }

    /// LiquidContainerLeakingBehaviour.OnBeforeSerialise gives SerialisableHoles from the shots; Start makes those holes again.
    #[test]
    fn a_canisters_holes_are_saved_and_made_again() {
        let Some((mut g, p)) = spawned("Liquid Canister") else { return };
        let at = g.physicals[p].transform.position;
        g.liquid_leak_shot(0, [1.0, 0.0], [at[0], at[1]], 2.0);
        g.liquid_leak_on_before_serialise(0);
        let saved = g.liquid_leaks[0].SerialisableHoles.clone().unwrap();
        assert_eq!((saved.len(), saved[0][4]), (1, 2.0));
        g.liquid_leak_repair(0);
        assert!(g.liquid_leaks[0].hole_data.is_empty());
        g.liquid_leaks[0].HasHoles = true;
        g.liquid_leak_start_holes(0);
        assert_eq!((g.liquid_leaks[0].hole_data.clone(), g.liquid_leaks[0].SerialisableHoles.is_none()), (saved, true));
        assert_eq!(g.liquid_leaks[0].holes.len(), 1);
    }

    #[test]
    fn syringe_fills_a_liquid_canister_and_it_floats_less() {
        use crate::game::container::Owner;
        let Some((mut g, p)) = spawned("Liquid Canister") else { return };
        let c = g.liquid_canisters.iter().position(|x| x.physical == p).expect("the canister");
        let at = g.physicals[p].transform.position;
        spawn_at(&mut g, "Life Syringe", [at[0] + 1.0, at[1] + 1.0, 0.0]);
        let s = g.syringes.len() - 1;
        g.syringes[s].Finite = true;
        g.syringe_lodged(s, p);
        assert_eq!(g.syringes[s].pushTargets, [Some(Owner::LiquidCanister(c))]);
        frames(&mut g, 120);
        assert!(g.get_amount(Owner::LiquidCanister(c), crate::game::circulation::liquid_of_class("LifeSerumLiquid")) > 0.1);
        // BuoyancyModifier = 1 - BuoyancyResponseCurve(ScaledLiquidAmount), full at Capacity.
        assert_eq!(g.container_limits(Owner::LiquidCanister(c)), [0.0, 70.0]);
        let life = crate::game::circulation::liquid_of_class("LifeSerumLiquid");
        g.add_liquid(Owner::LiquidCanister(c), life, 100.0);
        assert_eq!(g.bc(Owner::LiquidCanister(c)).TotalLiquidAmount, 70.0);
        frames(&mut g, 1);
        assert!(g.physicals[p].BuoyancyModifier < 0.01, "{}", g.physicals[p].BuoyancyModifier);
        // A shot makes a hole under the canister that drains it (DrainRate = intensity * 0.05 * 0.05); Repair ends that.
        let at = g.physicals[p].transform.position;
        g.liquid_leak_shot(0, [1.0, 0.0], [at[0], at[1]], 2.0);
        assert_eq!(g.liquid_holes.len(), 1);
        assert_eq!(g.liquid_holes[0].DrainRate, (crate::game::d(2.0) * 0.05f32 * 0.05f32).f());
        assert_eq!(g.liquid_holes[0].ForceMultiplier, 0.001 * 2.0);
        frames(&mut g, 60);
        let drained = g.bc(Owner::LiquidCanister(c)).TotalLiquidAmount;
        assert!(drained < 70.0, "{drained}");
        g.liquid_leak_repair(0);
        assert!(!g.liquid_leaks[0].HasHoles);
        frames(&mut g, 2);
        let left = g.bc(Owner::LiquidCanister(c)).TotalLiquidAmount;
        frames(&mut g, 60);
        assert_eq!(g.bc(Owner::LiquidCanister(c)).TotalLiquidAmount, left);
    }

    #[test]
    fn liquid_pump_cycles_its_modes_and_sets_the_pressure() {
        use crate::game::container::Owner;
        let Some((mut g, p)) = spawned("Liquid Pressuriser") else { return };
        let l = g.liquid_pumps.iter().position(|x| x.physical == p).expect("the pump");
        let o = Owner::LiquidPump(l);
        assert_eq!(g.container_pressure(o), 2);
        g.liquid_pump_use(l, 0);
        assert_eq!(g.container_pressure(o), 0);
        g.liquid_pump_use(l, 0);
        assert_eq!(g.container_pressure(o), 1);
        g.liquid_pump_use(l, 1);
        g.liquid_pump_use(l, 1);
        assert_eq!(g.container_pressure(o), 2);
    }

    #[test]
    fn centrifuge_removes_the_smallest_liquid_until_one_is_left() {
        use crate::game::container::Owner;
        let Some((mut g, p)) = spawned("Centrifuge") else { return };
        let c = g.centrifuges.iter().position(|x| x.physical == p).expect("the centrifuge");
        let o = Owner::Centrifuge(c);
        let (life, small) = (crate::game::circulation::liquid_of_class("LifeSerumLiquid"), crate::game::circulation::liquid("ADRENALINE"));
        g.add_liquid(o, life, 2.0);
        g.add_liquid(o, small, 0.5);
        g.centrifuge_use(c);
        frames(&mut g, 300);
        assert_eq!(g.get_amount(o, life), 2.0);
        assert_eq!(g.get_amount(o, small), 0.0);
        assert_eq!(g.centrifuges[c].currentIntensity, 1.0);
        let v = g.centrifuges[c].VibrationBehaviour;
        assert_eq!(g.vibrations[v].VibrateForce, g.centrifuges[c].VibrationIntensity);
    }

    #[test]
    fn liquidentifier_scans_its_contents_and_flushes() {
        use crate::game::container::Owner;
        let Some((mut g, p)) = spawned("Liquidentifier") else { return };
        let l = g.liquidentifiers.iter().position(|x| x.physical == p).expect("the liquidentifier");
        let o = Owner::Liquidentifier(l);
        g.add_liquid(o, crate::game::circulation::liquid_of_class("LifeSerumLiquid"), 3.0);
        g.add_liquid(o, crate::game::circulation::liquid("ADRENALINE"), 1.0);
        g.liquidentifier_use(l, 0);
        assert!(g.liquidentifiers[l].isBusy);
        frames(&mut g, 400);
        assert!(!g.liquidentifiers[l].isBusy);
        let shown = g.text_get(g.liquidentifiers[l].TextMesh.unwrap()).to_string();
        assert!(shown.ends_with("75% Life serum\r\n25% Adrenaline\r\n"), "{:?}", shown);
        g.liquidentifier_use(l, 1);
        frames(&mut g, 400);
        assert_eq!(g.bc(o).TotalLiquidAmount, 0.0);
        assert_eq!(g.text_get(g.liquidentifiers[l].TextMesh.unwrap()), crate::il!(str "LiquidentifierBehaviour/<FlushRoutine>d__15::MoveNext" 1));
    }

    #[test]
    fn tank_barrel_fires_a_hot_shell() {
        let Some((mut g, _)) = spawned("Tank") else { return };
        let before = g.physicals.len();
        g.tank_barrel_behaviour_use(0, crate::game::translated::rt::Activation { Channel: 0 });
        assert!(g.physicals.len() > before, "no shell");
        assert!(g.physicals[before..].iter().any(|p| p.Temperature == 300.0));
    }

    #[test]
    fn old_tank_gets_a_motor_wheel_joint_per_wheel() {
        let Some((g, _)) = spawned("Old Tank") else { return };
        assert_eq!(g.cars[0].WheelJoints.len(), 9);
        let live = g.anchored_joints.iter().filter(|a| g.cars[0].WheelJoints.contains(&a.component) && a.joint.is_some()).count();
        assert_eq!(live, 9);
    }

    #[test]
    fn bicycle_relative_joint_goes_with_its_hinge() {
        let Some((mut g, _)) = spawned("Bicycle") else { return };
        let live = |g: &Game| g.world.joints.iter().filter(|j| j.is_some()).count();
        let s = g.only_exist_withs[0].clone();
        let h = g.anchored_joints.iter().find(|a| a.component == s.Other).and_then(|a| a.hinge).expect("the hinge");
        g.run_hinge_destroy(h);
        let before = live(&g);
        frames(&mut g, 1);
        assert_eq!(live(&g), before - 1);
    }

    #[test]
    fn bicycle_frame_disintegrating_disables_its_joints() {
        let Some((mut g, _)) = spawned("Bicycle") else { return };
        let frame = g.physicals.iter().position(|p| p.name == "Frame").expect("the frame");
        let live = |g: &Game| g.world.joints.iter().filter(|j| j.is_some()).count();
        let before = live(&g);
        g.physical_disintegrate(frame);
        assert!(live(&g) + 2 <= before, "{} of {before}", live(&g));
    }

    #[test]
    fn pulse_drum_recoils_and_pushes_what_it_hits() {
        let Some((mut g, p)) = spawned("Pulse Drum") else { return };
        let i = g.pulse_drums.iter().position(|x| x.physical == p).expect("the drum");
        assert_eq!(g.pulse_drums[i].RayCount, 15);
        g.pulse_drum_use(i);
        frames(&mut g, 2);
        let v = g.physicals[p].rigidbody.velocity(&g.world);
        assert!(v[0] != 0.0 || v[1] != 0.0, "{v:?}");
    }

    #[test]
    fn liquid_duplicator_grows_its_liquid_and_flushes() {
        use crate::game::container::Owner;
        let Some((mut g, p)) = spawned("Liquid Duplicator") else { return };
        let l = g.liquid_duplicators.iter().position(|x| x.physical == p).expect("the duplicator");
        let o = Owner::LiquidDuplicator(l);
        let life = crate::game::circulation::liquid_of_class("LifeSerumLiquid");
        g.add_liquid(o, life, 0.1);
        assert_eq!(g.container_pressure(o), 2);
        g.liquid_duplicator_use(l, 0);
        assert_eq!(g.container_pressure(o), 0);
        frames(&mut g, 600);
        assert!(g.bc(o).TotalLiquidAmount > 1.0, "{}", g.bc(o).TotalLiquidAmount);
        g.liquid_duplicator_use(l, 0);
        g.liquid_duplicator_use(l, 1);
        assert!(g.liquid_duplicators[l].isBusy);
        frames(&mut g, 400);
        assert!(!g.liquid_duplicators[l].isBusy);
        assert_eq!(g.bc(o).TotalLiquidAmount, 0.0);
    }

    #[test]
    fn syringe_pushes_into_and_pulls_from_a_flask() {
        use crate::game::container::Owner;
        let Some((mut g, flask_p)) = spawned("Flask") else { return };
        let f = g.flasks.iter().position(|x| x.Phys == Some(flask_p)).expect("the flask");
        let at = g.physicals[flask_p].transform.position;
        let life = crate::game::circulation::liquid_of_class("LifeSerumLiquid");
        // A finite Life Syringe lodged in the flask pushes its serum in (SyringeBehaviour.push, PressureMode Push).
        let s = g.syringes.len();
        spawn_at(&mut g, "Life Syringe", [at[0] + 1.0, at[1] + 1.0, 0.0]);
        let life_syringe = g.syringes.len() - 1;
        assert_eq!(life_syringe, s);
        g.syringes[s].Finite = true;
        g.syringe_lodged(s, flask_p);
        assert_eq!(g.syringes[s].pushTargets, [Some(Owner::Flask(f))]);
        let before = g.bc(Owner::Syringe(s)).TotalLiquidAmount;
        frames(&mut g, 120);
        let in_flask = g.get_amount(Owner::Flask(f), life);
        assert!(in_flask > 0.1, "nothing reached the flask ({in_flask})");
        assert!(g.bc(Owner::Syringe(s)).TotalLiquidAmount < before, "the syringe kept its liquid");
        // An empty finite syringe on Pull draws it back out (pull: other.TransferTo(TransferRate, this)).
        let e = g.syringes.len();
        spawn_at(&mut g, "Empty Syringe", [at[0] - 1.0, at[1] + 1.0, 0.0]);
        g.syringes[e].Finite = true;
        g.syringe_use(e, 2);
        g.syringe_lodged(e, flask_p);
        assert_eq!(g.bc(Owner::Syringe(e)).TotalLiquidAmount, 0.0);
        frames(&mut g, 120);
        assert!(g.get_amount(Owner::Syringe(e), life) > 0.01, "the empty syringe drew nothing");
    }

    #[test]
    fn physics_gun_holds_a_human_limb_and_lets_go() {
        let Some((mut g, _)) = spawned("Human") else { return };
        // The leftmost limb: the gun's ray, aimed right at its height, meets the human there.
        let centre = |g: &Game, p: usize| g.world.body(g.physicals[p].rigidbody.body).sweep.c;
        let human: Vec<usize> = g.limbs.iter().map(|l| l.PhysicalBehaviour).collect();
        let left = human.iter().copied().min_by(|&a, &b| centre(&g, a).x.partial_cmp(&centre(&g, b).x).unwrap()).unwrap();
        let c = centre(&g, left);
        spawn_at(&mut g, "Physics Gun", [c.x - 1.2, c.y - 0.04, 0.0]);
        let gun = g.physics_guns.len() - 1;
        let gun_p = g.physics_guns[gun].physical;
        // Held down (Value > 0 each frame) it grabs the first physical its ray meets.
        for _ in 0..30 {
            g.physicals[gun_p].ContinuousActivationTracker[0].Value = 1;
            frames(&mut g, 1);
            if g.physics_guns[gun].Target.is_some() {
                break;
            }
        }
        let target = g.physics_guns[gun].Target.expect("the gun grabbed nothing");
        assert!(human.contains(&target), "the gun grabbed something else than the human");
        let f = g.physics_guns[gun].joint.expect("no RelativeJoint2D");
        let j = &g.fixed_joints[f];
        let r = j.relative.expect("not a RelativeJoint2D");
        assert!(j.joint.is_some() && !j.destroyed && j.state.enable_collision, "the joint is not live");
        assert!(matches!(j.connected, Some(BodyOwner::Physical(t)) if t == target) && matches!(j.attached, BodyOwner::Physical(a) if a == gun_p));
        assert_eq!((r.max_force, r.max_torque), (1200.0, 300.0));
        // It carries the limb along when the gun rises: the limb follows (the gun swings on it) at the same distance.
        let before = centre(&g, target).y;
        let held_distance = |g: &Game| {
            let (a, b) = (centre(g, gun_p), centre(g, target));
            ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
        };
        let start = held_distance(&g);
        for _ in 0..60 {
            g.physicals[gun_p].ContinuousActivationTracker[0].Value = 1;
            g.physicals[gun_p].rigidbody.set_velocity(&mut g.world, [0.0, 3.0]);
            frames(&mut g, 1);
        }
        let after = centre(&g, target).y;
        assert!(after > before + 0.1, "the held limb did not follow the gun ({before} -> {after})");
        assert!((held_distance(&g) - start).abs() < 0.05, "the joint let the limb drift from the gun ({start} -> {})", held_distance(&g));
        // Letting go destroys the joint.
        g.physicals[gun_p].ContinuousActivationTracker[0].Value = 0;
        frames(&mut g, 4);
        assert!(g.physics_guns[gun].Target.is_none(), "the gun still has a target");
        assert!(g.fixed_joints[f].destroyed && g.fixed_joints[f].joint.is_none(), "the joint stayed");
    }

    #[test]
    fn activator_electrode_activates_the_hand_it_touches() {
        let Some((mut g, _)) = spawned("Human") else { return };
        let grip = 1;
        let hand = g.grips[grip].PhysicalBehaviour;
        // The hand's grip point: the electrode's emission point goes there, so the hand is touched and can reach the electrode.
        let c = g.transform_point3(hand, g.grips[grip].GripPosition);
        // Something the grip can hold, within its reach but out of the electrode's emission circle.
        let pistol = spawn_at(&mut g, "Pistol", [c[0], c[1] - 0.35, 0.0]);
        assert!(!g.physicals[pistol].HoldingPositions.is_empty());
        spawn_at(&mut g, "Activator Electrode", [c[0] - 0.39, c[1], 0.0]);
        let e = g.activator_electrodes.len() - 1;
        let ep = g.activator_electrodes[e].physical;
        assert!(!g.grips[grip].isHolding);
        // Using the electrode passes the Use on to the object at its emission point: the hand's GripBehaviour picks up.
        g.activate_direct(ep);
        assert!(g.grips[grip].isHolding && g.grips[grip].CurrentlyHolding == Some(pistol), "the touched hand did not take the pistol");
        // A continuous use reaches the first physical at the emission point on the same channel.
        let before: i32 = (0..g.physicals.len()).filter(|&p| p != ep).map(|p| g.physicals[p].ContinuousActivationTracker[0].Value).sum();
        { let a = g.signal_direct(true, 0); g.broadcast_use_continuous(ep, &a); }
        let after: i32 = (0..g.physicals.len()).filter(|&p| p != ep).map(|p| g.physicals[p].ContinuousActivationTracker[0].Value).sum();
        assert!(after > before, "the continuous use reached nothing");
        // A disabled electrode passes nothing on.
        g.activator_electrodes[e].enabled = false;
        { let a = g.signal_direct(true, 0); g.broadcast_use_continuous(ep, &a); }
        let again: i32 = (0..g.physicals.len()).filter(|&p| p != ep).map(|p| g.physicals[p].ContinuousActivationTracker[0].Value).sum();
        assert_eq!(again, after);
    }

    /// Spawns `item` facing right, 1.2 to the left of the human's leftmost limb at its height, and holds its
    /// continuous use for `n` frames. Returns the item's physical and the human's limb physicals.
    fn spray_at_human(g: &mut Game, item: &str, n: usize, mut each: impl FnMut(&mut Game)) -> (usize, Vec<usize>) {
        let centre = |g: &Game, p: usize| g.world.body(g.physicals[p].rigidbody.body).sweep.c;
        let human: Vec<usize> = g.limbs.iter().map(|l| l.PhysicalBehaviour).collect();
        let left = human.iter().copied().min_by(|&a, &b| centre(g, a).x.partial_cmp(&centre(g, b).x).unwrap()).unwrap();
        let c = centre(g, left);
        let first = spawn_at(g, item, [c.x - 1.2, c.y, 0.0]);
        for _ in 0..n {
            g.physicals[first].ContinuousActivationTracker[0].Value = 1;
            each(g);
            frames(g, 1);
        }
        (first, human)
    }

    #[test]
    fn flamethrower_heats_and_ignites_the_human_it_sprays() {
        let Some((mut g, _)) = spawned("Human") else { return };
        let human: Vec<usize> = g.limbs.iter().map(|l| l.PhysicalBehaviour).collect();
        let before: Vec<f32> = human.iter().map(|&p| g.physicals[p].Temperature).collect();
        assert!(human.iter().all(|&p| !g.physicals[p].OnFire));
        let mut lit = false;
        spray_at_human(&mut g, "Flamethrower", 120, |g| lit |= g.limbs.iter().any(|l| g.physicals[l.PhysicalBehaviour].OnFire));
        lit |= human.iter().any(|&p| g.physicals[p].OnFire);
        let heated = human.iter().zip(&before).any(|(&p, &t)| g.physicals[p].Temperature > t + 0.5);
        assert!(lit || heated, "the flames never reached the human");
    }

    #[test]
    fn fire_extinguisher_puts_out_the_burning_limb_it_sprays() {
        let Some((mut g, _)) = spawned("Human") else { return };
        let centre = |g: &Game, p: usize| g.world.body(g.physicals[p].rigidbody.body).sweep.c;
        let human: Vec<usize> = g.limbs.iter().map(|l| l.PhysicalBehaviour).collect();
        let left = human.iter().copied().min_by(|&a, &b| centre(&g, a).x.partial_cmp(&centre(&g, b).x).unwrap()).unwrap();
        let right = human.iter().copied().max_by(|&a, &b| centre(&g, a).x.partial_cmp(&centre(&g, b).x).unwrap()).unwrap();
        for &p in &[left, right] {
            g.physicals[p].OnFire = true;
            g.physicals[p].burnIntensity = 1.0;
        }
        // Fire also dies by itself (0.07% a step), so the far limb counts only while the near one still burned or was sprayed.
        let mut far_lit_after_spray = false;
        spray_at_human(&mut g, "Fire Extinguisher", 60, |g| far_lit_after_spray |= !g.physicals[left].OnFire && g.physicals[right].OnFire);
        assert!(!g.physicals[left].OnFire && g.physicals[left].burnIntensity == 0.0, "the sprayed limb is still burning");
        assert!(far_lit_after_spray, "the limb out of reach was put out with the sprayed one");
    }
}

/// DragTool.GetDragFunction: 0.9; GentleDragTool.GetDragFunction: 0.2.
pub fn drag_function(gentle: bool) -> f32 {
    if gentle { 0.2 } else { 0.9 }
}
/// GentleDragTool.GetDraggerMassFunction
pub const GENTLE_DRAGGER_MASS: f32 = 0.01;
/// GentleDragTool.DragStrengthFunction
pub const GENTLE_DRAG_STRENGTH: f32 = 0.008;
