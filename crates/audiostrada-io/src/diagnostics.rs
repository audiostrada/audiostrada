//! System checks for low-latency audio, each with the exact fix to apply.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Warning,
    Error,
    Unknown,
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            CheckStatus::Ok => "ok",
            CheckStatus::Warning => "warning",
            CheckStatus::Error => "error",
            CheckStatus::Unknown => "unknown",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub status: CheckStatus,
    pub detail: String,
    /// Commands or steps that resolve the problem.
    pub fix: Option<String>,
}

impl Check {
    pub fn new(name: &'static str, status: CheckStatus, detail: impl Into<String>) -> Self {
        Self {
            name,
            status,
            detail: detail.into(),
            fix: None,
        }
    }

    pub fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

/// Runs every check relevant to the current platform.
pub fn run() -> Vec<Check> {
    #[cfg(target_os = "linux")]
    {
        linux::run()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::fmt;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{Check, CheckStatus};

    const RTKIT_SERVICE: &str =
        "/usr/share/dbus-1/system-services/org.freedesktop.RealtimeKit1.service";
    const MIN_RTPRIO: u64 = 70;
    const MIN_MEMLOCK: u64 = 4 * 1024 * 1024;
    const MAX_MIN_QUANTUM: u64 = 256;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Limit {
        Value(u64),
        Unlimited,
    }

    impl fmt::Display for Limit {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Limit::Value(v) => v.fmt(f),
                Limit::Unlimited => f.write_str("unlimited"),
            }
        }
    }

    impl Limit {
        fn at_least(self, min: u64) -> bool {
            match self {
                Limit::Unlimited => true,
                Limit::Value(v) => v >= min,
            }
        }
    }

    pub(super) fn run() -> Vec<Check> {
        let limits = fs::read_to_string("/proc/self/limits").unwrap_or_default();
        vec![
            check_pipewire(),
            check_realtime(&limits),
            check_memlock(&limits),
            check_quantum(),
            check_governor(),
            check_preemption(),
        ]
    }

    fn check_pipewire() -> Check {
        let dir = std::env::var_os("PIPEWIRE_RUNTIME_DIR")
            .or_else(|| std::env::var_os("XDG_RUNTIME_DIR"))
            .map(PathBuf::from);
        let name = "PipeWire server";
        match dir {
            Some(dir) if dir.join("pipewire-0").exists() => {
                Check::new(name, CheckStatus::Ok, "running")
            }
            _ => Check::new(name, CheckStatus::Error, "PipeWire socket not found")
                .with_fix("systemctl --user enable --now pipewire pipewire-pulse wireplumber"),
        }
    }

    pub(super) fn check_realtime(limits: &str) -> Check {
        let name = "Real-time priority";
        let Some(rtprio) = parse_limit(limits, "Max realtime priority") else {
            return Check::new(name, CheckStatus::Unknown, "cannot read process limits");
        };
        if rtprio.at_least(MIN_RTPRIO) {
            return Check::new(name, CheckStatus::Ok, format!("rtprio limit {rtprio}"));
        }
        let (status, how) = if Path::new(RTKIT_SERVICE).exists() {
            (
                CheckStatus::Warning,
                "only available through RTKit, which caps priorities and may demote threads",
            )
        } else {
            (CheckStatus::Error, "not available")
        };
        let detail = format!("rtprio limit {rtprio}: real-time scheduling {how}");
        Check::new(name, status, detail).with_fix(limits_fix("rtprio"))
    }

    pub(super) fn check_memlock(limits: &str) -> Check {
        let name = "Locked memory";
        match parse_limit(limits, "Max locked memory") {
            None => Check::new(name, CheckStatus::Unknown, "cannot read process limits"),
            Some(limit) if limit.at_least(MIN_MEMLOCK) => {
                Check::new(name, CheckStatus::Ok, format!("memlock limit {limit}"))
            }
            Some(limit) => Check::new(
                name,
                CheckStatus::Warning,
                format!("memlock limit {limit} is below {MIN_MEMLOCK} bytes"),
            )
            .with_fix(limits_fix("memlock")),
        }
    }

    /// Fix for a missing rlimit: join the group granted by an installed rule, or create one.
    fn limits_fix(item: &str) -> String {
        let groups = fs::read_to_string("/etc/group").unwrap_or_default();
        group_fix(item, &installed_limit_rules(), &groups, &current_groups())
    }

