use std::collections::HashMap;

// ── Snapshot ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub size_gigabytes: f64,
    pub regions: Vec<String>,
}

// ── Droplet with provisioning state ─────────────────────────────────────────

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

/// Combined view of a droplet: API data + local overlay state.
#[derive(Debug, Clone)]
pub struct DropletView {
    /// None if still waiting for the create API call to return
    pub api: Option<DropletInfo>,
    pub name: String,
    pub local_status: LocalStatus,
    pub provision: ProvisionState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LocalStatus {
    Normal,
    Creating,  // We sent create, waiting for API / waiting for active
    Deleting,  // We sent delete, waiting for it to disappear
}

/// Provisioning steps we run against a droplet after it becomes active.
#[derive(Debug, Clone)]
pub struct ProvisionState {
    pub steps: Vec<ProvisionStep>,
    pub current: Option<usize>,  // None = not started or all done
    pub error: Option<String>,
    pub step_logs: Vec<Vec<String>>,  // per-step log lines
    pub needs_check: bool,  // SSH check needed to determine actual state
}

#[derive(Debug, Clone)]
pub struct ProvisionStep {
    pub name: &'static str,
    pub status: StepStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Failed(String),
}

pub const PROVISION_STEP_NAMES: &[&str] = &[
    "Transport GitHub SSH key",
    "Verify GitHub SSH key",
    "Install Docker",
    "Verify Docker",
    "Install Flox",
    "Verify Flox",
    "Install build-essential",
    "Verify build-essential",
    "Clone PostHog",
    "Verify PostHog clone",
    "Pull latest main",
    "Activate Flox environment",
    "Ensure tmux hogli session",
];

impl ProvisionState {
    pub fn new() -> Self {
        let steps: Vec<ProvisionStep> = PROVISION_STEP_NAMES
            .iter()
            .map(|&name| ProvisionStep {
                name,
                status: StepStatus::Pending,
            })
            .collect();
        let step_count = steps.len();
        Self {
            steps,
            current: None,
            error: None,
            step_logs: vec![Vec::new(); step_count],
            needs_check: false,
        }
    }

    /// Index of the most relevant step to display (current running, or last non-pending).
    pub fn most_recent_step(&self) -> usize {
        if let Some(current) = self.current {
            return current;
        }
        self.steps
            .iter()
            .rposition(|s| s.status != StepStatus::Pending)
            .unwrap_or(0)
    }

    pub fn overall_label(&self) -> &'static str {
        if self.error.is_some() {
            return "provision failed";
        }
        if self.steps.iter().all(|s| s.status == StepStatus::Done) {
            return "ready";
        }
        match self.current {
            Some(i) => self.steps.get(i).map_or("provisioning", |s| s.name),
            None => "pending",
        }
    }

    pub fn is_done(&self) -> bool {
        self.steps.iter().all(|s| s.status == StepStatus::Done)
    }
}

// ── Droplet registry: single source of truth ────────────────────────────────

/// Merges API data with local state (creating/deleting overlays).
/// Key insight: the API list is truth, but we overlay local knowledge.
pub struct DropletRegistry {
    /// Map from droplet name → view. We use name as key because creating
    /// droplets don't have an ID yet.
    pub views: Vec<DropletView>,
}

impl DropletRegistry {
    pub fn new() -> Self {
        Self { views: vec![] }
    }

    /// Merge fresh API data with local state. The rules:
    /// - If a droplet is in local_status=Deleting, keep showing it as deleting
    ///   until it disappears from the API response.
    /// - If a droplet is in local_status=Creating, update its API data once
    ///   the API returns it, but keep Creating until status="active".
    /// - Once a creating droplet becomes active, move to Normal and start provisioning.
    pub fn merge_api_data(&mut self, api_droplets: Vec<DropletInfo>) {
        let api_by_name: HashMap<String, DropletInfo> =
            api_droplets.into_iter().map(|d| (d.name.clone(), d)).collect();

        // Update existing views
        for view in self.views.iter_mut() {
            if let Some(api) = api_by_name.get(&view.name) {
                view.api = Some(api.clone());

                match view.local_status {
                    LocalStatus::Creating => {
                        if api.status == "active" {
                            view.local_status = LocalStatus::Normal;
                            // Check markers via SSH to detect what's already done
                            // (important for snapshot-based droplets)
                            if view.provision.current.is_none() && !view.provision.is_done() {
                                view.provision.needs_check = true;
                            }
                        }
                    }
                    LocalStatus::Deleting => {
                        // Still in API, keep showing as deleting
                    }
                    LocalStatus::Normal => {
                        // Just update API data
                    }
                }
            } else {
                // Not in API anymore
                if view.local_status == LocalStatus::Deleting {
                    // Good, it's been deleted. Mark for removal.
                    view.local_status = LocalStatus::Normal;
                    view.api = None; // signal for removal
                }
            }
        }

        // Remove views that are gone from API and not locally tracked
        self.views.retain(|v| {
            v.api.is_some() || v.local_status == LocalStatus::Creating
        });

        // Add new droplets from API that we don't have locally
        let known_names: std::collections::HashSet<String> =
            self.views.iter().map(|v| v.name.clone()).collect();
        for (name, api) in api_by_name {
            if !known_names.contains(&name) {
                let is_active = api.status == "active";
                let mut ps = ProvisionState::new();
                if is_active {
                    // Need to SSH in and check which steps actually completed
                    ps.needs_check = true;
                }
                self.views.push(DropletView {
                    name: name.clone(),
                    api: Some(api),
                    local_status: LocalStatus::Normal,
                    provision: ps,
                });
            }
        }
    }

