//! Render state the weapons, menus and props drive: SetActive of drawn-only objects and the like.
#![allow(non_snake_case)]

use super::visual::SpriteAt;
use super::Game;

impl Game {
    /// GameObject.SetActive(on) of a subtree that only draws (`Linker::plain_field`): the drawn sprites follow.
    pub fn set_active_plain(&mut self, transform: i32, on: bool) {
        self.with_component_instance(transform, |g, inst, o| {
            let scripts = g.set_active(inst, o, on);
            // Optout is a marker MonoBehaviour with no methods.
            assert!(scripts.iter().all(|&(d, k)| matches!(&inst.prefab.objects[d].components[k].1, crate::assets::db::Component::Script { class, .. } if class == "Optout" || !(super::declares(class, "OnEnable") || super::declares(class, "OnDisable")))), "SetActive reached a script with OnEnable or OnDisable on a draw-only object");
        });
    }

    /// SpriteRenderer.transform.position = position (Transform::SetPosition: the local position from the parents, z kept as given).
    pub fn set_sprite_world_position(&mut self, s: SpriteAt, position: [f32; 3]) {
        match s {
            SpriteAt::Physical(_) => unimplemented!("mod-only: transform.position of a SpriteRenderer on a PhysicalBehaviour object"),
            SpriteAt::Renderer(r) => {
                let (anchor, chain) = (self.renderers[r].anchor, self.renderers[r].chain.clone());
                let (_, rotation, _) = self.anchored_world(anchor, &chain);
                let nodes = self.anchored_nodes(anchor, &chain);
                let ((local, _), _) = crate::unity::transform::set_world(&nodes[1..], position, rotation);
                let x = &mut self.renderers[r].chain[0].t;
                *x = [local[0], local[1], local[2], x[3]];
                self.renderer_object(r, |inst, o| inst.set_local_position(o, local));
            }
        }
    }

    /// gapC r13: the clone object holding renderer `r` gets the same local write (its Transform is one: ours kept
    /// only the renderer's chain copy). A renderer on no clone (or on the clone checked out now) keeps the copy only.
    pub(super) fn renderer_object(&mut self, r: usize, f: impl FnOnce(&mut super::spawn::Instance, usize)) {
        let id = self.renderers[r].id;
        if let Some(inst) = self.unsynced.iter_mut().chain(self.retained.iter_mut()).find(|i| i.objects.iter().any(|x| x.components.contains(&id))) {
            let o = inst.objects.iter().position(|x| x.components.contains(&id)).unwrap();
            f(inst, o);
        }
    }

    /// SpriteRenderer.transform.localScale = scale.
    pub fn set_sprite_local_scale(&mut self, s: SpriteAt, scale: [f32; 3]) {
        match s {
            SpriteAt::Physical(p) => self.physicals[p].transform.localScale = scale,
            SpriteAt::Renderer(r) => {
                let x = &mut self.renderers[r].chain[0].s;
                *x = [scale[0], scale[1], scale[2], x[3]];
                self.renderer_object(r, |inst, o| inst.set_local_scale(o, scale));
            }
        }
    }

    /// SpriteRenderer.transform.localPosition.
    pub fn sprite_local_position(&self, s: SpriteAt) -> [f32; 3] {
        match s {
            SpriteAt::Physical(_) => unimplemented!("mod-only: localPosition of a physical object's own SpriteRenderer"),
            SpriteAt::Renderer(r) => {
                let t = self.renderers[r].chain[0].t;
                [t[0], t[1], t[2]]
            }
        }
    }

    /// SpriteRenderer.transform.localPosition = position.
    pub fn set_sprite_local_position(&mut self, s: SpriteAt, position: [f32; 3]) {
        match s {
            SpriteAt::Physical(_) => unimplemented!("mod-only: localPosition of a physical object's own SpriteRenderer"),
            SpriteAt::Renderer(r) => {
                let t = &mut self.renderers[r].chain[0].t;
                *t = [position[0], position[1], position[2], t[3]];
                self.renderer_object(r, |inst, o| inst.set_local_position(o, position));

            }
        }
    }

    /// SpriteRenderer.transform.localScale.
    pub fn sprite_local_scale(&self, s: SpriteAt) -> [f32; 3] {
        match s {
            SpriteAt::Physical(p) => self.physicals[p].transform.localScale,
            SpriteAt::Renderer(r) => {
                let x = self.renderers[r].chain[0].s;
                [x[0], x[1], x[2]]
            }
        }
    }
}