    /// Fix for a missing rlimit given the installed rules, `/etc/group` and our group ids.
    pub(super) fn group_fix(
        item: &str,
        rules: &[LimitRule],
        groups: &str,
        membership: &[u32],
    ) -> String {
        let rule = rules
            .iter()
            .filter(|r| r.item == item)
            .find_map(|r| group_gid(groups, &r.group).map(|gid| (r, gid)));
        match rule {
            Some((rule, gid)) if membership.contains(&gid) => format!(
                "You are in group '{}' but the limit is not applied yet: log out and back in.",
                rule.group
            ),
            Some((rule, _)) => format!(
                "sudo usermod -aG {} $USER\nThen log out and back in.",
                rule.group
            ),
            None => "printf '@audio - rtprio 95\\n@audio - memlock unlimited\\n' | sudo tee /etc/security/limits.d/95-audiostrada.conf\nsudo usermod -aG audio $USER\nThen log out and back in.".into(),
        }
    }

    fn check_quantum() -> Check {
        let name = "PipeWire buffer size";
        let output = Command::new("pw-metadata")
            .args(["-n", "settings"])
            .output();
        match output {
            Ok(out) if out.status.success() => {
                quantum_check(&parse_pw_settings(&String::from_utf8_lossy(&out.stdout)))
            }
            _ => Check::new(name, CheckStatus::Unknown, "cannot run pw-metadata"),
        }
    }

    pub(super) fn quantum_check(settings: &HashMap<String, String>) -> Check {
        let name = "PipeWire buffer size";
        let get = |key: &str| settings.get(key).and_then(|v| v.parse::<u64>().ok());
        let (Some(rate), Some(quantum), Some(min)) = (
            get("clock.rate"),
            get("clock.quantum"),
            get("clock.min-quantum"),
        ) else {
            return Check::new(name, CheckStatus::Unknown, "clock settings not found");
        };
        let force = get("clock.force-quantum").unwrap_or(0);
        let detail = format!(
            "rate {rate} Hz, default quantum {quantum} ({:.1} ms), minimum {min}",
            quantum as f64 * 1000.0 / rate as f64
        );
        if force != 0 {
            Check::new(
                name,
                CheckStatus::Warning,
                format!(
                    "{detail}; quantum forced to {force}, AudioStrada cannot choose its buffer size"
                ),
            )
            .with_fix("pw-metadata -n settings 0 clock.force-quantum 0")
        } else if min > MAX_MIN_QUANTUM {
            Check::new(
                name,
                CheckStatus::Warning,
                format!("{detail}; low latency buffers are not allowed"),
            )
            .with_fix(
                "pw-metadata -n settings 0 clock.min-quantum 64\nTo make it permanent: mkdir -p ~/.config/pipewire/pipewire.conf.d && printf 'context.properties = { default.clock.min-quantum = 64 }\\n' > ~/.config/pipewire/pipewire.conf.d/audiostrada.conf",
            )
        } else {
            Check::new(name, CheckStatus::Ok, detail)
        }
    }

