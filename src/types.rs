#[derive(Debug, Clone)]
pub struct DropletInfo {
    pub id: i64,
    pub name: String,
    pub status: String,
    pub ip: Option<String>,
    pub region: String,
    pub size: String,
    pub created_at: String,
}

pub struct Region {
    pub slug: &'static str,
    pub name: &'static str,
}

pub struct MachineSize {
    pub slug: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub available: bool,
}

pub const REGIONS: &[Region] = &[
    Region { slug: "fra1", name: "Frankfurt" },
    Region { slug: "ams3", name: "Amsterdam" },
    Region { slug: "nyc1", name: "New York" },
    Region { slug: "lon1", name: "London" },
    Region { slug: "sgp1", name: "Singapore" },
    Region { slug: "tor1", name: "Toronto" },
];

pub const MACHINES: &[MachineSize] = &[
    MachineSize {
        slug: "gd-8vcpu-32gb-intel",
        name: "General Purpose Intel",
        desc: "8 vCPU / 32GB",
        available: true,
    },
    MachineSize {
        slug: "s-2vcpu-8gb-160gb-intel",
        name: "Premium Intel Medium",
        desc: "2 vCPU / 8GB / 160GB",
        available: true,
    },
];

pub const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn time_ago(iso_str: &str) -> String {
    if iso_str.is_empty() {
        return String::new();
    }
    // Simple ISO 8601 parse: "2024-01-15T10:30:00Z"
    let iso = iso_str.replace('Z', "+00:00");
    let parts: Vec<&str> = iso.split('T').collect();
    if parts.len() != 2 {
        return String::new();
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    let time_part = parts[1].split('+').next().unwrap_or("");
    let time_parts: Vec<&str> = time_part.split(':').collect();

    if date_parts.len() != 3 || time_parts.len() < 3 {
        return String::new();
    }

    let parse = || -> Option<i64> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let y: i64 = date_parts[0].parse().ok()?;
        let mo: i64 = date_parts[1].parse().ok()?;
        let d: i64 = date_parts[2].parse().ok()?;
        let h: i64 = time_parts[0].parse().ok()?;
        let mi: i64 = time_parts[1].parse().ok()?;
        let s: i64 = time_parts[2].split('.').next()?.parse().ok()?;

        // Rough epoch calculation (good enough for "time ago")
        let days = (y - 1970) * 365 + (y - 1969) / 4 - (y - 1901) / 100 + (y - 1601) / 400;
        let month_days: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut day_sum = days;
        for i in 0..(mo - 1) as usize {
            day_sum += month_days.get(i).copied().unwrap_or(30);
        }
        if mo > 2 && (y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)) {
            day_sum += 1;
        }
        day_sum += d - 1;
        let created_epoch = day_sum * 86400 + h * 3600 + mi * 60 + s;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;

        Some(now - created_epoch)
    };

    match parse() {
        Some(secs) if secs < 60 => "just now".to_string(),
        Some(secs) if secs < 3600 => format!("{}m ago", secs / 60),
        Some(secs) if secs < 86400 => format!("{}h ago", secs / 3600),
        Some(secs) if secs > 0 => format!("{}d ago", secs / 86400),
        _ => String::new(),
    }
}
