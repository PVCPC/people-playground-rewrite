//! UnityEvent persistent calls (UnityEngine.Events.PersistentCall): the serialised target, m_MethodName, m_Mode and
//! argument of each call.
//!
//! UnityEvent.Invoke runs the persistent calls in list order. PersistentCall.GetRuntimeCall builds no call for an Off
//! call, an empty method name or a null target; InvokableCall.Invoke skips a target destroyed since (AllowInvoke), which
//! `Game::event_call` does per call. Each call becomes one `EventCall` (link.rs `try_event_calls` holds the methods the
//! rewrite has); a method it lacks becomes `EventCall::Stop`, which stops when the call runs, naming Class.Method.
//! The m_Mode of every shipped call matches the one overload its method has (inventory test below), so the argument
//! field each kept call reads is the one Unity passes.

use super::link::{EventCall, invoked_calls, try_event_calls};
use crate::assets::db::{Component, ObjRef, Prefab};
use crate::assets::mono::Value;
use super::spawn::Instance;

/// `msg` as a `&'static str`, leaked once per distinct message.
pub(super) fn stop(msg: String) -> &'static str {
    if std::env::var_os("PP_STOP_PROBE").is_some() {
        println!("STOPCALL {msg}");
    }
    static SEEN: std::sync::Mutex<std::collections::BTreeSet<&'static str>> = std::sync::Mutex::new(std::collections::BTreeSet::new());
    let mut seen = SEEN.lock().unwrap();
    if let Some(&s) = seen.get(msg.as_str()) {
        return s;
    }
    let s: &'static str = Box::leak(msg.into_boxed_str());
    seen.insert(s);
    s
}

/// The class of a persistent call's target, as Unity names it.
fn target_class(prefab: &Prefab, file: usize, call: &Value) -> String {
    let t = call.get("m_Target").and_then(Value::pptr).unwrap_or_default();
    if t.file != 0 {
        return "an object outside the prefab".into();
    }
    match prefab.index.get(&ObjRef { file, path_id: t.path_id }) {
        None => "a dangling reference".into(),
        Some(&(_, usize::MAX)) => "GameObject".into(),
        Some(&(o, c)) => match &prefab.objects[o].components[c].1 {
            Component::Script { class, .. } => class.clone(),
            Component::Transform(_) => "Transform".into(),
            Component::Rigidbody2D(_) => "Rigidbody2D".into(),
            Component::Collider2D(_) => "Collider2D".into(),
            Component::Joint2D(_) => "Joint2D".into(),
            Component::SpriteRenderer(_) => "SpriteRenderer".into(),
            Component::PointEffector2D(_) => "PointEffector2D".into(),
            Component::ParentConstraint(_) => "ParentConstraint".into(),
            Component::AudioSource { .. } => "AudioSource".into(),
            Component::WindZone(_) => "WindZone".into(),
            Component::ForceField(_) => "ParticleSystemForceField".into(),
            Component::Other(198) => "ParticleSystem".into(),
            Component::Other(120) => "LineRenderer".into(),
            Component::Other(k) => format!("Unity class {k}"),
        },
    }
}

/// `unimplemented!` text for a call the rewrite has no exact method for.
pub(super) fn stop_for(prefab: &Prefab, file: usize, call: &Value, what: &str) -> &'static str {
    let method = call.get("m_MethodName").and_then(|m| if let Value::Str(s) = m { Some(s.as_str()) } else { None }).unwrap_or("");
    stop(format!("UnityEvent call to {}.{method} ({what})", target_class(prefab, file, call)))
}

/// One persistent call as Unity runs it: None for a null target (no runtime call) or a presentation-only call.
pub(super) fn call(prefab: &Prefab, file: usize, inst: &Instance, call: &Value, what: &str) -> Result<Option<EventCall>, String> {
    if call.get("m_Target").and_then(Value::pptr).ok_or("m_Target")?.is_null() {
        return Ok(None);
    }
    let one = Value::Struct(vec![("m_PersistentCalls".into(), Value::Struct(vec![("m_Calls".into(), Value::Array(vec![call.clone()]))]))]);
    Ok(match try_event_calls(prefab, file, &one, inst, true)? {
        Ok(kept) => kept.into_iter().next(),
        Err(_) => Some(EventCall::Stop(stop_for(prefab, file, call, what))),
    })
}

