use std::collections::HashMap;
use std::sync::mpsc::Sender;

use crossterm::event::{KeyCode, KeyEvent};

use crate::backend::{config, digitalocean, ssh};
use crate::types::{
    DropletInfo, DropletRegistry, LocalStatus, SnapshotInfo, StepStatus, MACHINES,
    PROVISION_STEP_NAMES, REGIONS,
};

// ── Tick constants (1 tick = ~100ms) ────────────────────────────────────────

const AUTO_ADVANCE_TICKS: u32 = 10; // 1s
const CHECK_INTERVAL_TICKS: u32 = 50; // 5s
const REFRESH_INTERVAL_TICKS: u32 = 30; // 3s (faster to catch status changes)
const SNAPSHOT_REFRESH_INTERVAL_TICKS: u32 = 100; // 10s

// ── Messages from background tasks ─────────────────────────────────────────

pub enum Msg {
    // Welcome: GitHub
    GithubKeyMissing,
    GithubKeyGenerated { public_key: String },
    GithubKeyGenFailed(String),
    GithubTestResult { success: bool, message: String },

    // Welcome: DigitalOcean
    DoKeyExists,
    DoKeyMissing,
    DoTestResult { success: bool, message: String },

    // Main: Droplets — single source: periodic API refresh
    DropletsLoaded(Vec<DropletInfo>),
    DropletCreateFailed { name: String, error: String },
    DropletDeleteFailed { id: i64, error: String },
    DropletRenameDone { id: i64, new_name: String },
    DropletRenameFailed { error: String },
    HostsMappingDone { name: String, mapped: bool },
    HostsMappingFailed { error: String },

    // Provisioning
    ProvisionStepDone { name: String, step_idx: usize },
    ProvisionStepFailed { name: String, step_idx: usize, error: String },
    ProvisionLog { name: String, step_idx: usize, line: String },
    ProvisionStateChecked { name: String, completed_steps: Vec<bool> },

    // Main: Snapshots
    SnapshotsLoaded(Vec<SnapshotInfo>),
    SnapshotCreateDone { name: String },
    SnapshotCreateFailed { error: String },
    SnapshotDeleteDone { id: String },
    SnapshotDeleteFailed { error: String },
    SnapshotRenameDone { id: String, new_name: String },
    SnapshotRenameFailed { error: String },

    // Main: Config health checks
    ConfigGithubCheck { success: bool, message: String },
    ConfigDoCheck { success: bool, message: String },
}

// ── App State ───────────────────────────────────────────────────────────────

pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
    pub spinner_idx: usize,
    pub tick_count: u64,
    pub notification: Option<(String, u32)>,
}

pub enum Screen {
    Welcome(WelcomeState),
    Main(MainState),
}

// ── Welcome State ───────────────────────────────────────────────────────────

pub struct WelcomeState {
    pub phase: WelcomePhase,
    pub auto_advance: u32,
    pub github_done_msg: Option<String>,
    // Parallel check: store DO result while GitHub is still in progress
    pub do_early_result: Option<DoEarlyResult>,
}

pub enum DoEarlyResult {
    Missing,
    TestOk(String),
    TestFailed(String),
}

pub enum WelcomePhase {
    CheckingGithub,
    GithubOk(String),
    GithubMissing,
    GeneratingGithub,
    GithubGenerated { public_key: String, copied: bool },
    TestingGithub { public_key: Option<String>, copied: bool },
    GithubFailed { error: String, public_key: Option<String>, copied: bool },

    CheckingDo,
    DoOk(String),
    DoMissing,
    DoInput(TextInput),
    TestingDo,
    DoFailed(String),
}

// ── Main State ──────────────────────────────────────────────────────────────

pub struct MainState {
    pub tab: Tab,
    pub droplets: DropletsState,
    pub snapshots: SnapshotsState,
    pub config: ConfigViewState,
    pub popup: Option<Popup>,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Tab {
    Droplets,
    Snapshots,
    Config,
}

pub struct SnapshotsState {
    pub list: Vec<SnapshotInfo>,
    pub selected: usize,
    pub loading: bool,
    pub refresh_countdown: u32,
    pub pending: Vec<String>, // snapshot names we're waiting to appear
}

pub struct DropletsState {
    pub registry: DropletRegistry,
    pub selected: usize,
    pub focus: DFocus,
    pub detail_selected: usize,
    pub provision_selected: usize,
    pub refresh_countdown: u32,
    pub loading: bool,
    pub tunnels: HashMap<String, std::process::Child>,
}

#[derive(PartialEq, Clone, Copy)]
pub enum DFocus {
    List,
    DetailInfo,
    DetailProvision,
}

pub struct ConfigViewState {
    pub focus: CFocus,
    pub github: KeyCheckInfo,
    pub digitalocean: KeyCheckInfo,
}

#[derive(PartialEq, Clone, Copy)]
pub enum CFocus {
    Github,
    DigitalOcean,
}

pub struct KeyCheckInfo {
    pub status: KeyStatus,
    pub message: Option<String>,
    pub selected: usize,
    pub next_check: u32,
}

#[derive(PartialEq, Clone, Copy)]
pub enum KeyStatus {
    Unknown,
    Checking,
    Ok,
    Error,
}

// ── Popups ──────────────────────────────────────────────────────────────────

pub enum Popup {
    CreateDroplet(CreatePopupState),
    Confirm { message: String, action: PendingAction },
    GithubSetup(GithubSetupPhase),
    DoSetup(DoSetupPhase),
SnapshotName { droplet_id: i64, input: TextInput },
    RenameSnapshot { snapshot_id: String, input: TextInput },
    RenameDroplet { droplet_id: i64, input: TextInput },
    Message(String),
}

pub enum PendingAction {
    DeleteDroplet { id: i64, name: String },
    DeleteSnapshot { id: String, name: String },
    RegenerateGithubKey,
}

pub enum GithubSetupPhase {
    Generating,
    Ready { public_key: String, copied: bool },
    Testing { public_key: String, copied: bool },
    Failed { error: String, public_key: String, copied: bool },
    Done(String),
}

pub enum DoSetupPhase {
    Input(TextInput),
    Testing,
    Failed(String),
    Done(String),
}

pub struct CreatePopupState {
    pub step: CreateStep,
    pub region_idx: usize,
    pub machine_idx: usize,
    pub snapshot_idx: Option<usize>, // None = base image, Some(i) = index into snapshots
    pub name: String,
    pub snapshots: Vec<SnapshotInfo>,
}

pub enum CreateStep {
    Main { selected: usize },
    Region { selected: usize },
    Machine { selected: usize },
    Snapshot { selected: usize }, // 0 = "None (base image)", 1+ = snapshot
    Name(TextInput),
}

// ── Text Input ──────────────────────────────────────────────────────────────

pub struct TextInput {
    pub text: String,
    pub cursor: usize,
    pub masked: bool,
}

impl TextInput {
    pub fn new(initial: &str, masked: bool) -> Self {
        Self {
            cursor: initial.len(),
            text: initial.to_string(),
            masked,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> InputResult {
        match key.code {
            KeyCode::Enter => return InputResult::Submit,
            KeyCode::Esc => return InputResult::Cancel,
            KeyCode::Char(c) => {
                self.text.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.text.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor < self.text.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            _ => {}
        }
        InputResult::Continue
    }

    pub fn display(&self) -> String {
        if self.masked {
            "•".repeat(self.text.len())
        } else {
            self.text.clone()
        }
    }
}

pub enum InputResult {
    Continue,
    Submit,
    Cancel,
}

// ── App Implementation ──────────────────────────────────────────────────────

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::Welcome(WelcomeState {
                phase: WelcomePhase::CheckingGithub,
                auto_advance: 0,
                github_done_msg: None,
                do_early_result: None,
            }),
            should_quit: false,
            spinner_idx: 0,
            tick_count: 0,
            notification: None,
        }
    }

