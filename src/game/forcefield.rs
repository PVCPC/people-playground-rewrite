//! ForcefieldGeneratorBehaviour ([Obsolete], on no prefab): Use switches the shield child (GetChild(0)) on and off, a body
//! entering its trigger is shoved away with an impulse and leaves a hit in the shader's ring of 32 hit positions.
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    Forcefield {
        Activated: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// The PhysicalBehaviour on the generator's GameObject: its Rigidbody2D is `rigidbody`.
        physical: usize = 0,
        /// shield = transform.GetChild(0): its object index in the instance.
        shield: usize = 0,
        /// The GameObject's Collider2Ds and Rigidbody2D: the trigger messages it receives.
        colliders: Vec<i32> = Vec::new(),
        rigidbody: i32 = 0,
        /// hits: the _HitPos array (x, y, age, 0), 32 of them.
        hits: Vec<[f32; 4]> = vec![[0.0; 4]; 32],
    }
}

impl Game {
    /// ForcefieldGeneratorBehaviour.Awake: every hit starts at (0, 0, 100, 0). `mat` (GetComponentInChildren<MeshRenderer>().material)
    /// only feeds the shader.
    pub fn forcefield_awake(&mut self, i: usize) {
        self.forcefields[i].hits = vec![[0.0, 0.0, 100.0, 0.0]; 32];
    }

    /// ForcefieldGeneratorBehaviour.Use: Activated flips and the shield follows with SetActive.
    pub fn forcefield_use(&mut self, i: usize) {
        self.forcefields[i].Activated = !self.forcefields[i].Activated;
        let (id, shield, on) = (self.forcefields[i].id, self.forcefields[i].shield, self.forcefields[i].Activated);
        self.with_component_instance(id, |g, inst, _| g.set_active_scripts(inst, shield, on));
    }

    /// Whether ForcefieldGeneratorBehaviour `i` gets the trigger messages of collider `own`.
    pub(super) fn forcefield_receives(&self, i: usize, own: i32, rigidbody: Option<crate::b2::world::RigidbodyRef>) -> bool {
        let s = &self.forcefields[i];
        super::decimator::trigger_receives(&s.colliders, s.rigidbody, own, rigidbody)
    }

    /// ForcefieldGeneratorBehaviour.OnTriggerEnter2D(collision): a collider with an attached Rigidbody2D is pushed along
    /// (its position - the generator's) * the relative speed * its mass * 2 as an impulse at the collider's closest point.
    pub fn forcefield_trigger_enter(&mut self, i: usize, collision: i32) {
        let Some(body) = self.world.collider_body(collision) else { return };
        // A collider on a body with no Rigidbody2D (the map's static one) has a null attachedRigidbody.
        let Some(attached) = self.rigidbody_of_body(body) else { return };
        let own = self.physicals[self.forcefields[i].physical].rigidbody.clone();
        let (v, w) = (own.velocity(&self.world), attached.velocity(&self.world));
        let magnitude = super::mathf::magnitude2([v[0] - w[0], v[1] - w[1]]);
        let at = self.physicals[self.forcefields[i].physical].transform.position;
        // collider.ClosestPoint(transform.position), after AutoSyncTransforms.
        self.sync_pending();
        let c = self.world.collider_closest_point(collision, crate::b2::math::Vec2::new(at[0], at[1]));
        let vector = [c.x, c.y];
        self.forcefield_create_hit(i, vector, magnitude);
        // (collision.transform.position - base.transform.position).normalized * magnitude * mass * 2
        let theirs = match self.physical_of_collider(collision) {
            Some(p) => self.physicals[p].transform.position,
            None => self.bare_position(collision),
        };
        let dir = super::explosion::normalized3([theirs[0] - at[0], theirs[1] - at[1], theirs[2] - at[2]]);
        let mass = attached.mass(&self.world);
        let impulse = [dir[0] * magnitude * mass * 2.0, dir[1] * magnitude * mass * 2.0];
        attached.add_impulse_at_position(&mut self.world, impulse, vector);
    }

    /// ForcefieldGeneratorBehaviour.CreateHit: the oldest hit slot takes the shield-local point and 1 / intensity as its age.
    pub(crate) fn forcefield_create_hit(&mut self, i: usize, world_pos: [f32; 2], intensity: f32) {
        let oldest = self.forcefield_get_oldest_index(i);
        let (id, shield) = (self.forcefields[i].id, self.forcefields[i].shield);
        let local = self.with_component_instance(id, |_, inst, _| {
            let mut v = [world_pos[0], world_pos[1], 0.0, 0.0];
            for node in inst.chain(shield).iter().rev() {
                v = crate::unity::transform::inverse(node, v, [0.0, 0.0, 0.0, 1.0]).0;
            }
            [v[0], v[1], v[2]]
        });
        // hits[oldestIndex] = (Vector3) assigned to a Vector4 has w = 0; then z = 1 / intensity.
        let oldest = oldest.unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array."));
        self.forcefields[i].hits[oldest] = [local[0], local[1], local[2], 0.0];
        self.forcefields[i].hits[oldest][2] = 1.0 / intensity;
    }

    /// ForcefieldGeneratorBehaviour.GetOldestIndex: the slot with the greatest age above 0, or None for -1.
    pub(crate) fn forcefield_get_oldest_index(&self, i: usize) -> Option<usize> {
        let mut num = 0.0f32;
        let mut result = None;
        for (k, h) in self.forcefields[i].hits.iter().enumerate() {
            if num < h[2] {
                num = h[2];
                result = Some(k);
            }
        }
        result
    }

    /// ForcefieldGeneratorBehaviour.Update: every hit ages by deltaTime (SetVectorArray is the shader's).
    pub fn forcefield_update(&mut self, i: usize) {
        let dt = self.time.deltaTime;
        for h in &mut self.forcefields[i].hits {
            h[2] += dt;
        }
    }
}
