//! LiquidentifierBehaviour: scans what it holds into a percentage list, or flushes it. TextMesh and StateMesh are world texts;
//! the sounds are presentation, the button is UI. The draws stay.
#![allow(non_snake_case)]

use super::container::{BloodContainer, Owner};
use super::coroutine::{Routine, Yield};
use super::{circulation, d, mathf, script, Game};

script! {
    Liquidentifier {}
    base { base: BloodContainer }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour
        physical: usize = 0,
        /// LiquidContainer, the LiquidContainerController whose fill Update sets.
        LiquidContainer: Option<usize> = None,
        /// FlushSound.length
        flush_length: f32 = 0.0,
        /// Clips.Length (PickRandom draws).
        Clips: usize = 0,
        /// Lights (SpriteRenderers: the routine and OnDisable switch them).
        Lights: Vec<Option<super::visual::SpriteAt>> = Vec::new(),
        /// TextMesh and StateMesh (world_text.rs).
        TextMesh: Option<usize> = None,
        StateMesh: Option<usize> = None,
        isBusy: bool = false,
    }
}

impl Game {
    /// LiquidentifierBehaviour.Use
    pub fn liquidentifier_use(&mut self, l: usize, channel: i32) {
        if self.liquidentifiers[l].enabled {
            let id = self.liquidentifiers[l].id;
            let routine = if channel == 1 { Self::liquidentifier_flush(l) } else { Self::liquidentifier_identify(l) };
            self.start_coroutine(id, id, routine);
        }
    }

    /// The text IdentificationRoutine leaves: LiquidDistribution by Raw, largest first, and the last share takes the rest.
    fn liquidentifier_contents(&mut self, l: usize) -> String {
        let o = Owner::Liquidentifier(l);
        self.delete_empty_liquid_entries(o);
        let mut text = String::from(crate::il!(str "LiquidentifierBehaviour/<IdentificationRoutine>d__14::MoveNext" 2));
        let total = self.bc(o).TotalLiquidAmount;
        if total > mathf::EPSILON {
            let mut items: Vec<(usize, f32)> = self.bc(o).LiquidDistribution.keys().into_iter().map(|k| (k, self.bc(o).LiquidDistribution.get(k).unwrap().raw)).collect();
            // OrderByDescending is stable, as is this.
            items.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            let (count, mut num, mut num2) = (items.len() as i32, 0, 0);
            for (liquid, raw) in items {
                let num3 = raw / total;
                let num4 = if num2 == count - 1 { 100 - num } else { (d(num3) * 100.0f32).f().round_ties_even() as i32 };
                if num4 > 0 {
                    text += &format!("{num4}% {}\r\n", circulation::liquid_display_name(liquid));
                    num += num4;
                    num2 += 1;
                }
            }
        } else {
            text += "<#222>(EMPTY)";
        }
        text
    }

    /// LiquidentifierBehaviour.IdentificationRoutine
    pub(crate) fn liquidentifier_identify(l: usize) -> Routine {
        let mut step = 0;
        Box::new(move |g| {
            step += 1;
            match step {
                1 => {
                    if g.liquidentifiers[l].isBusy {
                        return Yield::Done;
                    }
                    g.liquidentifiers[l].isBusy = true;
                    g.liquidentifier_lights(l, |_| true);
                    // PhysicalBehaviour.PlayClipOnce(Clips.PickRandom())
                    let (p, id, n) = (g.liquidentifiers[l].physical, g.liquidentifiers[l].id, g.liquidentifiers[l].Clips);
                    let clip = g.pick_clip(id, "Clips", n);
                    g.play_clip_once(p, clip, 1.0);
                    g.text_write(g.liquidentifiers[l].TextMesh, "INITIALISING...");
                    Yield::WaitForSeconds(1.123)
                }
                2..=31 => {
                    if step == 2 {
                        g.text_write(g.liquidentifiers[l].TextMesh, "SCANNING...");
                    }
                    // Lights[k].enabled = (double)Random.value > 0.5
                    g.liquidentifier_lights(l, |g| g.random.value() as f64 > 0.5);
                    Yield::WaitForSeconds(0.0682)
                }
                _ => {
                    let text = g.liquidentifier_contents(l);
                    g.liquidentifier_lights(l, |_| false);
                    g.text_write(g.liquidentifiers[l].TextMesh, &text);
                    g.liquidentifiers[l].isBusy = false;
                    Yield::Done
                }
            }
        })
    }