/// A UnityEvent's persistent calls, in the order Invoke runs them. `what` names the event (Class.Field).
pub(super) fn calls(prefab: &Prefab, file: usize, inst: &Instance, event: &Value, what: &str) -> Result<Vec<EventCall>, String> {
    let mut out = Vec::new();
    for c in invoked_calls(event) {
        out.extend(call(prefab, file, inst, c, what)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use crate::assets::classes::MonoBehaviour;
    use crate::assets::db::{AssetDb, ObjRef};
    use crate::assets::mono::Value;

    fn walk<'a>(v: &'a Value, path: &str, out: &mut Vec<(String, &'a Value)>) {
        match v {
            Value::Struct(fields) => {
                if let Some(calls) = v.get("m_PersistentCalls").and_then(|c| c.get("m_Calls")).and_then(Value::array) {
                    out.extend(calls.iter().map(|c| (path.to_string(), c)));
                }
                for (n, x) in fields {
                    walk(x, &format!("{path}.{n}"), out);
                }
            }
            Value::Array(xs) => xs.iter().for_each(|x| walk(x, &format!("{path}[]"), out)),
            _ => {}
        }
    }

    /// Every persistent call serialised in the game's files: (target class, method, m_Mode, m_CallState), then where.
    /// Writes PPG_EVENT_INVENTORY when set.
    #[test]
    fn persistent_call_inventory() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let Ok(out_path) = std::env::var("PPG_EVENT_INVENTORY") else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let mut rows = std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
        for file in 0..db.files.len() {
            let ids: Vec<i64> = db.files[file].objects.iter().filter(|o| o.class_id == 114).map(|o| o.path_id).collect();
            for path_id in ids {
                let r = ObjRef { file, path_id };
                let Ok(Some((script, _, fields))) = db.mono(r) else { continue };
                let mut calls = Vec::new();
                walk(&fields, &script.class_name, &mut calls);
                for (path, call) in calls {
                    let target = call.get("m_Target").and_then(Value::pptr).unwrap();
                    let t = match db.resolve(file, target) {
                        None => "null".to_string(),
                        Some(t) => match db.class(t) {
                            114 => db.script(t).ok().flatten().map_or("Mono?".into(), |s| s.class_name),
                            c => format!("#{c}"),
                        },
                    };
                    let method = call.get("m_MethodName").and_then(|m| if let Value::Str(s) = m { Some(s.clone()) } else { None }).unwrap_or_default();
                    let mode = call.get("m_Mode").and_then(Value::i64).unwrap_or(-1);
                    let state = call.get("m_CallState").and_then(Value::i64).unwrap_or(-1);
                    let a = call.get("m_Arguments").unwrap();
                    let arg = match mode {
                        2 => format!("obj={:?} {:?}", a.get("m_ObjectArgument").and_then(Value::pptr), a.get("m_ObjectArgumentAssemblyTypeName")),
                        3 => format!("{:?}", a.get("m_IntArgument")),
                        4 => format!("{:?}", a.get("m_FloatArgument")),
                        5 => format!("{:?}", a.get("m_StringArgument")),
                        6 => format!("{:?}", a.get("m_BoolArgument")),
                        _ => String::new(),
                    };
                    let arg = match db.resolve(file, target).filter(|&t| db.class(t) == 1 && method == "SetActive") {
                        Some(t) => {
                            let sub = db.prefab(t).unwrap();
                            let physics = sub.objects.iter().any(|o| o.components.iter().any(|(_, k)| matches!(k, crate::assets::db::Component::Rigidbody2D(_) | crate::assets::db::Component::Collider2D(_) | crate::assets::db::Component::Joint2D(_))));
                            format!("{arg} physics={physics}")
                        }
                        None => match db.resolve(file, target).filter(|&t| db.class(t) == 212) {
                            Some(t) => format!("{arg} sprite_null={}", crate::assets::components::SpriteRenderer::read(db.data(t)).unwrap().sprite.is_null()),
                            None => arg,
                        },
                    };
                    let owner_go =MonoBehaviour::read(db.data(r)).ok().and_then(|m| db.resolve(file, m.game_object)).and_then(|g| db.game_object(g).ok()).map(|g| g.name).unwrap_or_default();
                    rows.entry(format!("{t}\t{method}\tmode={mode}\tstate={state}")).or_default().insert(format!("{path} on '{owner_go}' file {file} {arg}"));
                }
            }
        }
        let mut s = String::new();
        for (k, v) in &rows {
            s += &format!("{k}\t({} uses)\n", v.len());
            for w in v.iter().take(40) {
                s += &format!("    {w}\n");
            }
        }
        std::fs::write(out_path, s).unwrap();
    }
}
