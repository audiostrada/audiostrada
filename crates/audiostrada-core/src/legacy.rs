//! Settings written by older versions, brought to the current ones.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// The bands of the eight-band EQ of format 1, in order: their old parameter prefix, the
/// current band type (index in the EQ's types), and default frequency and Q.
const OLD_EQ: [(&str, f32, f32, f32); 8] = [
    ("hp", 3.0, 80.0, 0.707),
    ("ls", 1.0, 120.0, 0.707),
    ("p1", 0.0, 250.0, 1.0),
    ("p2", 0.0, 800.0, 1.0),
    ("p3", 0.0, 2500.0, 1.0),
    ("p4", 0.0, 6000.0, 1.0),
    ("hs", 2.0, 8000.0, 0.707),
    ("lp", 4.0, 18_000.0, 0.707),
];

/// Current id of a parameter of the old EQ (`p1_gain` is now `b3_gain`).
pub fn eq_param(old: &str) -> Option<String> {
    let (prefix, field) = old.split_once('_')?;
    let band = OLD_EQ.iter().position(|(p, ..)| *p == prefix)?;
    let field = match field {
        "on" => "used",
        "freq" | "gain" | "q" => field,
        _ => return None,
    };
    Some(format!("b{}_{field}", band + 1))
}

/// Turns the parameters of the old eight-band EQ into bands 1 to 8 of the current one (low
/// cut, low shelf, four bells, high shelf, high cut). A shelf or bell is in use when it
/// boosts or cuts, a cut when it was on. Returns whether anything was old.
pub fn upgrade_eq(params: &mut BTreeMap<String, f32>) -> bool {
    if !params.keys().any(|k| eq_param(k).is_some()) {
        return false;
    }
    let old = std::mem::take(params);
    for (b, &(prefix, kind, freq, q)) in OLD_EQ.iter().enumerate() {
        let get = |field: &str| old.get(&format!("{prefix}_{field}")).copied();
        let used = match prefix {
            "hp" | "lp" => get("on").unwrap_or(0.0) >= 0.5,
            _ => get("gain").is_some_and(|g| g.abs() > 1e-3),
        };
        let band = |field: &str| format!("b{}_{field}", b + 1);
        params.insert(band("used"), if used { 1.0 } else { 0.0 });
        params.insert(band("type"), kind);
        params.insert(band("freq"), get("freq").unwrap_or(freq));
        params.insert(band("q"), get("q").unwrap_or(q));
        if let Some(gain) = get("gain") {
            params.insert(band("gain"), gain);
        }
    }
    params.extend(
        old.into_iter()
            .filter(|(k, _)| eq_param(k).is_none() && !k.ends_with("_on")),
    );
    true
}

/// Format 1 to 2: the EQ's bands became free bands.
pub(crate) fn v1_to_v2(mut value: Value) -> Value {
    let Some(project) = value.get_mut("project") else {
        return value;
    };
    let mut eqs = BTreeSet::new();
    for insert in eq_inserts(project) {
        if let Some(id) = insert.get("id").and_then(Value::as_u64) {
            eqs.insert(id);
        }
        upgrade_params(insert.get_mut("params"));
    }
    if let Some(snapshots) = project.get_mut("snapshots").and_then(Value::as_array_mut) {
        for inserts in snapshots
            .iter_mut()
            .filter_map(|s| s.get_mut("inserts").and_then(Value::as_object_mut))
        {
            for (id, state) in inserts.iter_mut() {
                if id.parse().is_ok_and(|id: u64| eqs.contains(&id)) {
                    upgrade_params(state.get_mut("params"));
                }
            }
        }
    }
    // Automation and MIDI mappings of the old bands follow them, and keep them in use.
    let mut automated: Vec<(u64, String)> = Vec::new();
    rename_targets(project, &eqs, &mut automated);
    for (insert, param) in automated {
        let Some((band, _)) = param.split_once('_') else {
            continue;
        };
        for eq in eq_inserts(project)
            .into_iter()
            .filter(|i| i.get("id").and_then(Value::as_u64) == Some(insert))
        {
            if let Some(params) = eq.get_mut("params").and_then(Value::as_object_mut) {
                params.insert(format!("{band}_used"), Value::from(1.0));
            }
        }
    }
    value
}

/// The EQ inserts of the tracks and of the master.
fn eq_inserts(project: &mut Value) -> Vec<&mut Value> {
    let Some(project) = project.as_object_mut() else {
        return Vec::new();
    };
    let mut lists: Vec<&mut Value> = Vec::new();
    for (key, v) in project.iter_mut() {
        match (key.as_str(), v) {
            ("tracks", Value::Array(tracks)) => {
                lists.extend(tracks.iter_mut().filter_map(|t| t.get_mut("inserts")));
            }
            ("master", master) => lists.extend(master.get_mut("inserts")),
            _ => {}
        }
    }
    lists
        .into_iter()
        .filter_map(Value::as_array_mut)
        .flatten()
        .filter(|i| i.get("effect").and_then(Value::as_str) == Some("eq"))
        .collect()
}

