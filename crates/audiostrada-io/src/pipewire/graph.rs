//! Mirror of the PipeWire objects needed to list devices and link the filter.

use std::collections::HashMap;

use crate::DeviceInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Node {
    pub name: String,
    pub description: String,
    pub class: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Port {
    pub node: u32,
    pub output: bool,
    pub monitor: bool,
    pub index: u32,
}

#[derive(Debug, Default)]
pub(super) struct Graph {
    pub nodes: HashMap<u32, Node>,
    pub ports: HashMap<u32, Port>,
    pub default_sink: Option<String>,
    pub default_source: Option<String>,
    pub configured_sink: Option<String>,
    pub configured_source: Option<String>,
    pub rate: Option<u32>,
}

impl Graph {
    /// Forgets an object; returns true if it was a device node.
    pub fn remove(&mut self, id: u32) -> bool {
        self.ports.remove(&id);
        self.nodes
            .remove(&id)
            .is_some_and(|n| is_device_class(&n.class))
    }

    pub fn has_node(&self, id: u32) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Default capture device: the session default if it really captures, else the configured
    /// one, else the first source.
    pub fn default_input(&self) -> Option<u32> {
        self.default_device(&[&self.default_source, &self.configured_source], true)
    }

    pub fn default_output(&self) -> Option<u32> {
        self.default_device(&[&self.default_sink, &self.configured_sink], false)
    }

    fn default_device(&self, candidates: &[&Option<String>], capture: bool) -> Option<u32> {
        let usable = |id: u32| !self.node_ports(id, capture).is_empty();
        candidates
            .iter()
            .filter_map(|name| self.find_device(name.as_deref()?))
            .find(|&id| usable(id))
            .or_else(|| {
                let mut ids: Vec<u32> = self
                    .nodes
                    .iter()
                    .filter(|(_, n)| is_device_class(&n.class))
                    .map(|(&id, _)| id)
                    .filter(|&id| usable(id))
                    .collect();
                ids.sort_unstable();
                ids.first().copied()
            })
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        let (default_input, default_output) = (self.default_input(), self.default_output());
        let mut devices: Vec<DeviceInfo> = self
            .nodes
            .iter()
            .filter(|(_, n)| is_device_class(&n.class))
            .map(|(&id, n)| DeviceInfo {
                id: n.name.clone(),
                name: n.description.clone(),
                inputs: self.node_ports(id, true).len() as u32,
                outputs: self.node_ports(id, false).len() as u32,
                is_default_input: default_input == Some(id),
                is_default_output: default_output == Some(id),
            })
            .collect();
        devices.sort_by(|a, b| a.name.cmp(&b.name));
        devices
    }

    pub fn find_device(&self, name: &str) -> Option<u32> {
        self.nodes
            .iter()
            .find(|(_, n)| n.name == name && is_device_class(&n.class))
            .map(|(&id, _)| id)
    }

    pub fn is_device(&self, node: u32) -> bool {
        self.nodes
            .get(&node)
            .is_some_and(|n| is_device_class(&n.class))
    }

    pub fn description(&self, node: u32) -> &str {
        self.nodes
            .get(&node)
            .map(|n| n.description.as_str())
            .unwrap_or("?")
    }

    /// Non-monitor ports of `node` in one direction, ordered by channel.
    pub fn node_ports(&self, node: u32, output: bool) -> Vec<u32> {
        let mut ports: Vec<(u32, u32)> = self
            .ports
            .iter()
            .filter(|(_, p)| p.node == node && p.output == output && !p.monitor)
            .map(|(&id, p)| (p.index, id))
            .collect();
        ports.sort_unstable();
        ports.into_iter().map(|(_, id)| id).collect()
    }
}

fn is_device_class(class: &str) -> bool {
    class.starts_with("Audio/Sink") || class.starts_with("Audio/Source") || class == "Audio/Duplex"
}

/// Extracts the node name from a default device metadata value like `{"name":"alsa_output..."}`.
pub(super) fn parse_default_name(value: &str) -> Option<String> {
    let rest = value.split("\"name\"").nth(1)?;
    let start = rest.find('"')? + 1;
    let len = rest[start..].find('"')?;
    Some(rest[start..start + len].to_owned())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(in crate::pipewire) fn graph() -> Graph {
        let mut g = Graph::default();
        let node = |name: &str, class: &str| Node {
            name: name.into(),
            description: name.to_uppercase(),
            class: class.into(),
        };
        g.nodes.insert(10, node("sink", "Audio/Sink"));
        g.nodes.insert(11, node("mic", "Audio/Source"));
        g.nodes.insert(12, node("player", "Stream/Output/Audio"));
        let port = |node, output, monitor, index| Port {
            node,
            output,
            monitor,
            index,
        };
        g.ports.insert(101, port(10, false, false, 1));
        g.ports.insert(100, port(10, false, false, 0));
        g.ports.insert(102, port(10, true, true, 0));
        g.ports.insert(110, port(11, true, false, 0));
        g.default_sink = Some("sink".into());
        g.default_source = Some("sink".into());
        g
    }

    #[test]
    fn devices_skip_streams_and_monitors() {
        let devices = graph().devices();
        assert_eq!(devices.len(), 2);
        assert_eq!(
            (
                devices[0].id.as_str(),
                devices[0].inputs,
                devices[0].outputs
            ),
            ("mic", 1, 0)
        );
        assert_eq!(
            (
                devices[1].id.as_str(),
                devices[1].inputs,
                devices[1].outputs
            ),
            ("sink", 0, 2)
        );
        assert!(devices[1].is_default_output && !devices[1].is_default_input);
        assert!(devices[0].is_default_input);
    }

    #[test]
    fn default_input_prefers_configured_source() {
        let mut g = graph();
        g.nodes.insert(
            13,
            Node {
                name: "usb".into(),
                description: "USB".into(),
                class: "Audio/Source".into(),
            },
        );
        g.ports.insert(
            130,
            Port {
                node: 13,
                output: true,
                monitor: false,
                index: 0,
            },
        );
        assert_eq!(g.default_input(), Some(11));
        g.configured_source = Some("usb".into());
        assert_eq!(g.default_input(), Some(13));
        assert_eq!(g.default_output(), Some(10));
    }

    #[test]
    fn ports_are_ordered_by_channel() {
        let g = graph();
        assert_eq!(g.node_ports(10, false), vec![100, 101]);
        assert_eq!(g.find_device("player"), None);
        assert_eq!(g.find_device("mic"), Some(11));
    }

    #[test]
    fn removing_objects_reports_devices() {
        let mut g = graph();
        assert!(g.is_device(10) && !g.is_device(12));
        assert!(!g.remove(101));
        assert!(!g.remove(12));
        assert!(g.remove(10));
        assert!(!g.has_node(10) && !g.is_device(10));
        assert_eq!(g.description(10), "?");
        assert_eq!(g.description(11), "MIC");
    }

    #[test]
    fn default_name_from_metadata() {
        assert_eq!(
            parse_default_name(r#"{"name":"alsa_output.pci-0000_00_1f.3.analog-stereo"}"#),
            Some("alsa_output.pci-0000_00_1f.3.analog-stereo".into())
        );
        assert_eq!(parse_default_name(r#"{ "name": "x" }"#), Some("x".into()));
        assert_eq!(parse_default_name("{}"), None);
    }
}