    fn check_governor() -> Check {
        let governors: Vec<String> = fs::read_dir("/sys/devices/system/cpu")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| fs::read_to_string(e.path().join("cpufreq/scaling_governor")).ok())
            .map(|g| g.trim().to_owned())
            .collect();
        governor_check(governors)
    }

    pub(super) fn governor_check(mut governors: Vec<String>) -> Check {
        let name = "CPU frequency governor";
        governors.sort();
        governors.dedup();
        if governors.is_empty() {
            return Check::new(
                name,
                CheckStatus::Unknown,
                "CPU frequency scaling not exposed",
            );
        }
        let detail = governors.join(", ");
        if governors.iter().all(|g| g == "performance") {
            return Check::new(name, CheckStatus::Ok, detail);
        }
        let fix = if in_path("cpupower") {
            "sudo cpupower frequency-set -g performance"
        } else if in_path("powerprofilesctl") {
            "powerprofilesctl set performance"
        } else {
            "echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor"
        };
        Check::new(
            name,
            CheckStatus::Warning,
            format!("{detail}; frequency changes can cause dropouts at small buffer sizes"),
        )
        .with_fix(fix)
    }

    fn check_preemption() -> Check {
        let version = fs::read_to_string("/proc/sys/kernel/version").unwrap_or_default();
        let release = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
        let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
        let config =
            fs::read_to_string(format!("/boot/config-{}", release.trim())).unwrap_or_default();
        preemption_check(&version, &cmdline, &config)
    }

    pub(super) fn preemption_check(version: &str, cmdline: &str, config: &str) -> Check {
        let name = "Kernel preemption";
        match preemption_model(version, cmdline, config) {
            None => Check::new(
                name,
                CheckStatus::Unknown,
                "cannot determine preemption model",
            ),
            Some(model @ ("rt" | "full" | "lazy")) => Check::new(name, CheckStatus::Ok, model),
            Some(model) => {
                let check = Check::new(
                    name,
                    CheckStatus::Warning,
                    format!("{model}: higher risk of dropouts at small buffer sizes"),
                );
                if version.contains("PREEMPT_DYNAMIC") {
                    check.with_fix("Add preempt=full to GRUB_CMDLINE_LINUX_DEFAULT in /etc/default/grub, then: sudo update-grub && reboot")
                } else {
                    check.with_fix(
                        "Install a low-latency or PREEMPT_RT kernel from your distribution.",
                    )
                }
            }
        }
    }

    pub(super) fn parse_limit(limits: &str, label: &str) -> Option<Limit> {
        let soft = limits
            .lines()
            .find_map(|l| l.strip_prefix(label))?
            .split_whitespace()
            .next()?;
        if soft == "unlimited" {
            Some(Limit::Unlimited)
        } else {
            soft.parse().ok().map(Limit::Value)
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    pub(super) struct LimitRule {
        pub group: String,
        pub item: String,
    }

    /// Group rules (`@group <type> <item> <value>`) from a limits.conf file.
    pub(super) fn parse_limit_rules(text: &str) -> Vec<LimitRule> {
        text.lines()
            .filter_map(|line| {
                let mut fields = line.split('#').next()?.split_whitespace();
                let group = fields.next()?.strip_prefix('@')?;
                let item = fields.nth(1)?;
                fields.next()?;
                Some(LimitRule {
                    group: group.to_owned(),
                    item: item.to_owned(),
                })
            })
            .collect()
    }

    fn installed_limit_rules() -> Vec<LimitRule> {
        let mut files = vec![PathBuf::from("/etc/security/limits.conf")];
        if let Ok(dir) = fs::read_dir("/etc/security/limits.d") {
            let mut extra: Vec<PathBuf> = dir.flatten().map(|e| e.path()).collect();
            extra.sort();
            files.extend(extra);
        }
        files
            .iter()
            .filter_map(|f| fs::read_to_string(f).ok())
            .flat_map(|t| parse_limit_rules(&t))
            .collect()
    }

    /// Id of `group` in the text of `/etc/group`.
    fn group_gid(groups: &str, group: &str) -> Option<u32> {
        groups.lines().find_map(|line| {
            let mut fields = line.split(':');
            (fields.next()? == group).then_some(())?;
            fields.nth(1)?.parse().ok()
        })
    }

    fn current_groups() -> Vec<u32> {
        fs::read_to_string("/proc/self/status")
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.strip_prefix("Groups:"))
            .map(|g| {
                g.split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Parses `pw-metadata` lines such as `update: id:0 key:'clock.rate' value:'48000' type:''`.
    pub(super) fn parse_pw_settings(text: &str) -> HashMap<String, String> {
        text.lines()
            .filter_map(|line| {
                let key = line.split("key:'").nth(1)?.split('\'').next()?;
                let value = line.split("value:'").nth(1)?.split('\'').next()?;
                Some((key.to_owned(), value.to_owned()))
            })
            .collect()
    }

    pub(super) fn preemption_model(
        version: &str,
        cmdline: &str,
        config: &str,
    ) -> Option<&'static str> {
        if version.contains("PREEMPT_RT") {
            return Some("rt");
        }
        let models = ["none", "voluntary", "full", "lazy"];
        if let Some(arg) = cmdline
            .split_whitespace()
            .find_map(|a| a.strip_prefix("preempt="))
        {
            return models.into_iter().find(|m| *m == arg);
        }
        let enabled = |opt: &str| config.lines().any(|l| l.trim() == format!("{opt}=y"));
        [
            ("CONFIG_PREEMPT_RT", "rt"),
            ("CONFIG_PREEMPT", "full"),
            ("CONFIG_PREEMPT_LAZY", "lazy"),
            ("CONFIG_PREEMPT_VOLUNTARY", "voluntary"),
            ("CONFIG_PREEMPT_NONE", "none"),
        ]
        .into_iter()
        .find_map(|(opt, model)| enabled(opt).then_some(model))
    }

    fn in_path(program: &str) -> bool {
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|p| p.join(program).is_file()))
            .unwrap_or(false)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::CheckStatus;
    use super::linux::*;

    const LIMITS: &str = "\
Limit                     Soft Limit           Hard Limit           Units
Max locked memory         8388608              8388608              bytes
Max realtime priority     0                    95
Max realtime timeout      unlimited            unlimited            us
";

    #[test]
    fn limits_use_soft_value() {
        assert_eq!(
            parse_limit(LIMITS, "Max realtime priority"),
            Some(Limit::Value(0))
        );
        assert_eq!(
            parse_limit(LIMITS, "Max locked memory"),
            Some(Limit::Value(8_388_608))
        );
        assert_eq!(
            parse_limit(LIMITS, "Max realtime timeout"),
            Some(Limit::Unlimited)
        );
        assert_eq!(parse_limit(LIMITS, "Max nice priority"), None);
    }

    #[test]
    fn limit_rules_keep_group_entries() {
        let text = "# comment\n@pipewire   - rtprio  95\n@audio hard memlock unlimited # x\nuser - nice -10\n@bad rtprio\n";
        let rules = parse_limit_rules(text);
        assert_eq!(rules.len(), 2);
        assert_eq!(
            (rules[0].group.as_str(), rules[0].item.as_str()),
            ("pipewire", "rtprio")
        );
        assert_eq!(
            (rules[1].group.as_str(), rules[1].item.as_str()),
            ("audio", "memlock")
        );
    }

    #[test]
    fn pw_settings_are_parsed() {
        let text = "Found \"settings\" metadata 32\nupdate: id:0 key:'clock.rate' value:'48000' type:''\nupdate: id:0 key:'clock.allowed-rates' value:'[ 48000 ]' type:''\n";
        let settings = parse_pw_settings(text);
        assert_eq!(settings["clock.rate"], "48000");
        assert_eq!(settings["clock.allowed-rates"], "[ 48000 ]");
        assert_eq!(settings.len(), 2);
    }

    #[test]
    fn preemption_model_priorities() {
        let dynamic = "#38-Ubuntu SMP PREEMPT_DYNAMIC";
        let lazy = "# CONFIG_PREEMPT is not set\nCONFIG_PREEMPT_LAZY=y\nCONFIG_PREEMPT_DYNAMIC=y\n";
        assert_eq!(preemption_model(dynamic, "ro quiet", lazy), Some("lazy"));
        assert_eq!(
            preemption_model(dynamic, "ro preempt=full", lazy),
            Some("full")
        );
        assert_eq!(preemption_model("SMP PREEMPT_RT", "", ""), Some("rt"));
        assert_eq!(
            preemption_model(dynamic, "", "CONFIG_PREEMPT_VOLUNTARY=y\n"),
            Some("voluntary")
        );
        assert_eq!(preemption_model(dynamic, "", ""), None);
    }

    #[test]
    fn preemption_fix_depends_on_dynamic_kernels() {
        let dynamic = "SMP PREEMPT_DYNAMIC";
        assert_eq!(
            preemption_check(dynamic, "preempt=full", "").status,
            CheckStatus::Ok
        );
        let check = preemption_check(dynamic, "preempt=voluntary", "");
        assert_eq!(check.status, CheckStatus::Warning);
        assert!(check.fix.unwrap().contains("preempt=full"));
        let check = preemption_check("SMP", "", "CONFIG_PREEMPT_NONE=y\n");
        assert!(check.fix.unwrap().contains("low-latency"));
        assert_eq!(preemption_check("", "", "").status, CheckStatus::Unknown);
    }

    #[test]
    fn quantum_settings_are_judged() {
        let settings = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|&(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
        };
        let base = [
            ("clock.rate", "48000"),
            ("clock.quantum", "1024"),
            ("clock.min-quantum", "32"),
        ];
        let check = quantum_check(&settings(&base));
        assert_eq!(check.status, CheckStatus::Ok);
        assert!(check.detail.contains("21.3 ms"), "{}", check.detail);
        let mut forced = base.to_vec();
        forced.push(("clock.force-quantum", "512"));
        let check = quantum_check(&settings(&forced));
        assert_eq!(check.status, CheckStatus::Warning);
        assert!(check.fix.unwrap().contains("force-quantum 0"));
        let high = [base[0], base[1], ("clock.min-quantum", "1024")];
        assert!(
            quantum_check(&settings(&high))
                .fix
                .unwrap()
                .contains("min-quantum 64")
        );
        assert_eq!(
            quantum_check(&settings(&base[..2])).status,
            CheckStatus::Unknown
        );
    }

    #[test]
    fn governors_other_than_performance_warn() {
        let check = governor_check(vec!["performance".into(), "performance".into()]);
        assert_eq!(
            (check.status, check.detail.as_str()),
            (CheckStatus::Ok, "performance")
        );
        let check = governor_check(vec!["powersave".into(), "performance".into()]);
        assert_eq!(check.status, CheckStatus::Warning);
        assert!(check.detail.starts_with("performance, powersave;"));
        assert!(check.fix.is_some());
        assert_eq!(governor_check(Vec::new()).status, CheckStatus::Unknown);
    }

    #[test]
    fn limits_fix_points_to_the_right_group() {
        let rules = parse_limit_rules("@audio - rtprio 95\n@pipewire - memlock 4096\n");
        let groups = "root:x:0:\naudio:x:29:me\npipewire:x:120:\n";
        let fix = group_fix("rtprio", &rules, groups, &[1000]);
        assert!(fix.starts_with("sudo usermod -aG audio"), "{fix}");
        let fix = group_fix("memlock", &rules, groups, &[120]);
        assert!(fix.contains("group 'pipewire'") && fix.contains("log out"));
        let fix = group_fix("rtprio", &rules, "root:x:0:\n", &[]);
        assert!(fix.contains("95-audiostrada.conf"));
    }

    #[test]
    fn limit_checks_read_the_soft_values() {
        let memlock = check_memlock(LIMITS);
        assert_eq!(memlock.status, CheckStatus::Ok);
        assert_eq!(memlock.detail, "memlock limit 8388608");
        let low = LIMITS.replace("8388608 ", "65536   ");
        assert_eq!(check_memlock(&low).status, CheckStatus::Warning);
        let realtime = check_realtime(LIMITS);
        assert_ne!(realtime.status, CheckStatus::Ok);
        assert!(realtime.fix.is_some());
        let high = LIMITS.replace(
            "Max realtime priority     0 ",
            "Max realtime priority     95",
        );
        assert_eq!(check_realtime(&high).status, CheckStatus::Ok);
        assert_eq!(check_realtime("").status, CheckStatus::Unknown);
        assert_eq!(check_memlock("").status, CheckStatus::Unknown);
        assert_eq!(Limit::Unlimited.to_string(), "unlimited");
        assert_eq!(format!("{:>7}", CheckStatus::Error), "  error");
    }
}

/// What the kernel says of the sound cards in use: each open stream's format, rate and
/// periods, and the clock a USB interface runs at. Empty when nothing is open or unknown.
pub fn sound_cards() -> String {
    #[cfg(target_os = "linux")]
    {
        linux_sound_cards(std::path::Path::new("/proc/asound"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        String::new()
    }
}

#[cfg(target_os = "linux")]
fn linux_sound_cards(root: &std::path::Path) -> String {
    let read = |path: std::path::PathBuf| std::fs::read_to_string(path).unwrap_or_default();
    let mut cards: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("card") && n[4..].parse::<u32>().is_ok())
        })
        .collect();
    cards.sort();
    let mut out = Vec::new();
    for card in cards {
        let id = read(card.join("id")).trim().to_owned();
        let mut parts = Vec::new();
        let mut pcms: Vec<_> = std::fs::read_dir(&card)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("pcm"))
            })
            .collect();
        pcms.sort();
        for pcm in pcms {
            let params = read(pcm.join("sub0").join("hw_params"));
            if params.trim().is_empty() || params.trim() == "closed" {
                continue;
            }
            let fields: Vec<&str> = params
                .lines()
                .filter(|l| {
                    ["format", "channels", "rate", "period_size", "buffer_size"]
                        .iter()
                        .any(|k| l.starts_with(k))
                })
                .map(str::trim)
                .collect();
            let name = pcm
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned();
            parts.push(format!("{name} [{}]", fields.join(", ")));
        }
        for i in 0..4 {
            let stream = read(card.join(format!("stream{i}")));
            let clocks: Vec<&str> = stream
                .lines()
                .map(str::trim)
                .filter(|l| {
                    l.starts_with("Status")
                        || l.starts_with("Packet Size")
                        || l.starts_with("Momentary freq")
                })
                .collect();
            if clocks.iter().any(|l| l.contains("Running")) {
                parts.push(format!("usb [{}]", clocks.join(", ")));
            }
        }
        if !parts.is_empty() {
            out.push(format!("{id}: {}", parts.join("; ")));
        }
    }
    out.join(" | ")
}