fn upgrade_params(params: Option<&mut Value>) {
    let Some(params) = params else {
        return;
    };
    let Ok(mut map) = serde_json::from_value::<BTreeMap<String, f32>>(params.clone()) else {
        return;
    };
    if upgrade_eq(&mut map)
        && let Ok(new) = serde_json::to_value(map)
    {
        *params = new;
    }
}

/// Renames the old EQ parameters in every automation or MIDI target of `value`, noting the
/// insert and new parameter of each.
fn rename_targets(value: &mut Value, eqs: &BTreeSet<u64>, renamed: &mut Vec<(u64, String)>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Object(target)) = map.get_mut("Param")
                && let Some(insert) = target.get("insert").and_then(Value::as_u64)
                && eqs.contains(&insert)
                && let Some(new) = target
                    .get("param")
                    .and_then(Value::as_str)
                    .and_then(eq_param)
            {
                renamed.push((insert, new.clone()));
                target.insert("param".into(), Value::from(new));
            }
            for v in map.values_mut() {
                rename_targets(v, eqs, renamed);
            }
        }
        Value::Array(items) => {
            for v in items {
                rename_targets(v, eqs, renamed);
            }
        }
        _ => {}
    }
}

/// What Strada-Amp's amp parameter was before three of its models went (Brit 900, Brit Hot Mod,
/// Brit Lead 100): `model`, numbered with them. Each old number gives the amp now: the Plexi
/// for the two British ones, the Panama 52 for the Laney.
const AMP_OF_OLD_MODEL: [f32; 17] = [
    0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 8.0, 8.0, 9.0, 10.0, 11.0, 11.0, 12.0, 13.0,
];

/// Strada-Amp's parameters saved with its old `model` numbering, as `amp`. Returns whether
/// anything was old.
pub fn upgrade_amp_rig(params: &mut BTreeMap<String, f32>) -> bool {
    let Some(old) = params.remove("model") else {
        return false;
    };
    let index = (old.round().max(0.0) as usize).min(AMP_OF_OLD_MODEL.len() - 1);
    params
        .entry("amp".to_owned())
        .or_insert(AMP_OF_OLD_MODEL[index]);
    true
}