    pub fn add_creating(&mut self, name: String) {
        self.views.push(DropletView {
            name,
            api: None,
            local_status: LocalStatus::Creating,
            provision: ProvisionState::new(),
        });
    }

    pub fn mark_deleting(&mut self, id: i64) {
        if let Some(view) = self.views.iter_mut().find(|v| {
            v.api.as_ref().map_or(false, |a| a.id == id)
        }) {
            view.local_status = LocalStatus::Deleting;
        }
    }

    pub fn mark_create_failed(&mut self, name: &str) {
        self.views.retain(|v| !(v.name == name && v.local_status == LocalStatus::Creating && v.api.is_none()));
    }

    pub fn views(&self) -> &[DropletView] {
        &self.views
    }

    pub fn get_by_index(&self, idx: usize) -> Option<&DropletView> {
        self.views.get(idx)
    }

    pub fn get_by_index_mut(&mut self, idx: usize) -> Option<&mut DropletView> {
        self.views.get_mut(idx)
    }

    pub fn find_by_name_mut(&mut self, name: &str) -> Option<&mut DropletView> {
        self.views.iter_mut().find(|v| v.name == name)
    }

    pub fn len(&self) -> usize {
        self.views.len()
    }
}

// ── Static data ─────────────────────────────────────────────────────────────

pub struct Region {
    pub slug: &'static str,
    pub name: &'static str,
}

pub struct MachineSize {
    pub slug: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub available: bool,
    pub hourly_price: f64,
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
        slug: "c-16-intel",
        name: "CPU-Optimized 16 Intel",
        desc: "16 vCPU / 32GB",
        available: true,
        hourly_price: 0.650,
    },
    MachineSize {
        slug: "c-32-intel",
        name: "CPU-Optimized 32 Intel",
        desc: "32 vCPU / 64GB",
        available: true,
        hourly_price: 1.300,
    },
];

pub const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Returns the number of seconds since an ISO 8601 timestamp, or None on parse failure.
pub fn seconds_since(iso_str: &str) -> Option<i64> {
    if iso_str.is_empty() {
        return None;
    }
    let iso = iso_str.replace('Z', "+00:00");
    let parts: Vec<&str> = iso.split('T').collect();
    if parts.len() != 2 {
        return None;
    }
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    let time_part = parts[1].split('+').next().unwrap_or("");
    let time_parts: Vec<&str> = time_part.split(':').collect();

    if date_parts.len() != 3 || time_parts.len() < 3 {
        return None;
    }

    use std::time::{SystemTime, UNIX_EPOCH};

    let y: i64 = date_parts[0].parse().ok()?;
    let mo: i64 = date_parts[1].parse().ok()?;
    let d: i64 = date_parts[2].parse().ok()?;
    let h: i64 = time_parts[0].parse().ok()?;
    let mi: i64 = time_parts[1].parse().ok()?;
    let s: i64 = time_parts[2].split('.').next()?.parse().ok()?;

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
}

pub fn hourly_price_for_size(size_slug: &str) -> Option<f64> {
    MACHINES.iter().find(|m| m.slug == size_slug).map(|m| m.hourly_price)
}

/// Snapshot storage cost: $0.06/GB per month
pub fn snapshot_monthly_cost(size_gb: f64) -> f64 {
    size_gb * 0.06
}

pub fn time_ago(iso_str: &str) -> String {
    match seconds_since(iso_str) {
        Some(secs) if secs < 60 => "just now".to_string(),
        Some(secs) if secs < 3600 => format!("{}m ago", secs / 60),
        Some(secs) if secs < 86400 => format!("{}h ago", secs / 3600),
        Some(secs) if secs > 0 => format!("{}d ago", secs / 86400),
        _ => String::new(),
    }
}
