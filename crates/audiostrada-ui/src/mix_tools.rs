//! Mixing tools for agents: presets and chains, automation, routing (sends, folders, VCAs,
//! groups, sidechains), the console colour, mix snapshots, and third-party plugins.

use audiostrada_core::automation::{self, AutomationTarget, CurveShape};
use audiostrada_core::console::{Console, ConsoleModel};
use audiostrada_core::routing::{self, Channel};
use audiostrada_core::{InsertId, TrackId};
use serde_json::{Value, json};

use crate::control::{seconds_to_samples, tool, track_id, track_ref};
use crate::document::Document;

pub fn definitions(live: bool) -> Vec<Value> {
    let track = json!({"description": "track id or name, or \"master\""});
    let insert = json!({"type": "integer", "description": "insert id, from inspect_project"});
    let target = json!({"description": "\"gain\" (dB), \"pan\" (-1 to 1), \"mute\" (0/1), {\"send\": track} (dB), or {\"insert\": id, \"param\": name} (the parameter's own unit; plugins: parameter number)"});
    let mut tools = vec![
        tool(
            "list_presets",
            "Presets of a built-in effect or instrument (factory and the user's): e.g. the synth's sounds, the drum machine's kits.",
            json!({"effect": {"type": "string", "description": "effect id, from list_effects"}}),
            &["effect"],
        ),
        tool(
            "load_preset",
            "Load a preset by name into an insert (one undo step).",
            json!({"track": track, "insert": insert, "name": {"type": "string"}}),
            &["track", "insert", "name"],
        ),
        tool(
            "save_preset",
            "Save an insert's settings as a user preset of its effect.",
            json!({"track": track, "insert": insert, "name": {"type": "string"}}),
            &["track", "insert", "name"],
        ),
        tool(
            "list_chains",
            "Saved effect chains (factory and the user's) with their effects.",
            json!({}),
            &[],
        ),
        tool(
            "add_chain",
            "Add a saved chain's effects at the end of a track's inserts.",
            json!({"track": track, "name": {"type": "string"}}),
            &["track", "name"],
        ),
        tool(
            "read_automation",
            "The automation lanes of a track (or the master): what each controls and its points [seconds, value, shape].",
            json!({"track": track}),
            &["track"],
        ),
        tool(
            "write_automation",
            "Write automation points on a lane: `replace` (default) clears the lane between the first and last point given, `add` keeps it, `clear` empties the lane. Points: [seconds, value, shape?] or {at, value, shape}; shape is linear (default), step or smooth. The lane is shown and switched on.",
            json!({"track": track, "target": target, "points": {"type": "array"}, "mode": {"type": "string", "enum": ["replace", "add", "clear"]}}),
            &["track", "target"],
        ),
        tool(
            "set_send",
            "Change a send between two tracks: level (dB), pre-fader, mute; or remove it.",
            json!({"from": track, "to": track, "gain_db": {"type": "number"}, "pre_fader": {"type": "boolean"}, "mute": {"type": "boolean"}, "remove": {"type": "boolean"}}),
            &["from", "to"],
        ),
        tool(
            "set_routing",
            "Route a track: into a folder or bus track (`parent`, null for none), under a VCA (`vca`, null for none), into a group (`group` index from inspect, null for none), feeding the master or not (`to_master`).",
            json!({"track": track, "parent": {}, "vca": {}, "group": {}, "to_master": {"type": "boolean"}}),
            &["track"],
        ),
        tool(
            "set_sidechain",
            "Feed an insert's sidechain (a compressor ducking under the kick) from another track, or none.",
            json!({"track": track, "insert": insert, "source": {"description": "track, or null"}}),
            &["track", "insert"],
        ),
        tool(
            "set_console",
            "The console colour of the project (track omitted, or \"master\") or of one track: model clean, iron (British Warm), imperial (British Bright), discrete (American Punch) or velvet (Nashville), amount 0-100; `off` switches every console colour off.",
            json!({"track": track, "model": {"type": "string", "enum": ["clean", "iron", "imperial", "discrete", "velvet", "none"]}, "amount": {"type": "number"}, "off": {"type": "boolean"}}),
            &[],
        ),
        tool(
            "snapshot",
            "Mix snapshots: `save` the console as it is under a name, `recall` one, `list` them.",
            json!({"action": {"type": "string", "enum": ["save", "recall", "list"]}, "name": {"type": "string"}}),
            &["action"],
        ),
        tool(
            "list_plugins",
            "Third-party plugins installed (CLAP, VST3, LV2, AU): their key for add_insert, name, vendor, instrument or effect.",
            json!({"filter": {"type": "string"}}),
            &[],
        ),
    ];
    if live {
        tools.push(tool("plugin_params", "The parameters of a third-party plugin insert, with their number, range and current value; set them with set_insert, by number.", json!({"track": track, "insert": insert, "filter": {"type": "string"}}), &["track", "insert"]));
    }
    tools
}

