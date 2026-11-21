//! The MapConfig record of a fn-probe snapshot (tools/recorder/MapConfigProbe.cs, PPG_MAPCONFIG=1): the live MapConfig's
//! Settings, Physics2D.gravity, the heat and cold post volumes' weights, the weather lightning's Chance and each weather
//! overlay (Snow, Rain, Fog) as Target, Visiblity, activeSelf, the material's _Density, the post volume's weight and the
//! AudioSource's volume. A case loads it over the linked map and compares it after the call.
use super::EnvironmentalSettings;
use crate::game::dump::Snapshot;
use crate::game::props::Prop;
use crate::game::visual::SpriteAt;
use crate::game::Game;

fn hx(w: &str) -> Result<f32, String> {
    u32::from_str_radix(w, 16).map(f32::from_bits).map_err(|_| format!("MapConfig record: {w} is no float"))
}

fn h(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}

fn bit(b: bool) -> &'static str {
    if b { "1" } else { "0" }
}

/// "env:<Floodlights>,<Rain>,<Snow>,<Fog>,<Gravity>,<Lightning_chance>,<Ambient_temperature>" (an EnvironmentalSettings argument).
pub(crate) fn parse_settings(s: &str) -> Result<EnvironmentalSettings, String> {
    let p: Vec<&str> = s.strip_prefix("env:").ok_or_else(|| format!("no EnvironmentalSettings: {s}"))?.split(',').collect();
    if p.len() != 7 {
        return Err(format!("EnvironmentalSettings: {s}"));
    }
    Ok(EnvironmentalSettings { Floodlights: p[0] == "1", Rain: p[1] == "1", Snow: p[2] == "1", Fog: p[3] == "1", Gravity: hx(p[4])?, Lightning_chance: hx(p[5])?, Ambient_temperature: hx(p[6])? })
}

fn settings_text(s: &EnvironmentalSettings) -> String {
    format!("env:{},{},{},{},{},{},{}", bit(s.Floodlights), bit(s.Rain), bit(s.Snow), bit(s.Fog), h(s.Gravity), h(s.Lightning_chance), h(s.Ambient_temperature))
}

/// "overlay:<k>" (a MapConfig.OverlayState argument): overlayTarget's k-th entry.
pub(crate) fn parse_overlay(s: &str) -> Result<usize, String> {
    s.strip_prefix("overlay:").and_then(|k| k.parse().ok()).ok_or_else(|| format!("no OverlayState: {s}"))
}

impl Game {
    fn map_overlay_text(&self, k: usize) -> String {
        let Some(o) = &self.map_overlays[k] else { return "taken".into() };
        let density = match o.Material.and_then(|r| self.material_prop(SpriteAt::Renderer(r), "_Density")) {
            Some(Prop::Float(v)) => h(*v),
            Some(_) => "?".into(),
            None => "unset".into(),
        };
        let pp = o.OptionalPostProcessing.as_ref().map_or("-".into(), |(_, v)| h(v.weight));
        let au = o.OptionalAudioSource.map_or("-".into(), |id| h(self.audio.get(id).volume));
        let cur = if self.map_weather[o.kind] == Some(k) { "*" } else { "" };
        format!("{}{cur}:{},{},{},{},{},{}", o.kind, bit(o.Target), h(o.Visiblity), bit(o.inst.objects[0].self_active), density, pp, au)
    }