    /// LiquidentifierBehaviour.FlushRoutine
    pub(crate) fn liquidentifier_flush(l: usize) -> Routine {
        let o = Owner::Liquidentifier(l);
        let (mut started, mut start_amount, mut time_spent) = (false, 0.0f32, 0.0f32);
        Box::new(move |g| {
            if !started {
                started = true;
                if g.liquidentifiers[l].isBusy {
                    return Yield::Done;
                }
                g.liquidentifiers[l].isBusy = true;
                let (p, id) = (g.liquidentifiers[l].physical, g.liquidentifiers[l].id);
                let clip = g.audio.clip(id, "FlushSound");
                g.play_clip_once(p, clip, 1.0);
                g.text_write(g.liquidentifiers[l].TextMesh, "FLUSHING...");
                start_amount = g.bc(o).TotalLiquidAmount;
            } else {
                time_spent += g.time.deltaTime;
            }
            if (g.bc(o).TotalLiquidAmount >= mathf::EPSILON || time_spent <= g.liquidentifiers[l].flush_length) && time_spent < 5.0 {
                let amount = (d(start_amount) * g.time.deltaTime / 1.521f32).f();
                g.drain(o, amount);
                return Yield::WaitForEndOfFrame;
            }
            g.delete_empty_liquid_entries(o);
            g.text_write(g.liquidentifiers[l].TextMesh, crate::il!(str "LiquidentifierBehaviour/<FlushRoutine>d__15::MoveNext" 1));
            g.liquidentifiers[l].isBusy = false;
            Yield::Done
        })
    }

    /// LiquidentifierBehaviour.Update
    pub fn liquidentifier_update(&mut self, l: usize) {
        let o = Owner::Liquidentifier(l);
        self.blood_container_update(o);
        // (float)Mathf.CeilToInt(TotalLiquidAmount * (5f / 14f) * 10f) * 0.1f + " LITER"
        let n = (self.bc(o).TotalLiquidAmount * (5.0f32 / 14.0f32) * 10.0f32).ceil() as i32;
        let text = format!("{} LITER", super::menu::cs_float(n as f32 * 0.1f32));
        self.text_write(self.liquidentifiers[l].StateMesh, &text);
        if let Some(c) = self.liquidentifiers[l].LiquidContainer {
            self.liquid_containers[c].FillPercentage = mathf::clamp(self.scaled_liquid_amount(o), 0.0, 1.0);
            self.liquid_container_color_from(c, o);
        }
    }

    /// Lights[i].enabled = on(), in order (a null light throws).
    fn liquidentifier_lights(&mut self, l: usize, mut on: impl FnMut(&mut Game) -> bool) {
        for k in 0..self.liquidentifiers[l].Lights.len() {
            let v = on(self);
            let light = self.liquidentifiers[l].Lights[k].unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            self.set_sprite_enabled(light, v);
        }
    }

    /// LiquidentifierBehaviour.OnDisable
    pub(crate) fn liquidentifier_on_disable(&mut self, l: usize) {
        let id = self.liquidentifiers[l].id;
        self.stop_all_coroutines(id);
        self.liquidentifier_lights(l, |_| false);
        self.liquidentifiers[l].isBusy = false;
        self.text_write(self.liquidentifiers[l].TextMesh, crate::il!(str "LiquidentifierBehaviour::OnDisable" 0));
    }
}