fn insert_of(doc: &Document, args: &Value) -> Result<(Channel, InsertId), String> {
    let channel = track_ref(doc, args.get("track"))?;
    let id = InsertId(
        args.get("insert")
            .and_then(Value::as_u64)
            .ok_or("`insert` id is needed")?,
    );
    routing::find_insert(doc.project(), channel, id).ok_or(format!("no insert {} there", id.0))?;
    Ok((channel, id))
}

fn target(doc: &Document, value: Option<&Value>) -> Result<AutomationTarget, String> {
    let v = value.ok_or("`target` is needed")?;
    match v.as_str() {
        Some("gain") => return Ok(AutomationTarget::Gain),
        Some("pan") => return Ok(AutomationTarget::Pan),
        Some("mute") => return Ok(AutomationTarget::Mute),
        _ => {}
    }
    if let Some(to) = v.get("send") {
        return Ok(AutomationTarget::Send(track_id(doc, Some(to))?));
    }
    if let (Some(insert), Some(param)) = (v.get("insert").and_then(Value::as_u64), v.get("param")) {
        let param = match param {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        return Ok(AutomationTarget::Param {
            insert: InsertId(insert),
            param,
        });
    }
    Err("`target`: \"gain\", \"pan\", \"mute\", {\"send\": track} or {\"insert\": id, \"param\": name}".into())
}

fn describe_target(doc: &Document, t: &AutomationTarget) -> Value {
    match t {
        AutomationTarget::Gain => json!("gain"),
        AutomationTarget::Pan => json!("pan"),
        AutomationTarget::Mute => json!("mute"),
        AutomationTarget::Send(to) => {
            json!({"send": doc.project().track(*to).map(|t| t.name.clone())})
        }
        AutomationTarget::Param { insert, param } => json!({"insert": insert.0, "param": param}),
    }
}

fn shape(name: Option<&str>) -> CurveShape {
    match name {
        Some("step") => CurveShape::Step,
        Some("smooth") => CurveShape::Smooth,
        _ => CurveShape::Linear,
    }
}

fn shape_name(s: CurveShape) -> &'static str {
    match s {
        CurveShape::Linear => "linear",
        CurveShape::Step => "step",
        CurveShape::Smooth => "smooth",
    }
}

fn console_model(name: &str) -> Option<ConsoleModel> {
    Some(match name {
        "clean" => ConsoleModel::Clean,
        "iron" => ConsoleModel::Iron,
        "imperial" => ConsoleModel::Imperial,
        "discrete" => ConsoleModel::Discrete,
        "velvet" => ConsoleModel::Velvet,
        _ => return None,
    })
}

/// An optional track: absent, a track, or null (none).
fn maybe_track(doc: &Document, args: &Value, key: &str) -> Result<Option<Option<TrackId>>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => Ok(Some(Some(track_id(doc, Some(v))?))),
    }
}