    fn map_config_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("settings", self.map_settings.map_or("null".into(), |s| settings_text(&s))),
            ("grav", format!("{},{}", h(self.world.gravity.x), h(self.world.gravity.y))),
            ("heat", h(self.post_weights[0])),
            ("cold", h(self.post_weights[1])),
            ("light", none_if_empty(self.map_lightnings().map(|l| format!("{}{}", if self.map_lightning == Some(l) { "*" } else { "" }, h(self.lightnings[l].Chance))).collect())),
            ("ov", none_if_empty((0..self.map_overlays.len()).map(|k| self.map_overlay_text(k)).collect())),
        ]
    }

    /// The map's live WeatherLightningBehaviours, in creation order.
    fn map_lightnings(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.lightnings.len()).filter(|&l| !self.destroyed_components.contains(&self.lightnings[l].id))
    }

    /// The record's state over the linked map's (whose Start ran with the prefab's Settings).
    pub(crate) fn load_map_config_record(&mut self, s: &Snapshot) -> Result<(), String> {
        let Some(r) = s.of("MapConfig").next() else { return Ok(()) };
        self.map_settings = match r.req("settings")? {
            "null" => None,
            t => Some(parse_settings(t)?),
        };
        let g = r.req("grav")?.split(',').map(hx).collect::<Result<Vec<_>, _>>()?;
        self.world.gravity = crate::b2::math::Vec2::new(g[0], g[1]);
        for (i, k) in ["heat", "cold"].into_iter().enumerate() {
            match r.req(k)? {
                "none" => {}
                v => self.post_weights[i] = hx(v)?,
            }
        }
        // The clones of a Reinitialise run again (the linked map has its Start's): made again, in order.
        let m = self.map_config.clone();
        let light = r.req("light")?;
        for (i, v) in light.split('|').filter(|_| light != "none").enumerate() {
            if self.map_lightnings().count() <= i {
                self.map_config_prepare_lightning_prefab(m.as_ref().ok_or("a MapConfig record without the rewrite's MapConfig")?);
            }
            let l = self.map_lightnings().nth(i).unwrap();
            let v = match v.strip_prefix('*') {
                Some(v) => {
                    self.map_lightning = Some(l);
                    v
                }
                None => v,
            };
            self.lightnings[l].Chance = hx(v)?;
        }
        let ov = r.req("ov")?;
        for (k, t) in ov.split('|').filter(|_| ov != "none").enumerate() {
            let (head, t) = t.split_once(':').ok_or_else(|| format!("MapConfig overlay {t}"))?;
            let kind: usize = head.trim_end_matches('*').parse().map_err(|_| format!("MapConfig overlay slot {head}"))?;
            if self.map_overlays.len() <= k {
                self.map_config_prepare_weather_instance(m.as_ref().ok_or("a MapConfig record without the rewrite's MapConfig")?, kind);
            }
            if head.ends_with('*') {
                self.map_weather[kind] = Some(k);
            }
            let p: Vec<&str> = t.split(',').collect();
            let mut o = self.map_overlays[k].take().unwrap();
            o.Target = p[0] == "1";
            o.Visiblity = hx(p[1])?;
            self.map_overlay_set_active(&mut o, p[2] == "1");
            if let (Some(m), true) = (o.Material, p[3] != "-") {
                self.overlay_set_density(m, hx(p[3])?);
            }
            if let (Some((_, v)), true) = (&mut o.OptionalPostProcessing, p[4] != "-") {
                v.weight = hx(p[4])?;
                let (id, w) = (v.id, v.weight);
                if let Some(x) = self.post_volumes.iter_mut().find(|x| x.id == id) {
                    x.weight = w;
                }
            }
            if let (Some(id), true) = (o.OptionalAudioSource, p[5] != "-") {
                self.audio.get_mut(id).set_volume(hx(p[5])?);
            }
            self.map_overlays[k] = Some(o);
        }
        Ok(())
    }
}

/// The snapshot's MapConfig record against the rewrite's state (nothing without one).
pub(crate) fn compare(g: &Game, s: &Snapshot) -> Vec<String> {
    let Some(r) = s.of("MapConfig").next() else { return Vec::new() };
    g.map_config_fields().into_iter().filter_map(|(k, ours)| {
        let theirs = r.get(k).unwrap_or("");
        (theirs != ours).then(|| format!("MapConfig.{k}: game {theirs} ours {ours}"))
    }).collect()
}

fn none_if_empty(v: Vec<String>) -> String {
    if v.is_empty() { "none".into() } else { v.join("|") }
}

#[cfg(test)]
mod tests {
    #[test]
    fn settings_round_trip() {
        let t = "env:1,0,1,0,c11cf5c3,3f000000,41900000";
        assert_eq!(super::settings_text(&super::parse_settings(t).unwrap()), t);
        assert_eq!(super::parse_overlay("overlay:2"), Ok(2));
    }
}