/// Strada-Amp's old `model` parameter in a whole project file: in its inserts (parallel
/// branches too), the console snapshots holding their values, and the automation lanes
/// driving it (renamed; their points keep their values).
pub fn upgrade_amp_rigs(value: &mut Value) {
    fn params_of(value: &mut Value) -> Option<BTreeMap<String, f32>> {
        serde_json::from_value(value.get("params")?.clone()).ok()
    }
    fn upgrade(value: &mut Value) -> bool {
        let Some(mut params) = params_of(value) else {
            return false;
        };
        if !upgrade_amp_rig(&mut params) {
            return false;
        }
        if let (Some(slot), Ok(new)) = (value.get_mut("params"), serde_json::to_value(params)) {
            *slot = new;
        }
        true
    }
    /// The ids of the rigs, their own parameters upgraded on the way.
    fn rigs(value: &mut Value, ids: &mut BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                if map.get("effect").and_then(Value::as_str) == Some("amp_rig")
                    && let Some(id) = map.get("id")
                {
                    ids.insert(id.to_string());
                }
                for child in map.values_mut() {
                    rigs(child, ids);
                }
                if map.get("effect").and_then(Value::as_str) == Some("amp_rig") {
                    let mut object = Value::Object(std::mem::take(map));
                    upgrade(&mut object);
                    if let Value::Object(upgraded) = object {
                        *map = upgraded;
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| rigs(v, ids)),
            _ => {}
        }
    }
    /// Snapshot values and lanes of those rigs.
    fn uses(value: &mut Value, ids: &BTreeSet<String>) {
        match value {
            Value::Object(map) => {
                let lane = map.get("insert").map(Value::to_string);
                if lane.is_some_and(|id| ids.contains(&id))
                    && map.get("param").and_then(Value::as_str) == Some("model")
                {
                    map.insert("param".into(), Value::from("amp"));
                }
                for (key, child) in map.iter_mut() {
                    if ids.contains(key) || ids.contains(&format!("\"{key}\"")) {
                        upgrade(child);
                    }
                    uses(child, ids);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| uses(v, ids)),
            _ => {}
        }
    }
    let mut ids = BTreeSet::new();
    rigs(value, &mut ids);
    if !ids.is_empty() {
        uses(value, &ids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_eq_settings_become_bands() {
        let mut params: BTreeMap<String, f32> = [
            ("hp_on", 1.0),
            ("hp_freq", 90.0),
            ("p3_gain", 2.5),
            ("p3_freq", 3500.0),
            ("hs_gain", 0.0),
            ("output", -1.0),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        assert!(upgrade_eq(&mut params));
        let get = |k: &str| params.get(k).copied();
        assert_eq!(get("b1_used"), Some(1.0));
        assert_eq!(get("b1_type"), Some(3.0));
        assert_eq!(get("b1_freq"), Some(90.0));
        assert_eq!(
            (get("b5_used"), get("b5_freq"), get("b5_gain")),
            (Some(1.0), Some(3500.0), Some(2.5))
        );
        assert_eq!(get("b7_used"), Some(0.0), "a shelf at 0 dB is not in use");
        assert_eq!(get("b8_used"), Some(0.0));
        assert_eq!(get("output"), Some(-1.0));
        assert!(!params.contains_key("hp_on"));
        assert!(!upgrade_eq(&mut params), "only once");
    }

    #[test]
    fn projects_of_format_1_keep_their_eq() {
        let file = serde_json::json!({
            "format": "audiostrada-project",
            "version": 1,
            "project": {
                "tracks": [{
                    "inserts": [{"id": 7, "effect": "eq", "params": {"p1_freq": 300.0}}],
                    "automation": [{"target": {"Param": {"insert": 7, "param": "p1_gain"}}}],
                }],
                "master": {"inserts": []},
                "midi_map": [{"target": {"Param": {"insert": 7, "param": "lp_freq"}}}],
                "snapshots": [{"inserts": {"7": {"params": {"p1_gain": 4.0}}}}],
            },
        });
        let v = v1_to_v2(file);
        let p = &v["project"];
        let params = &p["tracks"][0]["inserts"][0]["params"];
        assert_eq!(params["b3_freq"], 300.0);
        assert_eq!(params["b3_used"], 1.0, "its gain is automated");
        assert_eq!(
            p["tracks"][0]["automation"][0]["target"]["Param"]["param"],
            "b3_gain"
        );
        assert_eq!(p["midi_map"][0]["target"]["Param"]["param"], "b8_freq");
        assert_eq!(p["snapshots"][0]["inserts"]["7"]["params"]["b3_gain"], 4.0);
    }

    #[test]
    fn strada_amp_settings_of_old_find_their_amp() {
        let mut params = BTreeMap::from([("model".to_owned(), 9.0), ("gain".to_owned(), 5.0)]);
        assert!(upgrade_amp_rig(&mut params));
        assert_eq!(
            params.get("amp"),
            Some(&8.0),
            "the Brit 900 is now the Plexi"
        );
        assert!(!params.contains_key("model"));
        assert!(!upgrade_amp_rig(&mut params), "once");
        let mut params = BTreeMap::from([("model".to_owned(), 15.0)]);
        upgrade_amp_rig(&mut params);
        assert_eq!(params["amp"], 12.0, "the bass amp moved down");
        let mut params = BTreeMap::from([("model".to_owned(), 14.0)]);
        upgrade_amp_rig(&mut params);
        assert_eq!(params["amp"], 11.0, "the Laney is now the Panama");

        // A whole file: an insert in a parallel branch, a snapshot, a lane.
        let mut file = serde_json::json!({
            "tracks": [{"inserts": [{"id": 3, "effect": "parallel", "params": {}, "branches": [
                {"inserts": [{"id": 7, "effect": "amp_rig", "params": {"model": 12.0}}]}
            ]}, {"id": 8, "effect": "console", "params": {"model": 2.0}}],
            "automation": [{"target": {"Param": {"insert": 7, "param": "model"}}}]}],
            "snapshots": [{"inserts": {"7": {"params": {"model": 13.0}}, "8": {"params": {"model": 1.0}}}}]
        });
        upgrade_amp_rigs(&mut file);
        let rig = &file["tracks"][0]["inserts"][0]["branches"][0]["inserts"][0];
        assert_eq!(rig["params"], serde_json::json!({"amp": 10.0}));
        assert_eq!(
            file["tracks"][0]["inserts"][1]["params"]["model"], 2.0,
            "not a rig"
        );
        assert_eq!(
            file["tracks"][0]["automation"][0]["target"]["Param"]["param"],
            "amp"
        );
        assert_eq!(
            file["snapshots"][0]["inserts"]["7"]["params"],
            serde_json::json!({"amp": 11.0})
        );
        assert_eq!(file["snapshots"][0]["inserts"]["8"]["params"]["model"], 1.0);
    }
}