pub fn call(doc: &mut Document, name: &str, args: &Value) -> Option<Result<Value, String>> {
    let rate = doc.project().sample_rate;
    let string = |k: &str| args.get(k).and_then(Value::as_str).map(str::to_owned);
    let result = (|| -> Result<Value, String> {
        match name {
            "list_presets" => {
                let effect = string("effect").ok_or("`effect` is needed")?;
                let list = crate::presets::list(crate::presets::root().as_deref(), &effect);
                Ok(json!(
                    list.iter()
                        .map(|p| json!({"name": p.name, "factory": p.factory}))
                        .collect::<Vec<_>>()
                ))
            }
            "load_preset" => {
                let (channel, id) = insert_of(doc, args)?;
                let wanted = string("name").ok_or("`name` is needed")?;
                let effect = routing::find_insert(doc.project(), channel, id)
                    .map(|i| i.effect.clone())
                    .unwrap_or_default();
                let list = crate::presets::list(crate::presets::root().as_deref(), &effect);
                let preset = list
                    .iter()
                    .find(|p| p.name == wanted)
                    .or_else(|| list.iter().find(|p| p.name.eq_ignore_ascii_case(&wanted)))
                    .ok_or(format!(
                        "no preset {wanted} for {effect} (see list_presets)"
                    ))?;
                let state = preset
                    .state
                    .as_deref()
                    .and_then(audiostrada_core::base64::decode);
                doc.history_mut()
                    .edit(&format!("Preset {}", preset.name), |p| {
                        if let Some(i) = routing::find_insert_mut(p, channel, id) {
                            i.params = preset.params.clone();
                            if preset.state.is_some() {
                                i.state = preset.state.clone();
                            }
                        }
                    });
                if let (Some(state), Some(plugin)) = (state, doc.player_mut().plugin(channel, id)) {
                    plugin.load_state(&state);
                }
                Ok(json!({"loaded": preset.name}))
            }
            "save_preset" => {
                let (channel, id) = insert_of(doc, args)?;
                let name = string("name")
                    .filter(|n| !n.trim().is_empty())
                    .ok_or("`name` is needed")?;
                let insert = routing::find_insert(doc.project(), channel, id)
                    .cloned()
                    .ok_or("no such insert")?;
                let root = crate::presets::root().ok_or("no configuration folder")?;
                let preset = crate::presets::Preset {
                    name: name.clone(),
                    params: insert.params,
                    state: insert.state,
                    factory: false,
                };
                crate::presets::save(&root, &insert.effect, &preset)?;
                Ok(json!({"saved": name, "effect": insert.effect}))
            }
            "list_chains" => {
                let chains = crate::chains::list(crate::chains::root().as_deref());
                Ok(json!(chains.iter().map(|c| json!({"name": c.name, "effects": c.inserts.iter().map(|i| i.effect.clone()).collect::<Vec<_>>()})).collect::<Vec<_>>()))
            }
            "add_chain" => {
                let channel = track_ref(doc, args.get("track"))?;
                let wanted = string("name").ok_or("`name` is needed")?;
                let chains = crate::chains::list(crate::chains::root().as_deref());
                let chain = chains
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(&wanted))
                    .ok_or(format!("no chain {wanted} (see list_chains)"))?;
                let mut added = Vec::new();
                doc.history_mut()
                    .edit(&format!("Add chain {}", chain.name), |p| {
                        for insert in &chain.inserts {
                            added.extend(routing::add_insert_copy(p, channel, insert));
                        }
                    });
                if added.is_empty() {
                    return Err("this track cannot hold effects".into());
                }
                Ok(json!({"inserts": added.iter().map(|i| i.0).collect::<Vec<_>>()}))
            }
            "read_automation" => {
                let channel = track_ref(doc, args.get("track"))?;
                let project = doc.project();
                let lanes = match channel {
                    Channel::Track(t) => project
                        .track(t)
                        .map(|t| t.automation.clone())
                        .unwrap_or_default(),
                    Channel::Master => project.master.automation.clone(),
                };
                let secs = |s: u64| (s as f64 / f64::from(rate) * 1000.0).round() / 1000.0;
                Ok(json!(lanes
                    .iter()
                    .map(|l| json!({
                        "target": describe_target(doc, &l.target),
                        "enabled": l.enabled,
                        "points": l.points.iter().map(|p| json!([secs(p.time), p.value, shape_name(p.shape)])).collect::<Vec<_>>(),
                    }))
                    .collect::<Vec<_>>()))
            }
            "write_automation" => {
                let channel = track_ref(doc, args.get("track"))?;
                let target = target(doc, args.get("target"))?;
                if let AutomationTarget::Param { insert, .. } = &target
                    && routing::find_insert(doc.project(), channel, *insert).is_none()
                {
                    return Err(format!("no insert {} on that track", insert.0));
                }
                let mode = string("mode").unwrap_or_else(|| "replace".into());
                let mut points = Vec::new();
                for p in args
                    .get("points")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let at = seconds_to_samples(p.get("at").or_else(|| p.get(0)), rate)
                        .ok_or("each point needs a time (seconds)")?;
                    let value = p
                        .get("value")
                        .or_else(|| p.get(1))
                        .and_then(Value::as_f64)
                        .filter(|v| v.is_finite())
                        .ok_or("each point needs a value")? as f32;
                    let s = shape(p.get("shape").or_else(|| p.get(2)).and_then(Value::as_str));
                    points.push((at, value, s));
                }
                if points.is_empty() && mode != "clear" {
                    return Err("`points` are needed".into());
                }
                let count = points.len();
                doc.history_mut().edit("Write automation", |p| {
                    let lane = match channel {
                        Channel::Track(t) => {
                            p.track_mut(t).map(|t| automation::lane_mut(t, &target))
                        }
                        Channel::Master => {
                            Some(automation::master_lane_mut(&mut p.master, &target))
                        }
                    };
                    let Some(lane) = lane else { return };
                    match mode.as_str() {
                        "clear" => lane.points.clear(),
                        "add" => {}
                        _ => {
                            let (first, last) = (
                                points.iter().map(|p| p.0).min().unwrap_or(0),
                                points.iter().map(|p| p.0).max().unwrap_or(0),
                            );
                            lane.remove_range(first, last + 1);
                        }
                    }
                    for (at, value, s) in &points {
                        lane.add(*at, *value, *s);
                    }
                    lane.enabled = true;
                    lane.visible = true;
                });
                Ok(json!({"points_written": count}))
            }
            "set_send" => {
                let (from, to) = (
                    track_id(doc, args.get("from"))?,
                    track_id(doc, args.get("to"))?,
                );
                let exists = doc
                    .project()
                    .track(from)
                    .is_some_and(|t| t.sends.iter().any(|s| s.target == to));
                if args.get("remove").and_then(Value::as_bool) == Some(true) {
                    doc.history_mut()
                        .edit("Remove send", |p| routing::remove_send(p, from, to));
                    return Ok(json!("removed"));
                }
                if !exists {
                    let mut result = Ok(());
                    doc.history_mut()
                        .edit("Add send", |p| result = routing::add_send(p, from, to));
                    result.map_err(|e| format!("{e:?}"))?;
                }
                let gain = args
                    .get("gain_db")
                    .and_then(Value::as_f64)
                    .map(|v| v as f32);
                let pre = args.get("pre_fader").and_then(Value::as_bool);
                let mute = args.get("mute").and_then(Value::as_bool);
                doc.history_mut().edit("Change send", |p| {
                    if let Some(s) = p
                        .track_mut(from)
                        .and_then(|t| t.sends.iter_mut().find(|s| s.target == to))
                    {
                        if let Some(g) = gain {
                            s.gain_db = g.clamp(-120.0, 12.0);
                        }
                        if let Some(b) = pre {
                            s.pre_fader = b;
                        }
                        if let Some(m) = mute {
                            s.mute = m;
                        }
                    }
                });
                Ok(json!("ok"))
            }
            "set_routing" => {
                let track = track_id(doc, args.get("track"))?;
                let parent = maybe_track(doc, args, "parent")?;
                let vca = maybe_track(doc, args, "vca")?;
                let group = match args.get("group") {
                    None => None,
                    Some(Value::Null) => Some(None),
                    Some(v) => {
                        let g = v
                            .as_u64()
                            .ok_or("`group`: an index from inspect_project, or null")?
                            as usize;
                        if g >= doc.project().groups.len() {
                            return Err(format!("no group {g}"));
                        }
                        Some(Some(g))
                    }
                };
                let to_master = args.get("to_master").and_then(Value::as_bool);
                let mut problem = None;
                doc.history_mut().edit("Routing", |p| {
                    if let Some(parent) = parent
                        && let Err(e) = routing::set_parent(p, track, parent)
                    {
                        problem = Some(format!("parent: {e:?}"));
                    }
                    if let Some(vca) = vca
                        && let Err(e) = routing::set_vca(p, track, vca)
                    {
                        problem = Some(format!("vca: {e:?}"));
                    }
                    if let Some(g) = group {
                        audiostrada_core::groups::set_group(p, track, g);
                    }
                    if let Some(on) = to_master
                        && let Err(e) = routing::set_master_send(p, track, on)
                    {
                        problem = Some(format!("to_master: {e:?}"));
                    }
                });
                problem.map_or(Ok(json!("ok")), Err)
            }
            "set_sidechain" => {
                let (channel, id) = insert_of(doc, args)?;
                let source = match args.get("source") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(track_id(doc, Some(v))?),
                };
                doc.history_mut().edit("Sidechain", |p| {
                    if let Some(i) = routing::find_insert_mut(p, channel, id) {
                        i.sidechain = source;
                    }
                });
                Ok(json!("ok"))
            }
            "set_console" => {
                let model = string("model");
                let amount = args
                    .get("amount")
                    .and_then(Value::as_f64)
                    .map(|a| (a as f32).clamp(0.0, 100.0));
                let off = args.get("off").and_then(Value::as_bool);
                let channel = match args.get("track") {
                    Some(v) => track_ref(doc, Some(v))?,
                    None => Channel::Master,
                };
                let parsed = match model.as_deref() {
                    Some("none") | None => None,
                    Some(m) => Some(console_model(m).ok_or(format!("unknown console {m}"))?),
                };
                doc.history_mut().edit("Console", |p| {
                    if let Some(o) = off {
                        p.console_off = o;
                    }
                    match channel {
                        Channel::Master => {
                            if let Some(m) = parsed {
                                p.console.model = m;
                            }
                            if let Some(a) = amount {
                                p.console.amount = a;
                            }
                        }
                        Channel::Track(t) => {
                            if let Some(track) = p.track_mut(t) {
                                if model.as_deref() == Some("none") {
                                    track.console = None;
                                } else if parsed.is_some() || amount.is_some() {
                                    let mut c = track.console.unwrap_or(Console::default());
                                    if let Some(m) = parsed {
                                        c.model = m;
                                    }
                                    if let Some(a) = amount {
                                        c.amount = a;
                                    }
                                    track.console = Some(c);
                                }
                            }
                        }
                    }
                });
                Ok(json!("ok"))
            }
            "snapshot" => {
                let action = string("action").unwrap_or_default();
                match action.as_str() {
                    "list" => Ok(json!(
                        doc.project()
                            .snapshots
                            .iter()
                            .map(|s| s.name.clone())
                            .collect::<Vec<_>>()
                    )),
                    "save" => {
                        let name = string("name").ok_or("`name` is needed")?;
                        doc.history_mut().edit("Save snapshot", |p| {
                            let snap = audiostrada_core::snapshot::MixSnapshot::capture(p, &name);
                            p.snapshots.retain(|s| s.name != name);
                            p.snapshots.push(snap);
                        });
                        Ok(json!({"saved": name}))
                    }
                    "recall" => {
                        let name = string("name").ok_or("`name` is needed")?;
                        let snap = doc
                            .project()
                            .snapshots
                            .iter()
                            .find(|s| s.name == name)
                            .cloned()
                            .ok_or(format!("no snapshot {name}"))?;
                        doc.history_mut()
                            .edit("Recall snapshot", |p| snap.recall(p));
                        Ok(json!({"recalled": name}))
                    }
                    _ => Err("`action`: save, recall or list".into()),
                }
            }
            "list_plugins" => {
                let filter = string("filter").map(|f| f.to_lowercase());
                let list: Vec<Value> = doc
                    .player()
                    .plugins()
                    .plugins()
                    .filter(|p| filter.as_ref().is_none_or(|f| p.name.to_lowercase().contains(f) || p.vendor.to_lowercase().contains(f)))
                    .take(300)
                    .map(|p| json!({"key": p.key(), "name": p.name, "vendor": p.vendor, "instrument": p.instrument}))
                    .collect();
                Ok(json!(list))
            }
            "plugin_params" => {
                let (channel, id) = insert_of(doc, args)?;
                let stored = routing::find_insert(doc.project(), channel, id)
                    .map(|i| i.params.clone())
                    .unwrap_or_default();
                let filter = string("filter").map(|f| f.to_lowercase());
                let plugin = doc.player_mut().plugin(channel, id).ok_or(
                    "not a loaded third-party plugin (built-in effects: see list_effects)",
                )?;
                let params: Vec<Value> = plugin
                    .params()
                    .into_iter()
                    .filter(|p| !p.hidden && filter.as_ref().is_none_or(|f| p.name.to_lowercase().contains(f)))
                    .take(400)
                    .map(|p| {
                        let value = stored.get(&p.id.to_string()).map(|v| f64::from(*v)).or_else(|| plugin.param_value(p.id));
                        json!({"param": p.id, "name": p.name, "min": p.min, "max": p.max, "value": value})
                    })
                    .collect();
                Ok(json!(params))
            }
            _ => Err(String::new()),
        }
    })();
    // An empty error: not one of these tools.
    match result {
        Err(e) if e.is_empty() => None,
        r => Some(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::AppDirs;

    fn run(doc: &mut Document, name: &str, args: Value) -> Result<Value, String> {
        call(doc, name, &args).expect("a mix tool")
    }

    #[test]
    fn presets_automation_routing_console_and_snapshots() {
        let mut doc = Document::new(AppDirs::default());
        let (mut keys, mut bus, mut insert) = (None, None, None);
        doc.history_mut().edit("Setup", |p| {
            let t = p.add_track("Keys");
            insert = routing::add_insert(p, Channel::Track(t), "synth", Default::default());
            keys = Some(t);
            bus = Some(p.add_track("Bus"));
        });
        let (keys, insert) = (keys.unwrap(), insert.unwrap());
        let presets = run(&mut doc, "list_presets", json!({"effect": "synth"})).unwrap();
        assert!(
            presets
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == "Trap: 808 glide")
        );
        run(
            &mut doc,
            "load_preset",
            json!({"track": "Keys", "insert": insert.0, "name": "trap: 808 glide"}),
        )
        .unwrap();
        let params = &routing::find_insert(doc.project(), Channel::Track(keys), insert)
            .unwrap()
            .params;
        assert_eq!(params.get("glide"), Some(&90.0));
        assert!(
            run(
                &mut doc,
                "load_preset",
                json!({"track": "Keys", "insert": insert.0, "name": "nope"})
            )
            .is_err()
        );

        run(
            &mut doc,
            "write_automation",
            json!({"track": "Keys", "target": "gain", "points": [[0, -12], [2, 0, "smooth"]]}),
        )
        .unwrap();
        run(&mut doc, "write_automation", json!({"track": "Keys", "target": {"insert": insert.0, "param": "cutoff"}, "points": [{"at": 1, "value": 400}]})).unwrap();
        let lanes = run(&mut doc, "read_automation", json!({"track": "Keys"})).unwrap();
        assert_eq!(
            lanes[0]["points"],
            json!([[0.0, -12.0, "linear"], [2.0, 0.0, "smooth"]])
        );
        assert_eq!(
            lanes[1]["target"],
            json!({"insert": insert.0, "param": "cutoff"})
        );

        run(
            &mut doc,
            "set_send",
            json!({"from": "Keys", "to": "Bus", "gain_db": -6, "pre_fader": true}),
        )
        .unwrap();
        let send = doc.project().track(keys).unwrap().sends[0].clone();
        assert_eq!(
            (send.target, send.gain_db, send.pre_fader),
            (bus.unwrap(), -6.0, true)
        );
        run(
            &mut doc,
            "set_send",
            json!({"from": "Keys", "to": "Bus", "remove": true}),
        )
        .unwrap();
        assert!(doc.project().track(keys).unwrap().sends.is_empty());
        run(
            &mut doc,
            "set_routing",
            json!({"track": "Keys", "parent": "Bus"}),
        )
        .unwrap();
        assert_eq!(doc.project().track(keys).unwrap().parent, bus);

        run(
            &mut doc,
            "set_console",
            json!({"model": "iron", "amount": 70}),
        )
        .unwrap();
        assert_eq!(doc.project().console.model, ConsoleModel::Iron);
        run(&mut doc, "snapshot", json!({"action": "save", "name": "A"})).unwrap();
        assert_eq!(
            run(&mut doc, "snapshot", json!({"action": "list"})).unwrap(),
            json!(["A"])
        );
        assert!(call(&mut doc, "nope", &json!({})).is_none());
    }
}