    pub fn start_initial_check(&self, tx: &Sender<Msg>) {
        // GitHub check
        let tx1 = tx.clone();
        std::thread::spawn(move || {
            let cfg = config::load();
            match cfg.github_ssh_key_path {
                Some(path) if std::path::Path::new(&path).exists() => {
                    let (ok, msg) = ssh::test_github_ssh_key(&path);
                    tx1.send(Msg::GithubTestResult {
                        success: ok,
                        message: msg,
                    })
                    .ok();
                }
                _ => {
                    tx1.send(Msg::GithubKeyMissing).ok();
                }
            }
        });
        // DO check (in parallel)
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            let cfg = config::load();
            match cfg.do_api_key {
                Some(key) if !key.is_empty() => {
                    tx2.send(Msg::DoKeyExists).ok();
                    let (ok, msg) = ssh::test_do_api_key(&key);
                    tx2.send(Msg::DoTestResult {
                        success: ok,
                        message: msg,
                    })
                    .ok();
                }
                _ => {
                    tx2.send(Msg::DoKeyMissing).ok();
                }
            }
        });
    }

    // ── Tick ────────────────────────────────────────────────────────────────

    pub fn tick(&mut self, tx: &Sender<Msg>) {
        self.tick_count += 1;
        self.spinner_idx = (self.spinner_idx + 1) % 10;

        if let Some((_, ref mut ticks)) = self.notification {
            if *ticks > 0 {
                *ticks -= 1;
            } else {
                self.notification = None;
            }
        }

        let mut do_advance_welcome = false;
        let mut do_refresh_droplets = false;
        let mut do_refresh_snapshots = false;
        let mut do_check_github = false;
        let mut do_check_do = false;

        match &mut self.screen {
            Screen::Welcome(state) => {
                if state.auto_advance > 0 {
                    state.auto_advance -= 1;
                    if state.auto_advance == 0 {
                        do_advance_welcome = true;
                    }
                }
            }
            Screen::Main(main) => {
                if main.droplets.refresh_countdown > 0 {
                    main.droplets.refresh_countdown -= 1;
                } else {
                    main.droplets.refresh_countdown = REFRESH_INTERVAL_TICKS;
                    do_refresh_droplets = true;
                }

                if main.snapshots.refresh_countdown > 0 {
                    main.snapshots.refresh_countdown -= 1;
                } else {
                    main.snapshots.refresh_countdown = SNAPSHOT_REFRESH_INTERVAL_TICKS;
                    do_refresh_snapshots = true;
                }

                if main.config.github.next_check > 0 {
                    main.config.github.next_check -= 1;
                } else {
                    main.config.github.next_check = CHECK_INTERVAL_TICKS;
                    main.config.github.status = KeyStatus::Checking;
                    do_check_github = true;
                }
                if main.config.digitalocean.next_check > 0 {
                    main.config.digitalocean.next_check -= 1;
                } else {
                    main.config.digitalocean.next_check = CHECK_INTERVAL_TICKS;
                    main.config.digitalocean.status = KeyStatus::Checking;
                    do_check_do = true;
                }

                // Check tunnel health: detect dead SSH tunnel processes
                let mut dead_tunnels = Vec::new();
                for (name, child) in main.droplets.tunnels.iter_mut() {
                    if let Ok(Some(_)) = child.try_wait() {
                        dead_tunnels.push(name.clone());
                    }
                }
                for name in &dead_tunnels {
                    main.droplets.tunnels.remove(name);
                    if let Some(view) = main.droplets.registry.find_by_name_mut(name) {
                        view.port_forward.active = false;
                    }
                }
            }
        }

        if do_advance_welcome {
            self.advance_welcome(tx);
        }
        if do_refresh_droplets {
            self.spawn_refresh_droplets(tx);
        }
        if do_refresh_snapshots {
            self.spawn_refresh_snapshots(tx);
        }
        if do_check_github {
            self.spawn_config_github_check(tx);
        }
        if do_check_do {
            self.spawn_config_do_check(tx);
        }
    }

    fn advance_welcome(&mut self, _tx: &Sender<Msg>) {
        if let Screen::Welcome(state) = &mut self.screen {
            match &state.phase {
                WelcomePhase::GithubOk(_) => {
                    let msg = if let WelcomePhase::GithubOk(m) = &state.phase {
                        m.clone()
                    } else {
                        String::new()
                    };
                    state.github_done_msg = Some(msg);
                    state.auto_advance = 0;

                    // Use early DO result from parallel check if available
                    match state.do_early_result.take() {
                        Some(DoEarlyResult::TestOk(msg)) => {
                            state.phase = WelcomePhase::DoOk(msg);
                            state.auto_advance = AUTO_ADVANCE_TICKS;
                        }
                        Some(DoEarlyResult::TestFailed(msg)) => {
                            state.phase = WelcomePhase::DoFailed(msg);
                        }
                        Some(DoEarlyResult::Missing) => {
                            state.phase = WelcomePhase::DoMissing;
                        }
                        None => {
                            // Parallel thread hasn't finished yet; it will deliver the result
                            state.phase = WelcomePhase::CheckingDo;
                        }
                    }
                }
                WelcomePhase::DoOk(_) => {
                    self.transition_to_main();
                }
                _ => {}
            }
        }
    }

    fn transition_to_main(&mut self) {
        self.screen = Screen::Main(MainState {
            tab: Tab::Droplets,
            droplets: DropletsState {
                registry: DropletRegistry::new(),
                selected: 0,
                focus: DFocus::List,
                detail_selected: 0,
                provision_selected: 0,
                refresh_countdown: 0,
                loading: true,
                tunnels: HashMap::new(),
            },
            snapshots: SnapshotsState {
                list: Vec::new(),
                selected: 0,
                loading: true,
                refresh_countdown: 0,
                pending: Vec::new(),
            },
            config: ConfigViewState {
                focus: CFocus::Github,
                github: KeyCheckInfo {
                    status: KeyStatus::Unknown,
                    message: None,
                    selected: 0,
                    next_check: 0,
                },
                digitalocean: KeyCheckInfo {
                    status: KeyStatus::Unknown,
                    message: None,
                    selected: 0,
                    next_check: 0,
                },
            },
            popup: None,
        });
    }

    // ── Handle Key ──────────────────────────────────────────────────────────

    pub fn handle_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        if key.code == KeyCode::Char('q') && !self.is_text_input_active() {
            self.should_quit = true;
            return;
        }

        match &self.screen {
            Screen::Welcome(_) => self.handle_welcome_key(key, tx),
            Screen::Main(_) => {
                if let Screen::Main(main) = &self.screen {
                    if main.popup.is_some() {
                        self.handle_popup_key(key, tx);
                        return;
                    }
                }
                self.handle_main_key(key, tx);
            }
        }
    }

    fn is_text_input_active(&self) -> bool {
        match &self.screen {
            Screen::Welcome(w) => matches!(w.phase, WelcomePhase::DoInput(_)),
            Screen::Main(m) => match &m.popup {
                Some(Popup::CreateDroplet(c)) => matches!(c.step, CreateStep::Name(_)),
                Some(Popup::DoSetup(DoSetupPhase::Input(_))) => true,
Some(Popup::SnapshotName { .. }) => true,
                Some(Popup::RenameSnapshot { .. }) => true,
                Some(Popup::RenameDroplet { .. }) => true,
                _ => false,
            },
        }
    }

    // ── Welcome Key Handling ────────────────────────────────────────────────

    fn handle_welcome_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Welcome(state) = &mut self.screen else {
            return;
        };

        match &mut state.phase {
            // Loading states: only Esc to skip
            WelcomePhase::CheckingGithub
            | WelcomePhase::CheckingDo
            | WelcomePhase::TestingDo
            | WelcomePhase::GeneratingGithub => {
                if key.code == KeyCode::Esc {
                    self.transition_to_main();
                }
            }

            // Success states: auto-advance handles it, but Enter/Esc skip ahead
            WelcomePhase::GithubOk(_) | WelcomePhase::DoOk(_) => {
                if key.code == KeyCode::Enter || key.code == KeyCode::Esc {
                    state.auto_advance = 0;
                    self.advance_welcome(tx);
                }
            }

            WelcomePhase::GithubMissing => match key.code {
                KeyCode::Enter => {
                    state.phase = WelcomePhase::GeneratingGithub;
                    let tx = tx.clone();
                    std::thread::spawn(move || match ssh::generate_github_ssh_key() {
                        Ok((_, pub_key)) => {
                            tx.send(Msg::GithubKeyGenerated { public_key: pub_key }).ok();
                        }
                        Err(e) => {
                            tx.send(Msg::GithubKeyGenFailed(e.to_string())).ok();
                        }
                    });
                }
                KeyCode::Esc => self.transition_to_main(),
                _ => {}
            },

            WelcomePhase::GithubGenerated { public_key, copied } => match key.code {
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    ssh::copy_to_clipboard(public_key);
                    *copied = true;
                }
                KeyCode::Enter => {
                    let pk = public_key.clone();
                    let c = *copied;
                    state.phase = WelcomePhase::TestingGithub {
                        public_key: Some(pk),
                        copied: c,
                    };
                    let tx = tx.clone();
                    let cfg = config::load();
                    std::thread::spawn(move || {
                        if let Some(path) = cfg.github_ssh_key_path {
                            let (ok, msg) = ssh::test_github_ssh_key(&path);
                            tx.send(Msg::GithubTestResult { success: ok, message: msg }).ok();
                        }
                    });
                }
                KeyCode::Esc => self.transition_to_main(),
                _ => {}
            },

            WelcomePhase::TestingGithub { .. } => {
                if key.code == KeyCode::Esc {
                    self.transition_to_main();
                }
            }

            WelcomePhase::GithubFailed { public_key, copied, .. } => match key.code {
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    if let Some(pk) = public_key {
                        ssh::copy_to_clipboard(pk);
                        *copied = true;
                    }
                }
                KeyCode::Enter => {
                    let pk = public_key.clone();
                    let c = *copied;
                    state.phase = WelcomePhase::TestingGithub {
                        public_key: pk,
                        copied: c,
                    };
                    let tx = tx.clone();
                    let cfg = config::load();
                    std::thread::spawn(move || {
                        if let Some(path) = cfg.github_ssh_key_path {
                            let (ok, msg) = ssh::test_github_ssh_key(&path);
                            tx.send(Msg::GithubTestResult { success: ok, message: msg }).ok();
                        }
                    });
                }
                KeyCode::Esc => self.transition_to_main(),
                _ => {}
            },

            WelcomePhase::DoMissing => match key.code {
                KeyCode::Enter => {
                    state.phase = WelcomePhase::DoInput(TextInput::new("", true));
                }
                KeyCode::Esc => self.transition_to_main(),
                _ => {}
            },

            WelcomePhase::DoInput(input) => match input.handle_key(key) {
                InputResult::Submit => {
                    let key_value = input.text.clone();
                    if key_value.is_empty() {
                        return;
                    }
                    let mut cfg = config::load();
                    cfg.do_api_key = Some(key_value.clone());
                    config::save(&cfg);
                    state.phase = WelcomePhase::TestingDo;
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let (ok, msg) = ssh::test_do_api_key(&key_value);
                        tx.send(Msg::DoTestResult { success: ok, message: msg }).ok();
                    });
                }
                InputResult::Cancel => self.transition_to_main(),
                InputResult::Continue => {}
            },

            WelcomePhase::DoFailed(_) => match key.code {
                KeyCode::Enter => {
                    state.phase = WelcomePhase::DoInput(TextInput::new("", true));
                }
                KeyCode::Esc => self.transition_to_main(),
                _ => {}
            },
        }
    }

    // ── Main Key Handling ───────────────────────────────────────────────────

    fn handle_main_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else { return };

        match key.code {
            KeyCode::Tab => {
                main.tab = match main.tab {
                    Tab::Droplets => Tab::Snapshots,
                    Tab::Snapshots => Tab::Config,
                    Tab::Config => Tab::Droplets,
                };
            }
            _ => match main.tab {
                Tab::Droplets => self.handle_droplets_key(key, tx),
                Tab::Snapshots => self.handle_snapshots_key(key, tx),
                Tab::Config => self.handle_config_key(key, tx),
            },
        }
    }

    fn handle_droplets_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else { return };
        match main.droplets.focus {
            DFocus::List => self.handle_droplets_list_key(key, tx),
            DFocus::DetailInfo => self.handle_detail_info_key(key, tx),
            DFocus::DetailProvision => self.handle_detail_provision_key(key, tx),
        }
    }

    fn handle_droplets_list_key(&mut self, key: KeyEvent, _tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else { return };
        let ds = &mut main.droplets;
        let droplet_count = ds.registry.len();
        let total = droplet_count + 1; // +1 for "+ Create new"

        match key.code {
            KeyCode::Up => {
                if ds.selected > 0 {
                    ds.selected -= 1;
                }
            }
            KeyCode::Down => {
                if ds.selected + 1 < total {
                    ds.selected += 1;
                }
            }
            KeyCode::Enter | KeyCode::Right => {
                if ds.selected < droplet_count {
                    let view = &ds.registry.views()[ds.selected];
                    if view.local_status != LocalStatus::Deleting {
                        ds.focus = DFocus::DetailInfo;
                        ds.detail_selected = 0;
                    }
                } else {
                    let count = droplet_count;
                    let snapshots = main.snapshots.list.clone();
                    main.popup = Some(Popup::CreateDroplet(CreatePopupState {
                        step: CreateStep::Main { selected: 0 },
                        region_idx: 2, // nyc1
                        machine_idx: 0,
                        snapshot_idx: None,
                        name: format!("droplet-{}", count + 1),
                        snapshots,
                    }));
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if ds.selected < droplet_count {
                    let view = &ds.registry.views()[ds.selected];
                    if view.local_status != LocalStatus::Deleting {
                        if let Some(api) = &view.api {
                            let id = api.id;
                            let name = api.name.clone();
                            main.popup = Some(Popup::Confirm {
                                message: format!("Delete '{name}'?"),
                                action: PendingAction::DeleteDroplet { id, name },
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_detail_info_key(&mut self, key: KeyEvent, _tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else { return };
        let ds = &mut main.droplets;

        let action_count = if let Some(view) = ds.registry.get_by_index(ds.selected) {
            if let Some(api) = &view.api {
                if api.ip.is_some() { 6 } else { 1 } // Copy SSH, Open SSH, .droplet, Snapshot, Rename, Delete
            } else {
                0
            }
        } else {
            0
        };

        match key.code {
            KeyCode::Left | KeyCode::Esc => {
                ds.focus = DFocus::List;
            }
            KeyCode::Right => {
                ds.focus = DFocus::DetailProvision;
                if let Some(view) = ds.registry.get_by_index(ds.selected) {
                    ds.provision_selected = view.provision.most_recent_step();
                }
            }
            KeyCode::Up => {
                if ds.detail_selected > 0 {
                    ds.detail_selected -= 1;
                }
            }
            KeyCode::Down => {
                if ds.detail_selected + 1 < action_count {
                    ds.detail_selected += 1;
                }
            }
            KeyCode::Enter => {
                let Some(view) = ds.registry.get_by_index(ds.selected) else { return };
                let Some(api) = &view.api else { return };
                let has_ip = api.ip.is_some();
                let droplet_id = api.id;
                let droplet_name = api.name.clone();
                let ip = api.ip.clone();

                if has_ip && ds.detail_selected == 0 {
                    // Copy SSH cmd
                    if let Some(ip) = ip {
                        let cfg = config::load();
                        let key_path = cfg.droplet_ssh_key_path.unwrap_or_default();
                        let cmd = format!(
                            "ssh -i {key_path} -o StrictHostKeyChecking=accept-new root@{ip}"
                        );
                        let copied = ssh::copy_to_clipboard(&cmd);
                        self.notification = Some((
                            if copied { format!("Copied: {cmd}") } else { cmd },
                            30,
                        ));
                    }
                } else if has_ip && ds.detail_selected == 1 {
                    // Open SSH in terminal
                    if let Some(ip) = ip {
                        let cfg = config::load();
                        let key_path = cfg.droplet_ssh_key_path.unwrap_or_default();
                        if ssh::open_ssh_in_terminal(&key_path, &ip) {
                            self.notification = Some(("Opened SSH in Terminal.app".to_string(), 30));
                        } else {
                            self.notification = Some(("Failed to open terminal".to_string(), 30));
                        }
                    }
                } else if has_ip && ds.detail_selected == 2 {
                    // Toggle .droplet hosts mapping + auto-start/stop port forward + configure Caddy
                    if let Some(ref ip) = ip {
                        let dname = droplet_name.clone();
                        let ip_for_thread = ip.clone();
                        let tx = _tx.clone();
                        let is_currently_mapped = ds.registry.get_by_index(ds.selected)
                            .map(|v| v.hosts_mapped)
                            .unwrap_or(false);
                        let local_port = ds.registry.get_by_index(ds.selected)
                            .map(|v| v.port_forward.local_port)
                            .unwrap_or(28000);
                        let dname_for_thread = dname.clone();
                        std::thread::spawn(move || {
                            match ssh::toggle_host_mapping(&dname_for_thread, local_port) {
                                Ok(mapped) => {
                                    // If mapping ON, also configure Caddy to accept any host
                                    if mapped {
                                        let cfg = config::load();
                                        let key_path = cfg.droplet_ssh_key_path.unwrap_or_default();
                                        if let Err(e) = ssh::configure_caddy_any_host(&key_path, &ip_for_thread) {
                                            tx.send(Msg::HostsMappingFailed { error: format!("Caddy config failed: {e}") }).ok();
                                            return;
                                        }
                                    }
                                    tx.send(Msg::HostsMappingDone { name: dname_for_thread, mapped }).ok();
                                }
                                Err(e) => { tx.send(Msg::HostsMappingFailed { error: e.to_string() }).ok(); }
                            }
                        });
                        if !is_currently_mapped {
                            // Mapping ON: auto-start port forwarding if not already active
                            let is_active = ds.registry.get_by_index(ds.selected)
                                .map(|v| v.port_forward.active)
                                .unwrap_or(false);
                            if !is_active {
                                let cfg = config::load();
                                let key_path = cfg.droplet_ssh_key_path.unwrap_or_default();
                                match ssh::start_port_forward(&key_path, ip, local_port) {
                                    Ok(child) => {
                                        ds.tunnels.insert(droplet_name.clone(), child);
                                        if let Some(view) = ds.registry.get_by_index_mut(ds.selected) {
                                            view.port_forward.active = true;
                                        }
                                    }
                                    Err(_) => {}
                                }
                            }
                        } else {
                            // Mapping OFF: also stop port forwarding
                            if let Some(mut child) = ds.tunnels.remove(&dname) {
                                let _ = child.kill();
                                let _ = child.wait();
                            }
                            if let Some(view) = ds.registry.get_by_index_mut(ds.selected) {
                                view.port_forward.active = false;
                            }
                        }
                    }
                } else if has_ip && ds.detail_selected == 3 {
                    // Snapshot this droplet
                    main.popup = Some(Popup::SnapshotName {
                        droplet_id,
                        input: TextInput::new(&format!("{}-snapshot", droplet_name), false),
                    });
                } else if has_ip && ds.detail_selected == 4 {
                    // Rename droplet
                    main.popup = Some(Popup::RenameDroplet {
                        droplet_id,
                        input: TextInput::new(&droplet_name, false),
                    });
                } else {
                    // Delete
                    main.popup = Some(Popup::Confirm {
                        message: format!("Delete '{droplet_name}'?"),
                        action: PendingAction::DeleteDroplet {
                            id: droplet_id,
                            name: droplet_name,
                        },
                    });
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                let Some(view) = ds.registry.get_by_index(ds.selected) else { return };
                if view.local_status != LocalStatus::Deleting {
                    if let Some(api) = &view.api {
                        let id = api.id;
                        let name = api.name.clone();
                        main.popup = Some(Popup::Confirm {
                            message: format!("Delete '{name}'?"),
                            action: PendingAction::DeleteDroplet { id, name },
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_detail_provision_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else { return };
        let ds = &mut main.droplets;
        let step_count = PROVISION_STEP_NAMES.len();

        match key.code {
            KeyCode::Left | KeyCode::Esc => {
                ds.focus = DFocus::DetailInfo;
                // Reset to most recent step
                if let Some(view) = ds.registry.get_by_index(ds.selected) {
                    ds.provision_selected = view.provision.most_recent_step();
                }
            }
            KeyCode::Up => {
                if ds.provision_selected > 0 {
                    ds.provision_selected -= 1;
                }
            }
            KeyCode::Down => {
                if ds.provision_selected + 1 < step_count {
                    ds.provision_selected += 1;
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                let selected_step = ds.provision_selected;
                let selected_droplet = ds.selected;
                let Some(view) = ds.registry.get_by_index_mut(selected_droplet) else { return };
                let has_failure = matches!(
                    view.provision.steps.get(selected_step).map(|s| &s.status),
                    Some(StepStatus::Failed(_))
                );
                if has_failure {
                    // Clear error state
                    view.provision.error = None;
                    view.provision.steps[selected_step].status = StepStatus::Running;
                    view.provision.current = Some(selected_step);
                    // Clear logs for this step so fresh output shows
                    if let Some(logs) = view.provision.step_logs.get_mut(selected_step) {
                        logs.clear();
                    }
                    let name = view.name.clone();
                    self.run_provision_step(&name, selected_step, tx);
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                let Some(view) = ds.registry.get_by_index(ds.selected) else { return };
                if view.local_status != LocalStatus::Deleting {
                    if let Some(api) = &view.api {
                        let id = api.id;
                        let name = api.name.clone();
                        main.popup = Some(Popup::Confirm {
                            message: format!("Delete '{name}'?"),
                            action: PendingAction::DeleteDroplet { id, name },
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_config_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else { return };

        match key.code {
            KeyCode::Left | KeyCode::Right => {
                main.config.focus = match main.config.focus {
                    CFocus::Github => CFocus::DigitalOcean,
                    CFocus::DigitalOcean => CFocus::Github,
                };
            }
            KeyCode::Up => {
                let info = match main.config.focus {
                    CFocus::Github => &mut main.config.github,
                    CFocus::DigitalOcean => &mut main.config.digitalocean,
                };
                if info.selected > 0 { info.selected -= 1; }
            }
            KeyCode::Down => {
                let info = match main.config.focus {
                    CFocus::Github => &mut main.config.github,
                    CFocus::DigitalOcean => &mut main.config.digitalocean,
                };
                if info.selected < 1 { info.selected += 1; }
            }
            KeyCode::Enter => {
                let focus = main.config.focus;
                let selected = match focus {
                    CFocus::Github => main.config.github.selected,
                    CFocus::DigitalOcean => main.config.digitalocean.selected,
                };

                match (focus, selected) {
                    (CFocus::Github, 0) => {
                        let has_key = config::load().github_ssh_key_path.is_some();
                        if has_key {
                            main.popup = Some(Popup::Confirm {
                                message: "Replace existing GitHub SSH key?".to_string(),
                                action: PendingAction::RegenerateGithubKey,
                            });
                        } else {
                            main.popup = Some(Popup::GithubSetup(GithubSetupPhase::Generating));
                            let tx = tx.clone();
                            std::thread::spawn(move || match ssh::generate_github_ssh_key() {
                                Ok((_, pk)) => { tx.send(Msg::GithubKeyGenerated { public_key: pk }).ok(); }
                                Err(e) => { tx.send(Msg::GithubKeyGenFailed(e.to_string())).ok(); }
                            });
                        }
                    }
                    (CFocus::Github, 1) => {
                        main.config.github.status = KeyStatus::Checking;
                        main.config.github.next_check = CHECK_INTERVAL_TICKS;
                        self.spawn_config_github_check(tx);
                    }
                    (CFocus::DigitalOcean, 0) => {
                        main.popup = Some(Popup::DoSetup(DoSetupPhase::Input(TextInput::new("", true))));
                    }
                    (CFocus::DigitalOcean, 1) => {
                        main.config.digitalocean.status = KeyStatus::Checking;
                        main.config.digitalocean.next_check = CHECK_INTERVAL_TICKS;
                        self.spawn_config_do_check(tx);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_snapshots_key(&mut self, key: KeyEvent, _tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else { return };
        let total = main.snapshots.list.len();

        match key.code {
            KeyCode::Up => {
                if main.snapshots.selected > 0 {
                    main.snapshots.selected -= 1;
                }
            }
            KeyCode::Down => {
                if total > 0 && main.snapshots.selected + 1 < total {
                    main.snapshots.selected += 1;
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if let Some(snap) = main.snapshots.list.get(main.snapshots.selected) {
                    let id = snap.id.clone();
                    let name = snap.name.clone();
                    main.popup = Some(Popup::RenameSnapshot {
                        snapshot_id: id,
                        input: TextInput::new(&name, false),
                    });
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if let Some(snap) = main.snapshots.list.get(main.snapshots.selected) {
                    let id = snap.id.clone();
                    let name = snap.name.clone();
                    main.popup = Some(Popup::Confirm {
                        message: format!("Delete snapshot '{name}'?"),
                        action: PendingAction::DeleteSnapshot { id, name },
                    });
                }
            }
            _ => {}
        }
    }

    // ── Popup Key Handling ──────────────────────────────────────────────────

    fn handle_popup_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else { return };
        let Some(popup) = &mut main.popup else { return };

        match popup {
            Popup::Confirm { action, .. } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let action = std::mem::replace(
                        action,
                        PendingAction::DeleteDroplet { id: 0, name: String::new() },
                    );
                    main.popup = None;
                    self.execute_pending_action(action, tx);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    main.popup = None;
                }
                _ => {}
            },

            Popup::Message(_) => {
                if key.code == KeyCode::Enter || key.code == KeyCode::Esc {
                    main.popup = None;
                }
            }

Popup::SnapshotName { droplet_id, input } => match input.handle_key(key) {
                InputResult::Submit => {
                    let name = input.text.clone();
                    if name.is_empty() { return; }
                    let did = *droplet_id;
                    main.popup = None;
                    self.notification = Some((format!("Creating snapshot '{name}'..."), 50));
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let cfg = config::load();
                        let api_key = cfg.do_api_key.unwrap_or_default();
                        match digitalocean::create_droplet_snapshot(&api_key, did, &name) {
                            Ok(()) => { tx.send(Msg::SnapshotCreateDone { name }).ok(); }
                            Err(e) => { tx.send(Msg::SnapshotCreateFailed { error: e.to_string() }).ok(); }
                        }
                    });
                }
                InputResult::Cancel => { main.popup = None; }
                InputResult::Continue => {}
            },

            Popup::RenameSnapshot { snapshot_id, input } => match input.handle_key(key) {
                InputResult::Submit => {
                    let new_name = input.text.clone();
                    if new_name.is_empty() { return; }
                    let sid = snapshot_id.clone();
                    main.popup = None;
                    let tx = tx.clone();
                    let name_clone = new_name.clone();
                    std::thread::spawn(move || {
                        let cfg = config::load();
                        let api_key = cfg.do_api_key.unwrap_or_default();
                        match digitalocean::rename_snapshot(&api_key, &sid, &name_clone) {
                            Ok(()) => { tx.send(Msg::SnapshotRenameDone { id: sid, new_name: name_clone }).ok(); }
                            Err(e) => { tx.send(Msg::SnapshotRenameFailed { error: e.to_string() }).ok(); }
                        }
                    });
                    self.notification = Some((format!("Renamed to '{new_name}'"), 30));
                }
                InputResult::Cancel => { main.popup = None; }
                InputResult::Continue => {}
            },

            Popup::RenameDroplet { droplet_id, input } => match input.handle_key(key) {
                InputResult::Submit => {
                    let new_name = input.text.clone();
                    if new_name.is_empty() { return; }
                    let did = *droplet_id;
                    main.popup = None;
                    let tx = tx.clone();
                    let name_clone = new_name.clone();
                    std::thread::spawn(move || {
                        let cfg = config::load();
                        let api_key = cfg.do_api_key.unwrap_or_default();
                        match digitalocean::rename_droplet(&api_key, did, &name_clone) {
                            Ok(()) => { tx.send(Msg::DropletRenameDone { id: did, new_name: name_clone }).ok(); }
                            Err(e) => { tx.send(Msg::DropletRenameFailed { error: e.to_string() }).ok(); }
                        }
                    });
                    self.notification = Some((format!("Renaming to '{new_name}'..."), 30));
                }
                InputResult::Cancel => { main.popup = None; }
                InputResult::Continue => {}
            },

            Popup::CreateDroplet(_) => self.handle_create_popup_key(key, tx),

            Popup::GithubSetup(phase) => match phase {
                GithubSetupPhase::Generating => {
                    if key.code == KeyCode::Esc { main.popup = None; }
                }
                GithubSetupPhase::Ready { public_key, copied } => match key.code {
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        ssh::copy_to_clipboard(public_key);
                        *copied = true;
                    }
                    KeyCode::Enter => {
                        let pk = public_key.clone();
                        let c = *copied;
                        *phase = GithubSetupPhase::Testing { public_key: pk, copied: c };
                        let tx = tx.clone();
                        let cfg = config::load();
                        std::thread::spawn(move || {
                            if let Some(path) = cfg.github_ssh_key_path {
                                let (ok, msg) = ssh::test_github_ssh_key(&path);
                                tx.send(Msg::GithubTestResult { success: ok, message: msg }).ok();
                            }
                        });
                    }
                    KeyCode::Esc => { main.popup = None; }
                    _ => {}
                },
                GithubSetupPhase::Testing { .. } => {
                    if key.code == KeyCode::Esc { main.popup = None; }
                }
                GithubSetupPhase::Failed { public_key, copied, .. } => match key.code {
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        ssh::copy_to_clipboard(public_key);
                        *copied = true;
                    }
                    KeyCode::Enter => {
                        let pk = public_key.clone();
                        let c = *copied;
                        *phase = GithubSetupPhase::Testing { public_key: pk, copied: c };
                        let tx = tx.clone();
                        let cfg = config::load();
                        std::thread::spawn(move || {
                            if let Some(path) = cfg.github_ssh_key_path {
                                let (ok, msg) = ssh::test_github_ssh_key(&path);
                                tx.send(Msg::GithubTestResult { success: ok, message: msg }).ok();
                            }
                        });
                    }
                    KeyCode::Esc => { main.popup = None; }
                    _ => {}
                },
                GithubSetupPhase::Done(_) => {
                    if key.code == KeyCode::Enter || key.code == KeyCode::Esc {
                        main.popup = None;
                        main.config.github.next_check = 0;
                    }
                }
            },

            Popup::DoSetup(phase) => match phase {
                DoSetupPhase::Input(input) => match input.handle_key(key) {
                    InputResult::Submit => {
                        let key_value = input.text.clone();
                        if key_value.is_empty() { return; }
                        let mut cfg = config::load();
                        cfg.do_api_key = Some(key_value.clone());
                        config::save(&cfg);
                        *phase = DoSetupPhase::Testing;
                        let tx = tx.clone();
                        std::thread::spawn(move || {
                            let (ok, msg) = ssh::test_do_api_key(&key_value);
                            tx.send(Msg::DoTestResult { success: ok, message: msg }).ok();
                        });
                    }
                    InputResult::Cancel => { main.popup = None; }
                    InputResult::Continue => {}
                },
                DoSetupPhase::Testing => {
                    if key.code == KeyCode::Esc { main.popup = None; }
                }
                DoSetupPhase::Failed(_) => match key.code {
                    KeyCode::Enter => {
                        *phase = DoSetupPhase::Input(TextInput::new("", true));
                    }
                    KeyCode::Esc => { main.popup = None; }
                    _ => {}
                },
                DoSetupPhase::Done(_) => {
                    if key.code == KeyCode::Enter || key.code == KeyCode::Esc {
                        main.popup = None;
                        main.config.digitalocean.next_check = 0;
                    }
                }
            },
        }
    }

    fn handle_create_popup_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else { return };
        let Some(Popup::CreateDroplet(state)) = &mut main.popup else { return };

        match &mut state.step {
            CreateStep::Main { selected } => match key.code {
                KeyCode::Up => { if *selected > 0 { *selected -= 1; } }
                KeyCode::Down => { if *selected < 5 { *selected += 1; } }
                KeyCode::Enter => match *selected {
                    0 => { state.step = CreateStep::Region { selected: state.region_idx }; }
                    1 => { state.step = CreateStep::Machine { selected: state.machine_idx }; }
                    2 => {
                        // Image/Snapshot picker
                        let cur = state.snapshot_idx.map(|i| i + 1).unwrap_or(0);
                        state.step = CreateStep::Snapshot { selected: cur };
                    }
                    3 => {
                        let name = state.name.clone();
                        state.step = CreateStep::Name(TextInput::new(&name, false));
                    }
                    4 => {
                        // Create
                        let name = state.name.clone();
                        let region = REGIONS[state.region_idx].slug.to_string();
                        let machine = MACHINES[state.machine_idx].slug.to_string();
                        let image = match state.snapshot_idx {
                            Some(i) => state.snapshots[i].id.to_string(),
                            None => "ubuntu-24-04-x64".to_string(),
                        };
                        main.popup = None;
                        main.droplets.registry.add_creating(name.clone());
                        let tx = tx.clone();
                        std::thread::spawn(move || {
                            let cfg = config::load();
                            let api_key = cfg.do_api_key.unwrap_or_default();

                            let ssh_key_id = match ssh::ensure_droplet_ssh_key_on_do(&api_key) {
                                Ok(id) => id,
                                Err(e) => {
                                    tx.send(Msg::DropletCreateFailed { name, error: e.to_string() }).ok();
                                    return;
                                }
                            };

                            match digitalocean::create_droplet(&api_key, &name, &region, &machine, &[ssh_key_id], &image) {
                                Ok(_) => {
                                    // Don't send a "done" message. The periodic refresh
                                    // will pick up the new droplet from the API.
                                }
                                Err(e) => {
                                    tx.send(Msg::DropletCreateFailed { name, error: e.to_string() }).ok();
                                }
                            }
                        });
                    }
                    5 => { main.popup = None; }
                    _ => {}
                },
                KeyCode::Esc => { main.popup = None; }
                _ => {}
            },

            CreateStep::Region { selected } => match key.code {
                KeyCode::Up => { if *selected > 0 { *selected -= 1; } }
                KeyCode::Down => { if *selected + 1 < REGIONS.len() { *selected += 1; } }
                KeyCode::Enter => {
                    state.region_idx = *selected;
                    state.step = CreateStep::Main { selected: 0 };
                }
                KeyCode::Esc => { state.step = CreateStep::Main { selected: 0 }; }
                _ => {}
            },

            CreateStep::Machine { selected } => {
                let available_count = MACHINES.iter().filter(|m| m.available).count();
                let available: Vec<usize> = MACHINES.iter().enumerate()
                    .filter(|(_, m)| m.available).map(|(i, _)| i).collect();
                match key.code {
                    KeyCode::Up => { if *selected > 0 { *selected -= 1; } }
                    KeyCode::Down => { if *selected + 1 < available_count { *selected += 1; } }
                    KeyCode::Enter => {
                        if *selected < available.len() {
                            state.machine_idx = available[*selected];
                        }
                        state.step = CreateStep::Main { selected: 1 };
                    }
                    KeyCode::Esc => { state.step = CreateStep::Main { selected: 1 }; }
                    _ => {}
                }
            }

            CreateStep::Snapshot { selected } => {
                let total = state.snapshots.len() + 1; // +1 for "None (base image)"
                match key.code {
                    KeyCode::Up => { if *selected > 0 { *selected -= 1; } }
                    KeyCode::Down => { if *selected + 1 < total { *selected += 1; } }
                    KeyCode::Enter => {
                        state.snapshot_idx = if *selected == 0 { None } else { Some(*selected - 1) };
                        state.step = CreateStep::Main { selected: 2 };
                    }
                    KeyCode::Esc => { state.step = CreateStep::Main { selected: 2 }; }
                    _ => {}
                }
            }

            CreateStep::Name(input) => match input.handle_key(key) {
                InputResult::Submit => {
                    if !input.text.is_empty() {
                        state.name = input.text.clone();
                    }
                    state.step = CreateStep::Main { selected: 3 };
                }
                InputResult::Cancel => {
                    state.step = CreateStep::Main { selected: 3 };
                }
                InputResult::Continue => {}
            },
        }
    }

    fn execute_pending_action(&mut self, action: PendingAction, tx: &Sender<Msg>) {
        match action {
            PendingAction::DeleteDroplet { id, ref name } => {
                if let Screen::Main(main) = &mut self.screen {
                    // Kill tunnel if active
                    if let Some(mut child) = main.droplets.tunnels.remove(name) {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    main.droplets.registry.mark_deleting(id);
                    main.droplets.focus = DFocus::DetailInfo;
                }
                let tx = tx.clone();
                let cfg = config::load();
                let api_key = cfg.do_api_key.unwrap_or_default();
                std::thread::spawn(move || {
                    match digitalocean::delete_droplet(&api_key, id) {
                        Ok(()) => {
                            // Don't send a "done" message. The periodic refresh
                            // will notice the droplet is gone from the API.
                        }
                        Err(e) => {
                            tx.send(Msg::DropletDeleteFailed { id, error: e.to_string() }).ok();
                        }
                    }
                });
            }
            PendingAction::DeleteSnapshot { ref id, ref name } => {
                self.notification = Some((format!("Deleting snapshot '{name}'..."), 30));
                let tx = tx.clone();
                let id = id.clone();
                std::thread::spawn(move || {
                    let cfg = config::load();
                    let api_key = cfg.do_api_key.unwrap_or_default();
                    match digitalocean::delete_snapshot(&api_key, &id) {
                        Ok(()) => { tx.send(Msg::SnapshotDeleteDone { id }).ok(); }
                        Err(e) => { tx.send(Msg::SnapshotDeleteFailed { error: e.to_string() }).ok(); }
                    }
                });
            }
            PendingAction::RegenerateGithubKey => {
                if let Screen::Main(main) = &mut self.screen {
                    main.popup = Some(Popup::GithubSetup(GithubSetupPhase::Generating));
                }
                let tx = tx.clone();
                std::thread::spawn(move || match ssh::generate_github_ssh_key() {
                    Ok((_, pk)) => { tx.send(Msg::GithubKeyGenerated { public_key: pk }).ok(); }
                    Err(e) => { tx.send(Msg::GithubKeyGenFailed(e.to_string())).ok(); }
                });
            }
        }
    }

    // ── Handle Messages ─────────────────────────────────────────────────────

    pub fn handle_message(&mut self, msg: Msg, tx: &Sender<Msg>) {
        match msg {
            Msg::GithubKeyMissing => {
                if let Screen::Welcome(state) = &mut self.screen {
                    state.phase = WelcomePhase::GithubMissing;
                }
            }

            Msg::GithubKeyGenerated { public_key } => {
                match &mut self.screen {
                    Screen::Welcome(state) => {
                        state.phase = WelcomePhase::GithubGenerated { public_key, copied: false };
                    }
                    Screen::Main(main) => {
                        if let Some(Popup::GithubSetup(phase)) = &mut main.popup {
                            *phase = GithubSetupPhase::Ready { public_key, copied: false };
                        }
                    }
                }
            }

            Msg::GithubKeyGenFailed(error) => {
                match &mut self.screen {
                    Screen::Welcome(state) => {
                        state.phase = WelcomePhase::GithubFailed { error, public_key: None, copied: false };
                    }
                    Screen::Main(main) => {
                        main.popup = Some(Popup::Message(format!("Key generation failed: {error}")));
                    }
                }
            }

            Msg::GithubTestResult { success, message } => {
                match &mut self.screen {
                    Screen::Welcome(state) => {
                        if success {
                            state.phase = WelcomePhase::GithubOk(message);
                            state.auto_advance = AUTO_ADVANCE_TICKS;
                        } else {
                            let (pk, copied) = match &state.phase {
                                WelcomePhase::TestingGithub { public_key, copied } => {
                                    (public_key.clone(), *copied)
                                }
                                _ => (None, false),
                            };
                            state.phase = WelcomePhase::GithubFailed { error: message, public_key: pk, copied };
                        }
                    }
                    Screen::Main(main) => {
                        if let Some(Popup::GithubSetup(phase)) = &mut main.popup {
                            if success {
                                *phase = GithubSetupPhase::Done(message);
                            } else {
                                let (pk, c) = match phase {
                                    GithubSetupPhase::Testing { public_key, copied } => (public_key.clone(), *copied),
                                    _ => (String::new(), false),
                                };
                                *phase = GithubSetupPhase::Failed { error: message, public_key: pk, copied: c };
                            }
                        }
                    }
                }
            }

            Msg::DoKeyExists => {
                // No action needed — DoTestResult will follow from the parallel thread
            }

            Msg::DoKeyMissing => {
                if let Screen::Welcome(state) = &mut self.screen {
                    let in_github_phase = matches!(
                        state.phase,
                        WelcomePhase::CheckingGithub
                            | WelcomePhase::GithubOk(_)
                            | WelcomePhase::GithubMissing
                            | WelcomePhase::GeneratingGithub
                            | WelcomePhase::GithubGenerated { .. }
                            | WelcomePhase::TestingGithub { .. }
                            | WelcomePhase::GithubFailed { .. }
                    );
                    if in_github_phase {
                        state.do_early_result = Some(DoEarlyResult::Missing);
                    } else {
                        state.phase = WelcomePhase::DoMissing;
                    }
                }
            }

            Msg::DoTestResult { success, message } => {
                match &mut self.screen {
                    Screen::Welcome(state) => {
                        let in_github_phase = matches!(
                            state.phase,
                            WelcomePhase::CheckingGithub
                                | WelcomePhase::GithubOk(_)
                                | WelcomePhase::GithubMissing
                                | WelcomePhase::GeneratingGithub
                                | WelcomePhase::GithubGenerated { .. }
                                | WelcomePhase::TestingGithub { .. }
                                | WelcomePhase::GithubFailed { .. }
                        );
                        if in_github_phase {
                            state.do_early_result = Some(if success {
                                DoEarlyResult::TestOk(message)
                            } else {
                                DoEarlyResult::TestFailed(message)
                            });
                        } else if success {
                            state.phase = WelcomePhase::DoOk(message);
                            state.auto_advance = AUTO_ADVANCE_TICKS;
                        } else {
                            state.phase = WelcomePhase::DoFailed(message);
                        }
                    }
                    Screen::Main(main) => {
                        if let Some(Popup::DoSetup(phase)) = &mut main.popup {
                            if success {
                                *phase = DoSetupPhase::Done(message);
                            } else {
                                *phase = DoSetupPhase::Failed(message);
                            }
                        }
                    }
                }
            }

            Msg::DropletsLoaded(droplets) => {
                let mut needs_check = Vec::new();

                if let Screen::Main(main) = &mut self.screen {
                    main.droplets.registry.merge_api_data(droplets);
                    main.droplets.loading = false;

                    // Clamp selection
                    let total = main.droplets.registry.len() + 1;
                    if main.droplets.selected >= total {
                        main.droplets.selected = total.saturating_sub(1);
                    }

                    // Collect droplets that need remote state check
                    // (both newly created and newly discovered active droplets)
                    needs_check = main.droplets.registry.views()
                        .iter()
                        .filter(|v| {
                            v.provision.needs_check
                                && v.api.as_ref().map_or(false, |a| a.ip.is_some())
                        })
                        .map(|v| {
                            (
                                v.name.clone(),
                                v.api.as_ref().unwrap().ip.clone().unwrap(),
                            )
                        })
                        .collect();

                    // Clear needs_check to prevent re-triggering on next refresh
                    for (name, _) in &needs_check {
                        if let Some(view) = main.droplets.registry.find_by_name_mut(name) {
                            view.provision.needs_check = false;
                        }
                    }

                    // Sync hosts mapping state from /etc/hosts
                    for view in main.droplets.registry.views.iter_mut() {
                        view.hosts_mapped = ssh::is_host_mapped(&view.name);
                    }
                }

                for (name, ip) in needs_check {
                    self.spawn_provision_check(&name, &ip, tx);
                }
            }

            Msg::DropletCreateFailed { name, error } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.droplets.registry.mark_create_failed(&name);
                }
                self.notification = Some((format!("Create failed: {error}"), 50));
            }

            Msg::DropletDeleteFailed { id, error } => {
                // Unmark deleting — the refresh will show it back as normal
                // (the delete API call failed, so it's still there)
                if let Screen::Main(main) = &mut self.screen {
                    if let Some(view) = main.droplets.registry.views.iter_mut().find(|v| {
                        v.api.as_ref().map_or(false, |a| a.id == id)
                    }) {
                        view.local_status = LocalStatus::Normal;
                    }
                }
                self.notification = Some((format!("Delete failed: {error}"), 50));
            }

            Msg::DropletRenameDone { id, new_name } => {
                if let Screen::Main(main) = &mut self.screen {
                    if let Some(view) = main.droplets.registry.views.iter_mut().find(|v| {
                        v.api.as_ref().map_or(false, |a| a.id == id)
                    }) {
                        view.name = new_name.clone();
                        if let Some(api) = &mut view.api {
                            api.name = new_name.clone();
                        }
                    }
                }
                self.notification = Some((format!("Renamed to '{new_name}'"), 30));
            }

            Msg::DropletRenameFailed { error } => {
                self.notification = Some((format!("Rename failed: {error}"), 50));
            }

            Msg::HostsMappingDone { name, mapped } => {
                if let Screen::Main(main) = &mut self.screen {
                    if let Some(view) = main.droplets.registry.find_by_name_mut(&name) {
                        view.hosts_mapped = mapped;
                    }
                    if mapped {
                        self.notification = Some((
                            format!("http://{name}.droplet ready"),
                            50,
                        ));
                    } else {
                        self.notification = Some((
                            format!("{name}.droplet removed from /etc/hosts"),
                            30,
                        ));
                    }
                }
            }

            Msg::HostsMappingFailed { error } => {
                self.notification = Some((format!("Hosts mapping failed: {error}"), 50));
            }

            Msg::ProvisionStepDone { name, step_idx } => {
                if let Screen::Main(main) = &mut self.screen {
                    if let Some(view) = main.droplets.registry.find_by_name_mut(&name) {
                        if let Some(step) = view.provision.steps.get_mut(step_idx) {
                            step.status = StepStatus::Done;
                        }
                        let next = step_idx + 1;
                        if next < view.provision.steps.len() {
                            view.provision.current = Some(next);
                            view.provision.steps[next].status = StepStatus::Running;
                            self.run_provision_step(&name, next, tx);
                        } else {
                            view.provision.current = None;
                        }
                    }
                }
            }

            Msg::ProvisionStepFailed { name, step_idx, error } => {
                if let Screen::Main(main) = &mut self.screen {
                    if let Some(view) = main.droplets.registry.find_by_name_mut(&name) {
                        if let Some(step) = view.provision.steps.get_mut(step_idx) {
                            step.status = StepStatus::Failed(error.clone());
                        }
                        view.provision.error = Some(error);
                        view.provision.current = None;
                    }
                }
            }

            Msg::ProvisionLog { name, step_idx, line } => {
                if let Screen::Main(main) = &mut self.screen {
                    if let Some(view) = main.droplets.registry.find_by_name_mut(&name) {
                        if let Some(logs) = view.provision.step_logs.get_mut(step_idx) {
                            logs.push(line);
                            if logs.len() > 200 {
                                logs.remove(0);
                            }
                        }
                    }
                }
            }

            Msg::ProvisionStateChecked { name, completed_steps } => {
                let mut start_step = None;
                if let Screen::Main(main) = &mut self.screen {
                    if let Some(view) = main.droplets.registry.find_by_name_mut(&name) {
                        for (i, done) in completed_steps.iter().enumerate() {
                            if *done {
                                if let Some(step) = view.provision.steps.get_mut(i) {
                                    step.status = StepStatus::Done;
                                }
                            }
                        }
                        let first_incomplete = completed_steps.iter().position(|&done| !done);
                        match first_incomplete {
                            Some(idx) => {
                                view.provision.current = Some(idx);
                                view.provision.steps[idx].status = StepStatus::Running;
                                start_step = Some(idx);
                            }
                            None => {
                                view.provision.current = None; // all done
                            }
                        }
                    }
                }
                if let Some(idx) = start_step {
                    self.run_provision_step(&name, idx, tx);
                }
            }

            Msg::SnapshotsLoaded(snapshots) => {
                if let Screen::Main(main) = &mut self.screen {
                    // Check if any pending snapshots have appeared
                    let mut completed = Vec::new();
                    main.snapshots.pending.retain(|name| {
                        if snapshots.iter().any(|s| s.name == *name) {
                            completed.push(name.clone());
                            false
                        } else {
                            true
                        }
                    });
                    for name in &completed {
                        self.notification = Some((format!("Snapshot '{name}' ready"), 50));
                    }

                    main.snapshots.list = snapshots;
                    main.snapshots.loading = false;
                    if main.snapshots.selected >= main.snapshots.list.len() && !main.snapshots.list.is_empty() {
                        main.snapshots.selected = main.snapshots.list.len() - 1;
                    }

                    // Poll faster while snapshots are pending
                    if !main.snapshots.pending.is_empty() {
                        main.snapshots.refresh_countdown = REFRESH_INTERVAL_TICKS;
                    }
                }
            }

            Msg::SnapshotCreateDone { name } => {
                self.notification = Some((format!("Snapshot '{name}' in progress..."), 50));
                if let Screen::Main(main) = &mut self.screen {
                    main.snapshots.pending.push(name);
                    // Poll faster while creating
                    main.snapshots.refresh_countdown = REFRESH_INTERVAL_TICKS;
                }
            }

            Msg::SnapshotCreateFailed { error } => {
                self.notification = Some((format!("Snapshot failed: {error}"), 50));
            }

            Msg::SnapshotDeleteDone { id } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.snapshots.list.retain(|s| s.id != id);
                    if main.snapshots.selected >= main.snapshots.list.len() && !main.snapshots.list.is_empty() {
                        main.snapshots.selected = main.snapshots.list.len() - 1;
                    }
                }
                self.notification = Some(("Snapshot deleted".to_string(), 30));
            }

            Msg::SnapshotDeleteFailed { error } => {
                self.notification = Some((format!("Delete snapshot failed: {error}"), 50));
            }

            Msg::SnapshotRenameDone { id, new_name } => {
                if let Screen::Main(main) = &mut self.screen {
                    if let Some(snap) = main.snapshots.list.iter_mut().find(|s| s.id == id) {
                        snap.name = new_name;
                    }
                }
            }

            Msg::SnapshotRenameFailed { error } => {
                self.notification = Some((format!("Rename failed: {error}"), 50));
            }

            Msg::ConfigGithubCheck { success, message } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.config.github.status = if success { KeyStatus::Ok } else { KeyStatus::Error };
                    main.config.github.message = Some(message);
                }
            }

            Msg::ConfigDoCheck { success, message } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.config.digitalocean.status = if success { KeyStatus::Ok } else { KeyStatus::Error };
                    main.config.digitalocean.message = Some(message);
                }
            }
        }
    }

    // ── Provisioning ────────────────────────────────────────────────────────

    fn start_provisioning(&self, name: &str, tx: &Sender<Msg>) {
        self.run_provision_step(name, 0, tx);
    }

    fn run_provision_step(&self, name: &str, step_idx: usize, tx: &Sender<Msg>) {
        let Screen::Main(main) = &self.screen else { return };
        let Some(view) = main.droplets.registry.views().iter().find(|v| v.name == name) else { return };
        let Some(api) = &view.api else { return };
        let Some(ip) = &api.ip else { return };

        let ip = ip.clone();
        let name = name.to_string();
        let tx = tx.clone();
        let cfg = config::load();
        let droplet_key = cfg.droplet_ssh_key_path.unwrap_or_default();

        std::thread::spawn(move || {
            let log_name = name.clone();
            let log_tx = tx.clone();
            let on_log = move |line: &str| {
                log_tx
                    .send(Msg::ProvisionLog {
                        name: log_name.clone(),
                        step_idx,
                        line: line.to_string(),
                    })
                    .ok();
            };

            let result = match step_idx {
                0 => ssh::provision_transport_github_key(&droplet_key, &ip, &on_log),
                1 => ssh::provision_verify_github_key(&droplet_key, &ip, &on_log),
                2 => ssh::provision_install_docker(&droplet_key, &ip, &on_log),
                3 => ssh::provision_verify_docker(&droplet_key, &ip, &on_log),
                4 => ssh::provision_install_flox(&droplet_key, &ip, &on_log),
                5 => ssh::provision_verify_flox(&droplet_key, &ip, &on_log),
                6 => ssh::provision_install_build_essential(&droplet_key, &ip, &on_log),
                7 => ssh::provision_verify_build_essential(&droplet_key, &ip, &on_log),
                8 => ssh::provision_clone_posthog(&droplet_key, &ip, &on_log),
                9 => ssh::provision_verify_posthog_clone(&droplet_key, &ip, &on_log),
                10 => ssh::provision_pull_latest_main(&droplet_key, &ip, &on_log),
                11 => ssh::provision_flox_activate(&droplet_key, &ip, &on_log),
                _ => Ok(()),
            };

            match result {
                Ok(()) => {
                    tx.send(Msg::ProvisionStepDone { name, step_idx }).ok();
                }
                Err(e) => {
                    tx.send(Msg::ProvisionStepFailed {
                        name,
                        step_idx,
                        error: e.to_string(),
                    })
                    .ok();
                }
            }
        });
    }

    fn spawn_provision_check(&self, name: &str, ip: &str, tx: &Sender<Msg>) {
        let name = name.to_string();
        let ip = ip.to_string();
        let tx = tx.clone();
        let cfg = config::load();
        let droplet_key = cfg.droplet_ssh_key_path.unwrap_or_default();
        let total = PROVISION_STEP_NAMES.len();

        std::thread::spawn(move || {
            match ssh::check_provision_markers(&droplet_key, &ip, total) {
                Ok(markers) => {
                    tx.send(Msg::ProvisionStateChecked {
                        name,
                        completed_steps: markers,
                    })
                    .ok();
                }
                Err(_) => {
                    // SSH failed — could be droplet not ready. Will be retried
                    // if needs_check gets set again (won't by default, so the
                    // droplet stays in "pending" state).
                }
            }
        });
    }

    // ── Background Task Spawning ────────────────────────────────────────────

    fn spawn_refresh_droplets(&self, tx: &Sender<Msg>) {
        let tx = tx.clone();
        let cfg = config::load();
        std::thread::spawn(move || {
            if let Some(api_key) = cfg.do_api_key {
                match digitalocean::list_droplets(&api_key) {
                    Ok(droplets) => { tx.send(Msg::DropletsLoaded(droplets)).ok(); }
                    Err(_) => { tx.send(Msg::DropletsLoaded(vec![])).ok(); }
                }
            } else {
                tx.send(Msg::DropletsLoaded(vec![])).ok();
            }
        });
    }

    fn spawn_refresh_snapshots(&self, tx: &Sender<Msg>) {
        let tx = tx.clone();
        let cfg = config::load();
        std::thread::spawn(move || {
            if let Some(api_key) = cfg.do_api_key {
                match digitalocean::list_snapshots(&api_key) {
                    Ok(snapshots) => { tx.send(Msg::SnapshotsLoaded(snapshots)).ok(); }
                    Err(_) => { tx.send(Msg::SnapshotsLoaded(vec![])).ok(); }
                }
            } else {
                tx.send(Msg::SnapshotsLoaded(vec![])).ok();
            }
        });
    }

    fn spawn_config_github_check(&self, tx: &Sender<Msg>) {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let cfg = config::load();
            let (ok, msg) = match cfg.github_ssh_key_path {
                Some(path) if std::path::Path::new(&path).exists() => ssh::test_github_ssh_key(&path),
                _ => (false, "No key configured".to_string()),
            };
            tx.send(Msg::ConfigGithubCheck { success: ok, message: msg }).ok();
        });
    }

    fn spawn_config_do_check(&self, tx: &Sender<Msg>) {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let cfg = config::load();
            let (ok, msg) = match cfg.do_api_key {
                Some(key) if !key.is_empty() => ssh::test_do_api_key(&key),
                _ => (false, "No key configured".to_string()),
            };
            tx.send(Msg::ConfigDoCheck { success: ok, message: msg }).ok();
        });
    }

    pub fn cleanup(&mut self) {
        if let Screen::Main(main) = &mut self.screen {
            for (_, mut child) in main.droplets.tunnels.drain() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
