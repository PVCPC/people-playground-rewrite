//! FoliageBehaviour (the bushes' and trees' leaves): Start's z jitter (a Random.value draw), and the burn progress its
//! OnWillRenderObject copies from the parent PhysicalBehaviour, which deactivates the leaf at 0.95, and the property block
//! that gives it to the leaf's Unlit/FoliageShader (foliage.wesl).
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    Foliage {
        BurnProgress: f32 = 0.0,
        SyncWithParentPhysicalBehaviour: bool = true,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// phys: GetComponentInParent<PhysicalBehaviour>() in Awake when syncing.
        phys: Option<usize> = None,
        /// The `Game.renderers` entry of Renderer (the one OnWillRenderObject follows).
        renderer: usize = 0,
        /// ShaderPropertyName ([SkipSerialisation]) and materialProperty (Awake's copy of Renderer's block).
        ShaderPropertyName: String = "_BurnProgress".into(),
        materialProperty: super::props::PropBlock = Default::default(),
    }
}

impl Game {
    /// FoliageBehaviour.SetBurnProgressInternal: the property block, the field, and SetActive(false) from 0.95.
    pub(crate) fn foliage_set_burn_progress_internal(&mut self, f: usize, burn_progress: f32) {
        self.foliage_burn_progress_block(f, burn_progress);
        if burn_progress >= 0.95 {
            self.with_component_instance(self.foliages[f].id, |g, inst, o| g.set_active_scripts(inst, o, false));
        }
    }

    /// SetBurnProgressInternal before its SetActive: the property block and the field.
    pub(crate) fn foliage_burn_progress_block(&mut self, f: usize, burn_progress: f32) {
        let name = self.foliages[f].ShaderPropertyName.clone();
        self.foliages[f].materialProperty.set_float(&name, burn_progress);
        let block = self.foliages[f].materialProperty.clone();
        self.set_property_block(super::visual::SpriteAt::Renderer(self.foliages[f].renderer), &block);
        self.foliages[f].BurnProgress = burn_progress;
    }

    /// FoliageBehaviour.Start
    pub fn foliage_start(&mut self, f: usize) {
        let id = self.foliages[f].id;
        self.foliage_set_burn_progress_internal(f, self.foliages[f].BurnProgress);
        let r = self.random.value();
        // A clone Physics2D already synced is kept: its Rigidbody2D and colliders follow at the next SyncTransforms.
        if !self.unsynced.iter().any(|i| i.objects.iter().any(|x| x.components.contains(&id))) {
            self.retained_sync.push(id);
        }
        self.with_component_instance(id, |_, inst, o| {
            let p = inst.position(o);
            inst.set_world_position(o, [p[0], p[1], p[2] + r * 0.5]);
        });
    }

    /// OnWillRenderObject of the leaves the camera draws, in renderer order: their SpriteRenderer is enabled and active
    /// in the hierarchy, and its bounds overlap the orthographic view (`view` None: everything is drawn).
    // The order is RendererScene's node order (`renderer_scene`; reference-probe/game/foliage.txt at spawn).
    pub fn foliage_will_render(&mut self, view: Option<([f32; 2], [f32; 2])>) {
        let mut order: Vec<usize> = (0..self.foliages.len()).collect();
        order.sort_by_key(|&f| self.renderer_nodes.iter().position(|&n| n == self.renderers[self.foliages[f].renderer].id));
        // FoliageBehaviour.OnWillRenderObject: for each leaf drawn, below
        for f in order {
            let r = &self.renderers[self.foliages[f].renderer];
            if !r.visible || !r.enabled {
                continue;
            }
            let drawn = view.is_none_or(|(min, max)| {
                let (c, e) = crate::unity::transform::transform_aabb(r.bounds.0, r.bounds.1, &self.anchored_nodes(r.anchor, &r.chain));
                (0..2).all(|i| c[i] + e[i] >= min[i] && c[i] - e[i] <= max[i])
            });
            if drawn {
                self.foliage_on_will_render(f);
            }
        }
    }

    /// FoliageBehaviour.OnWillRenderObject
    pub fn foliage_on_will_render(&mut self, f: usize) {
        // if (SyncWithParentPhysicalBehaviour && (bool)phys) SetBurnProgress(phys.BurnProgress)
        let s = &self.foliages[f];
        let Some(p) = s.phys.filter(|_| s.SyncWithParentPhysicalBehaviour) else { return };
        if self.destroyed_components.contains(&self.physicals[p].id) {
            return;
        }
        // FoliageBehaviour.SetBurnProgress: skipped within 0.01 of the field
        let burn_progress = self.physicals[p].BurnProgress;
        if !((burn_progress - self.foliages[f].BurnProgress).abs() < 0.01) {
            self.foliage_set_burn_progress_internal(f, burn_progress);
        }
    }
}
