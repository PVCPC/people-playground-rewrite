//! DangleBehaviour (the broken fire extinguisher's handle): a node on a string from the pivot, stepped at 30 Hz while
//! the handle's sprite is drawn, turns the handle towards it.
use super::misc::FixedIntervalDistributor;
use super::{d, mathf, BareAnchor, Game};
use crate::unity::transform::{global_position, set_world, vector3_slerp, Trs};

#[allow(non_snake_case)]
#[derive(Clone, Debug)]
pub struct Dangle {
    pub id: i32,
    pub enabled: bool,
    /// The handle's own SpriteRenderer (OnWillRenderObject), None without one.
    pub renderer: Option<usize>,
    /// What its Transform hangs from, and the local TRSs up to that (its own first).
    pub anchor: BareAnchor,
    pub chain: Vec<Trs>,
    /// The renderers drawn through its Transform: (renderer, chain index).
    pub refs: Vec<(usize, usize)>,
    pub Pivot: [f32; 2],
    pub Length: f32,
    pub AngleOffset: f32,
    pub Dampening: f32,
    pub(crate) nodePosition: [f32; 2],
    pub(crate) nodeVelocity: [f32; 2],
    pub(crate) nodeForce: [f32; 2],
    pub(crate) previousDir: [f32; 2],
    pub(crate) dir: [f32; 2],
    pub(crate) distributor: FixedIntervalDistributor,
}

impl Default for Dangle {
    fn default() -> Self {
        Self {
            id: Default::default(),
            enabled: Default::default(),
            renderer: Default::default(),
            anchor: Default::default(),
            chain: Default::default(),
            refs: Default::default(),
            Pivot: Default::default(),
            Length: 1.0,
            AngleOffset: Default::default(),
            Dampening: 0.05,
            nodePosition: Default::default(),
            nodeVelocity: Default::default(),
            nodeForce: Default::default(),
            previousDir: Default::default(),
            dir: Default::default(),
            distributor: Default::default(),
        }
    }
}

impl Dangle {
    #[allow(non_snake_case)]
    pub fn new(id: i32, enabled: bool, Pivot: [f32; 2], Length: f32, AngleOffset: f32, Dampening: f32) -> Self {
        // distributor = new FixedIntervalDistributor { RateHz = 30f, MaxCycles = 1 }
        let mut distributor = FixedIntervalDistributor::with_rate(30.0);
        distributor.MaxCycles = 1;
        let z = [0.0; 2];
        Self { id, enabled, renderer: None, anchor: BareAnchor::Root, chain: Vec::new(), refs: Vec::new(), Pivot, Length, AngleOffset, Dampening, nodePosition: z, nodeVelocity: z, nodeForce: z, previousDir: z, dir: z, distributor }
    }
}

impl Game {
    /// base.transform.TransformPoint(Pivot), as a Vector2.
    fn dangle_pivot(&self, i: usize) -> [f32; 2] {
        let s = &self.dangles[i];
        let mut v = [s.Pivot[0], s.Pivot[1], 0.0, 0.0];
        for n in &self.anchored_nodes(s.anchor, &s.chain) {
            v = global_position(n, v);
        }
        [v[0], v[1]]
    }

    /// DangleBehaviour.Awake
    pub fn dangle_awake(&mut self, i: usize) {
        let p = self.dangle_pivot(i);
        let s = &mut self.dangles[i];
        s.nodePosition = p;
        s.dir = [1.0, 0.0];
        s.previousDir = s.dir;
    }

    /// DangleBehaviour.OnWillRenderObject for each one whose sprite the camera draws.
    pub fn dangles_will_render(&mut self) {
        for i in 0..self.dangles.len() {
            let s = &self.dangles[i];
            let Some(r) = s.renderer else { continue };
            if !self.will_render_dispatch(s.id, s.enabled && !self.disabled_ids.contains(&s.id) && self.sprite_drawn(super::visual::SpriteAt::Renderer(r))) {
                continue;
            }
            self.dangle_will_render(i);
        }
    }

    fn dangle_will_render(&mut self, i: usize) {
        let vector = self.dangle_pivot(i);
        let (dt, g) = (self.time.deltaTime, self.world.gravity);
        let s = &mut self.dangles[i];
        let interval = s.distributor.IntervalSeconds;
        let num = interval * interval;
        let num2 = mathf::clamp01(1.0 - s.Dampening);
        let add = |a: [f32; 2], b: [f32; 2]| [a[0] + b[0], a[1] + b[1]];
        let sub = |a: [f32; 2], b: [f32; 2]| [a[0] - b[0], a[1] - b[1]];
        let mul = |a: [f32; 2], k: f32| [a[0] * k, a[1] * k];
        // CalculateCycleCount runs in the loop's condition, once per check.
        let mut n = 0;
        while n < s.distributor.calculate_cycle_count(dt) {
            let mut vector2 = add(add(s.nodePosition, mul(s.nodeVelocity, interval)), mul(s.nodeForce, num));
            let vector3 = sub(vector, vector2);
            let magnitude = mathf::magnitude2(vector3);
            if magnitude != 0.0 {
                s.previousDir = s.dir;
                s.dir = [vector3[0] / magnitude, vector3[1] / magnitude];
                let num3 = s.Length - magnitude;
                vector2 = sub(vector2, mul(s.dir, num3 * 0.9));
            }
            let v = sub(vector2, s.nodePosition);
            s.nodeVelocity = mul([v[0] / interval, v[1] / interval], num2);
            s.nodePosition = vector2;
            s.nodeForce = [g.x, g.y];
            n += 1;
        }
        let slerped = vector3_slerp([s.previousDir[0], s.previousDir[1], 0.0], [s.dir[0], s.dir[1], 0.0], s.distributor.InterpolationTime);
        let normalized = super::person::normalized(slerped);
        // Mathf.Atan2 is Math.Atan2 on doubles.
        let atan = crate::game::mathf::atan2(normalized[1], normalized[0]);
        let z = (d(s.AngleOffset) + d(57.29578) * atan).f();
        // base.transform.rotation = Quaternion.Euler(0, 0, z): the local rotation under its parents.
        let q = super::menu::euler_z_rotation(z);
        let (anchor, chain) = (s.anchor, s.chain.clone());
        let ((_, local), _) = set_world(&self.anchored_nodes(anchor, &chain[1..]), [0.0; 3], q);
        self.dangles[i].chain[0].q = local;
        // gapB r11: the GameObject's own Transform too (its Clone's localRotation), not only the renderers' copy.
        let id = self.dangles[i].id;
        self.with_component_instance(id, |_, inst, o| inst.objects[o].trs.q = local);
        let own = self.dangles[i].chain[0];
        for &(r, k) in &self.dangles[i].refs {
            self.renderers[r].chain[k] = own;
        }
    }
}
