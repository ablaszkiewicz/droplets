use std::collections::HashSet;
use std::sync::mpsc::Sender;

use crossterm::event::{KeyCode, KeyEvent};

use crate::backend::{config, digitalocean, ssh};
use crate::types::{DropletInfo, MACHINES, REGIONS};

// ── Tick constants (1 tick = ~100ms) ────────────────────────────────────────

const AUTO_ADVANCE_TICKS: u32 = 15; // 1.5s
const CHECK_INTERVAL_TICKS: u32 = 50; // 5s
const REFRESH_INTERVAL_TICKS: u32 = 50; // 5s

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

    // Main: Droplets
    DropletsLoaded(Vec<DropletInfo>),
    DropletCreateDone { name: String },
    DropletCreateFailed { name: String, error: String },
    DropletDeleted(i64),
    DropletDeleteFailed { id: i64, error: String },

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
    pub notification: Option<(String, u32)>, // (message, ticks remaining)
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

    AllReady,
}

// ── Main State ──────────────────────────────────────────────────────────────

pub struct MainState {
    pub tab: Tab,
    pub droplets: DropletsState,
    pub config: ConfigViewState,
    pub popup: Option<Popup>,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Tab {
    Droplets,
    Config,
}

pub struct DropletsState {
    pub items: Vec<DropletInfo>,
    pub selected: usize,
    pub focus: DFocus,
    pub detail_selected: usize,
    pub creating: Vec<String>,
    pub deleting: HashSet<i64>,
    pub refresh_countdown: u32,
    pub loading: bool,
}

#[derive(PartialEq, Clone, Copy)]
pub enum DFocus {
    List,
    Detail,
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
    Confirm {
        message: String,
        action: PendingAction,
    },
    GithubSetup(GithubSetupPhase),
    DoSetup(DoSetupPhase),
    Message(String),
}

pub enum PendingAction {
    DeleteDroplet { id: i64, name: String },
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
    pub name: String,
}

pub enum CreateStep {
    Main { selected: usize },
    Region { selected: usize },
    Machine { selected: usize },
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
            }),
            should_quit: false,
            spinner_idx: 0,
            tick_count: 0,
            notification: None,
        }
    }

    /// Returns total item count for the droplets list (items + creating + separator + create option)
    pub fn droplet_list_len(&self) -> usize {
        if let Screen::Main(main) = &self.screen {
            let base = main.droplets.items.len() + main.droplets.creating.len();
            base + 1 // +1 for "+ Create new"
        } else {
            0
        }
    }

    pub fn start_initial_check(&self, tx: &Sender<Msg>) {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let cfg = config::load();
            match cfg.github_ssh_key_path {
                Some(path) if std::path::Path::new(&path).exists() => {
                    let (ok, msg) = ssh::test_github_ssh_key(&path);
                    tx.send(Msg::GithubTestResult {
                        success: ok,
                        message: msg,
                    })
                    .ok();
                }
                _ => {
                    tx.send(Msg::GithubKeyMissing).ok();
                }
            }
        });
    }

    // ── Tick ────────────────────────────────────────────────────────────────

    pub fn tick(&mut self, tx: &Sender<Msg>) {
        self.tick_count += 1;
        self.spinner_idx = (self.spinner_idx + 1) % 10;

        // Decrement notification timer
        if let Some((_, ref mut ticks)) = self.notification {
            if *ticks > 0 {
                *ticks -= 1;
            } else {
                self.notification = None;
            }
        }

        // Collect what needs to happen without holding mutable borrow
        let mut do_advance_welcome = false;
        let mut do_refresh_droplets = false;
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
            }
        }

        // Now execute actions without conflicting borrows
        if do_advance_welcome {
            self.advance_welcome(tx);
        }
        if do_refresh_droplets {
            self.spawn_refresh_droplets(tx);
        }
        if do_check_github {
            self.spawn_config_github_check(tx);
        }
        if do_check_do {
            self.spawn_config_do_check(tx);
        }
    }

    fn advance_welcome(&mut self, tx: &Sender<Msg>) {
        if let Screen::Welcome(state) = &mut self.screen {
            match &state.phase {
                WelcomePhase::GithubOk(_) => {
                    let msg = if let WelcomePhase::GithubOk(m) = &state.phase {
                        m.clone()
                    } else {
                        String::new()
                    };
                    state.github_done_msg = Some(msg);
                    state.phase = WelcomePhase::CheckingDo;
                    state.auto_advance = 0;
                    // Start DO check
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let cfg = config::load();
                        match cfg.do_api_key {
                            Some(key) if !key.is_empty() => {
                                tx.send(Msg::DoKeyExists).ok();
                                let (ok, msg) = ssh::test_do_api_key(&key);
                                tx.send(Msg::DoTestResult {
                                    success: ok,
                                    message: msg,
                                })
                                .ok();
                            }
                            _ => {
                                tx.send(Msg::DoKeyMissing).ok();
                            }
                        }
                    });
                }
                WelcomePhase::DoOk(_) | WelcomePhase::AllReady => {
                    self.transition_to_main(tx);
                }
                _ => {}
            }
        }
    }

    fn transition_to_main(&mut self, _tx: &Sender<Msg>) {
        self.screen = Screen::Main(MainState {
            tab: Tab::Droplets,
            droplets: DropletsState {
                items: vec![],
                selected: 0,
                focus: DFocus::List,
                detail_selected: 0,
                creating: vec![],
                deleting: HashSet::new(),
                refresh_countdown: 0, // load immediately
                loading: true,
            },
            config: ConfigViewState {
                focus: CFocus::Github,
                github: KeyCheckInfo {
                    status: KeyStatus::Unknown,
                    message: None,
                    selected: 0,
                    next_check: 0, // check immediately
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
        // Global quit (but not during text input)
        if key.code == KeyCode::Char('q') && !self.is_text_input_active() {
            self.should_quit = true;
            return;
        }

        match &self.screen {
            Screen::Welcome(_) => self.handle_welcome_key(key, tx),
            Screen::Main(_) => {
                // Popup gets priority
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
            WelcomePhase::CheckingGithub | WelcomePhase::CheckingDo | WelcomePhase::TestingDo => {
                if key.code == KeyCode::Esc {
                    self.transition_to_main(tx);
                }
            }

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
                            tx.send(Msg::GithubKeyGenerated { public_key: pub_key })
                                .ok();
                        }
                        Err(e) => {
                            tx.send(Msg::GithubKeyGenFailed(e.to_string())).ok();
                        }
                    });
                }
                KeyCode::Esc => self.transition_to_main(tx),
                _ => {}
            },

            WelcomePhase::GeneratingGithub => {
                if key.code == KeyCode::Esc {
                    self.transition_to_main(tx);
                }
            }

            WelcomePhase::GithubGenerated {
                public_key, copied, ..
            } => match key.code {
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
                            tx.send(Msg::GithubTestResult {
                                success: ok,
                                message: msg,
                            })
                            .ok();
                        }
                    });
                }
                KeyCode::Esc => self.transition_to_main(tx),
                _ => {}
            },

            WelcomePhase::TestingGithub { .. } => {
                if key.code == KeyCode::Esc {
                    self.transition_to_main(tx);
                }
            }

            WelcomePhase::GithubFailed {
                public_key, copied, ..
            } => match key.code {
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
                            tx.send(Msg::GithubTestResult {
                                success: ok,
                                message: msg,
                            })
                            .ok();
                        }
                    });
                }
                KeyCode::Esc => self.transition_to_main(tx),
                _ => {}
            },

            WelcomePhase::DoMissing => match key.code {
                KeyCode::Enter => {
                    state.phase = WelcomePhase::DoInput(TextInput::new("", true));
                }
                KeyCode::Esc => self.transition_to_main(tx),
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
                        tx.send(Msg::DoTestResult {
                            success: ok,
                            message: msg,
                        })
                        .ok();
                    });
                }
                InputResult::Cancel => {
                    self.transition_to_main(tx);
                }
                InputResult::Continue => {}
            },

            WelcomePhase::DoFailed(_) => match key.code {
                KeyCode::Enter => {
                    state.phase = WelcomePhase::DoInput(TextInput::new("", true));
                }
                KeyCode::Esc => self.transition_to_main(tx),
                _ => {}
            },

            WelcomePhase::AllReady => {
                self.transition_to_main(tx);
            }
        }
    }

    // ── Main Key Handling ───────────────────────────────────────────────────

    fn handle_main_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else {
            return;
        };

        match key.code {
            KeyCode::Tab => {
                main.tab = match main.tab {
                    Tab::Droplets => Tab::Config,
                    Tab::Config => Tab::Droplets,
                };
            }
            _ => match main.tab {
                Tab::Droplets => self.handle_droplets_key(key, tx),
                Tab::Config => self.handle_config_key(key, tx),
            },
        }
    }

    fn handle_droplets_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else {
            return;
        };

        match main.droplets.focus {
            DFocus::List => self.handle_droplets_list_key(key, tx),
            DFocus::Detail => self.handle_droplets_detail_key(key, tx),
        }
    }

    fn handle_droplets_list_key(&mut self, key: KeyEvent, _tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else {
            return;
        };
        let ds = &mut main.droplets;
        let total = ds.items.len() + ds.creating.len() + 1; // +1 for Create

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
                let items_len = ds.items.len();
                let creating_len = ds.creating.len();

                if ds.selected < items_len {
                    // Selected an actual droplet
                    let droplet = &ds.items[ds.selected];
                    if !ds.deleting.contains(&droplet.id) {
                        ds.focus = DFocus::Detail;
                        ds.detail_selected = 0;
                    }
                } else if ds.selected >= items_len + creating_len {
                    // "+ Create new"
                    let count = items_len + creating_len;
                    main.popup = Some(Popup::CreateDroplet(CreatePopupState {
                        step: CreateStep::Main { selected: 0 },
                        region_idx: 2, // nyc1 (New York)
                        machine_idx: 0,
                        name: format!("droplet-{}", count + 1),
                    }));
                }
                // Else it's a creating item, do nothing
            }
            _ => {}
        }
    }

    fn handle_droplets_detail_key(&mut self, key: KeyEvent, _tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else {
            return;
        };
        let ds = &mut main.droplets;

        // How many actions? If droplet has IP: copy-ssh + delete = 2. Else: delete = 1.
        let selected_idx = ds.selected;
        let action_count = if selected_idx < ds.items.len() {
            if ds.items[selected_idx].ip.is_some() {
                2
            } else {
                1
            }
        } else {
            0
        };

        match key.code {
            KeyCode::Left | KeyCode::Esc => {
                ds.focus = DFocus::List;
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
                if selected_idx >= ds.items.len() {
                    return;
                }
                let droplet = ds.items[selected_idx].clone();
                let has_ip = droplet.ip.is_some();

                let action_idx = ds.detail_selected;
                if has_ip && action_idx == 0 {
                    // Copy SSH
                    if let Some(ref ip) = droplet.ip {
                        let cfg = config::load();
                        let key_path = cfg.droplet_ssh_key_path.unwrap_or_default();
                        let cmd = format!(
                            "ssh -i {key_path} -o StrictHostKeyChecking=accept-new root@{ip}"
                        );
                        let copied = ssh::copy_to_clipboard(&cmd);
                        self.notification = Some((
                            if copied {
                                format!("Copied: {cmd}")
                            } else {
                                cmd
                            },
                            30,
                        ));
                    }
                } else {
                    // Delete
                    main.popup = Some(Popup::Confirm {
                        message: format!("Delete '{}'?", droplet.name),
                        action: PendingAction::DeleteDroplet {
                            id: droplet.id,
                            name: droplet.name,
                        },
                    });
                }
            }
            _ => {}
        }
    }

    fn handle_config_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else {
            return;
        };

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
                if info.selected > 0 {
                    info.selected -= 1;
                }
            }
            KeyCode::Down => {
                let info = match main.config.focus {
                    CFocus::Github => &mut main.config.github,
                    CFocus::DigitalOcean => &mut main.config.digitalocean,
                };
                if info.selected < 1 {
                    info.selected += 1;
                }
            }
            KeyCode::Enter => {
                let focus = main.config.focus;
                let info = match focus {
                    CFocus::Github => &main.config.github,
                    CFocus::DigitalOcean => &main.config.digitalocean,
                };
                let selected = info.selected;

                match (focus, selected) {
                    (CFocus::Github, 0) => {
                        // Set up again
                        let has_key = config::load().github_ssh_key_path.is_some();
                        if has_key {
                            main.popup = Some(Popup::Confirm {
                                message: "Replace existing GitHub SSH key?".to_string(),
                                action: PendingAction::RegenerateGithubKey,
                            });
                        } else {
                            main.popup =
                                Some(Popup::GithubSetup(GithubSetupPhase::Generating));
                            let tx = tx.clone();
                            std::thread::spawn(move || match ssh::generate_github_ssh_key() {
                                Ok((_, pk)) => {
                                    tx.send(Msg::GithubKeyGenerated { public_key: pk }).ok();
                                }
                                Err(e) => {
                                    tx.send(Msg::GithubKeyGenFailed(e.to_string())).ok();
                                }
                            });
                        }
                    }
                    (CFocus::Github, 1) => {
                        // Test now
                        main.config.github.status = KeyStatus::Checking;
                        main.config.github.next_check = CHECK_INTERVAL_TICKS;
                        self.spawn_config_github_check(tx);
                    }
                    (CFocus::DigitalOcean, 0) => {
                        // Set up again
                        main.popup = Some(Popup::DoSetup(DoSetupPhase::Input(TextInput::new(
                            "", true,
                        ))));
                    }
                    (CFocus::DigitalOcean, 1) => {
                        // Test now
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

    // ── Popup Key Handling ──────────────────────────────────────────────────

    fn handle_popup_key(&mut self, key: KeyEvent, tx: &Sender<Msg>) {
        let Screen::Main(main) = &mut self.screen else {
            return;
        };

        let popup = match &mut main.popup {
            Some(p) => p,
            None => return,
        };

        match popup {
            Popup::Confirm { action, .. } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let action = std::mem::replace(
                        action,
                        PendingAction::DeleteDroplet {
                            id: 0,
                            name: String::new(),
                        },
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

            Popup::CreateDroplet(_) => self.handle_create_popup_key(key, tx),

            Popup::GithubSetup(phase) => match phase {
                GithubSetupPhase::Generating => {
                    if key.code == KeyCode::Esc {
                        main.popup = None;
                    }
                }
                GithubSetupPhase::Ready { public_key, copied } => match key.code {
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        ssh::copy_to_clipboard(public_key);
                        *copied = true;
                    }
                    KeyCode::Enter => {
                        let pk = public_key.clone();
                        let c = *copied;
                        *phase = GithubSetupPhase::Testing {
                            public_key: pk,
                            copied: c,
                        };
                        let tx = tx.clone();
                        let cfg = config::load();
                        std::thread::spawn(move || {
                            if let Some(path) = cfg.github_ssh_key_path {
                                let (ok, msg) = ssh::test_github_ssh_key(&path);
                                tx.send(Msg::GithubTestResult {
                                    success: ok,
                                    message: msg,
                                })
                                .ok();
                            }
                        });
                    }
                    KeyCode::Esc => {
                        main.popup = None;
                    }
                    _ => {}
                },
                GithubSetupPhase::Testing { .. } => {
                    if key.code == KeyCode::Esc {
                        main.popup = None;
                    }
                }
                GithubSetupPhase::Failed {
                    public_key, copied, ..
                } => match key.code {
                    KeyCode::Char('c') | KeyCode::Char('C') => {
                        ssh::copy_to_clipboard(public_key);
                        *copied = true;
                    }
                    KeyCode::Enter => {
                        let pk = public_key.clone();
                        let c = *copied;
                        *phase = GithubSetupPhase::Testing {
                            public_key: pk,
                            copied: c,
                        };
                        let tx = tx.clone();
                        let cfg = config::load();
                        std::thread::spawn(move || {
                            if let Some(path) = cfg.github_ssh_key_path {
                                let (ok, msg) = ssh::test_github_ssh_key(&path);
                                tx.send(Msg::GithubTestResult {
                                    success: ok,
                                    message: msg,
                                })
                                .ok();
                            }
                        });
                    }
                    KeyCode::Esc => {
                        main.popup = None;
                    }
                    _ => {}
                },
                GithubSetupPhase::Done(_) => {
                    if key.code == KeyCode::Enter || key.code == KeyCode::Esc {
                        main.popup = None;
                        main.config.github.next_check = 0; // trigger recheck
                    }
                }
            },

            Popup::DoSetup(phase) => match phase {
                DoSetupPhase::Input(input) => match input.handle_key(key) {
                    InputResult::Submit => {
                        let key_value = input.text.clone();
                        if key_value.is_empty() {
                            return;
                        }
                        let mut cfg = config::load();
                        cfg.do_api_key = Some(key_value.clone());
                        config::save(&cfg);
                        *phase = DoSetupPhase::Testing;
                        let tx = tx.clone();
                        std::thread::spawn(move || {
                            let (ok, msg) = ssh::test_do_api_key(&key_value);
                            tx.send(Msg::DoTestResult {
                                success: ok,
                                message: msg,
                            })
                            .ok();
                        });
                    }
                    InputResult::Cancel => {
                        main.popup = None;
                    }
                    InputResult::Continue => {}
                },
                DoSetupPhase::Testing => {
                    if key.code == KeyCode::Esc {
                        main.popup = None;
                    }
                }
                DoSetupPhase::Failed(_) => match key.code {
                    KeyCode::Enter => {
                        *phase = DoSetupPhase::Input(TextInput::new("", true));
                    }
                    KeyCode::Esc => {
                        main.popup = None;
                    }
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
        let Screen::Main(main) = &mut self.screen else {
            return;
        };
        let Some(Popup::CreateDroplet(state)) = &mut main.popup else {
            return;
        };

        match &mut state.step {
            CreateStep::Main { selected } => match key.code {
                KeyCode::Up => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if *selected < 4 {
                        *selected += 1;
                    }
                }
                KeyCode::Enter => match *selected {
                    0 => {
                        state.step = CreateStep::Region {
                            selected: state.region_idx,
                        };
                    }
                    1 => {
                        state.step = CreateStep::Machine {
                            selected: state.machine_idx,
                        };
                    }
                    2 => {
                        let name = state.name.clone();
                        state.step = CreateStep::Name(TextInput::new(&name, false));
                    }
                    3 => {
                        // Create
                        let name = state.name.clone();
                        let region = REGIONS[state.region_idx].slug.to_string();
                        let machine = MACHINES[state.machine_idx].slug.to_string();
                        main.popup = None;
                        main.droplets.creating.push(name.clone());
                        let tx = tx.clone();
                        std::thread::spawn(move || {
                            let cfg = config::load();
                            let api_key = cfg.do_api_key.unwrap_or_default();

                            let ssh_key_id = match ssh::ensure_droplet_ssh_key_on_do(&api_key) {
                                Ok(id) => id,
                                Err(e) => {
                                    tx.send(Msg::DropletCreateFailed {
                                        name,
                                        error: e.to_string(),
                                    })
                                    .ok();
                                    return;
                                }
                            };

                            match digitalocean::create_droplet(
                                &api_key,
                                &name,
                                &region,
                                &machine,
                                &[ssh_key_id],
                            ) {
                                Ok(_) => {
                                    tx.send(Msg::DropletCreateDone { name }).ok();
                                }
                                Err(e) => {
                                    tx.send(Msg::DropletCreateFailed {
                                        name,
                                        error: e.to_string(),
                                    })
                                    .ok();
                                }
                            }
                        });
                    }
                    4 => {
                        // Cancel
                        main.popup = None;
                    }
                    _ => {}
                },
                KeyCode::Esc => {
                    main.popup = None;
                }
                _ => {}
            },

            CreateStep::Region { selected } => match key.code {
                KeyCode::Up => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if *selected + 1 < REGIONS.len() {
                        *selected += 1;
                    }
                }
                KeyCode::Enter => {
                    state.region_idx = *selected;
                    state.step = CreateStep::Main { selected: 0 };
                }
                KeyCode::Esc => {
                    state.step = CreateStep::Main { selected: 0 };
                }
                _ => {}
            },

            CreateStep::Machine { selected } => {
                let available: Vec<usize> = MACHINES
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.available)
                    .map(|(i, _)| i)
                    .collect();

                match key.code {
                    KeyCode::Up => {
                        if *selected > 0 {
                            *selected -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if *selected + 1 < available.len() {
                            *selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if *selected < available.len() {
                            state.machine_idx = available[*selected];
                        }
                        state.step = CreateStep::Main { selected: 1 };
                    }
                    KeyCode::Esc => {
                        state.step = CreateStep::Main { selected: 1 };
                    }
                    _ => {}
                }
            }

            CreateStep::Name(input) => match input.handle_key(key) {
                InputResult::Submit => {
                    if !input.text.is_empty() {
                        state.name = input.text.clone();
                    }
                    state.step = CreateStep::Main { selected: 2 };
                }
                InputResult::Cancel => {
                    state.step = CreateStep::Main { selected: 2 };
                }
                InputResult::Continue => {}
            },
        }
    }

    fn execute_pending_action(&mut self, action: PendingAction, tx: &Sender<Msg>) {
        match action {
            PendingAction::DeleteDroplet { id, name: _ } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.droplets.deleting.insert(id);
                    main.droplets.focus = DFocus::List;
                }
                let tx = tx.clone();
                let cfg = config::load();
                let api_key = cfg.do_api_key.unwrap_or_default();
                std::thread::spawn(move || {
                    match digitalocean::delete_droplet(&api_key, id) {
                        Ok(()) => {
                            tx.send(Msg::DropletDeleted(id)).ok();
                        }
                        Err(e) => {
                            tx.send(Msg::DropletDeleteFailed {
                                id,
                                error: e.to_string(),
                            })
                            .ok();
                        }
                    }
                });
            }
            PendingAction::RegenerateGithubKey => {
                if let Screen::Main(main) = &mut self.screen {
                    main.popup = Some(Popup::GithubSetup(GithubSetupPhase::Generating));
                }
                let tx = tx.clone();
                std::thread::spawn(move || match ssh::generate_github_ssh_key() {
                    Ok((_, pk)) => {
                        tx.send(Msg::GithubKeyGenerated { public_key: pk }).ok();
                    }
                    Err(e) => {
                        tx.send(Msg::GithubKeyGenFailed(e.to_string())).ok();
                    }
                });
            }
        }
    }

    // ── Handle Messages ─────────────────────────────────────────────────────

    pub fn handle_message(&mut self, msg: Msg, _tx: &Sender<Msg>) {
        match msg {
            Msg::GithubKeyMissing => {
                if let Screen::Welcome(state) = &mut self.screen {
                    state.phase = WelcomePhase::GithubMissing;
                }
            }

            Msg::GithubKeyGenerated { public_key } => {
                match &mut self.screen {
                    Screen::Welcome(state) => {
                        state.phase = WelcomePhase::GithubGenerated {
                            public_key,
                            copied: false,
                        };
                    }
                    Screen::Main(main) => {
                        if let Some(Popup::GithubSetup(phase)) = &mut main.popup {
                            *phase = GithubSetupPhase::Ready {
                                public_key,
                                copied: false,
                            };
                        }
                    }
                }
            }

            Msg::GithubKeyGenFailed(error) => {
                match &mut self.screen {
                    Screen::Welcome(state) => {
                        state.phase = WelcomePhase::GithubFailed {
                            error,
                            public_key: None,
                            copied: false,
                        };
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
                            state.phase = WelcomePhase::GithubFailed {
                                error: message,
                                public_key: pk,
                                copied,
                            };
                        }
                    }
                    Screen::Main(main) => {
                        if let Some(Popup::GithubSetup(phase)) = &mut main.popup {
                            if success {
                                *phase = GithubSetupPhase::Done(message);
                            } else {
                                let (pk, c) = match phase {
                                    GithubSetupPhase::Testing { public_key, copied } => {
                                        (public_key.clone(), *copied)
                                    }
                                    _ => (String::new(), false),
                                };
                                *phase = GithubSetupPhase::Failed {
                                    error: message,
                                    public_key: pk,
                                    copied: c,
                                };
                            }
                        }
                    }
                }
            }

            Msg::DoKeyExists => {
                // Just waiting for the test result
            }

            Msg::DoKeyMissing => {
                if let Screen::Welcome(state) = &mut self.screen {
                    state.phase = WelcomePhase::DoMissing;
                }
            }

            Msg::DoTestResult { success, message } => {
                match &mut self.screen {
                    Screen::Welcome(state) => {
                        if success {
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
                if let Screen::Main(main) = &mut self.screen {
                    main.droplets.items = droplets;
                    main.droplets.loading = false;
                    // Clamp selection
                    let total = main.droplets.items.len()
                        + main.droplets.creating.len()
                        + 1;
                    if main.droplets.selected >= total {
                        main.droplets.selected = total.saturating_sub(1);
                    }
                }
            }

            Msg::DropletCreateDone { name } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.droplets.creating.retain(|n| n != &name);
                    main.droplets.refresh_countdown = 0; // trigger immediate refresh
                }
            }

            Msg::DropletCreateFailed { name, error } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.droplets.creating.retain(|n| n != &name);
                    self.notification = Some((format!("Create failed: {error}"), 50));
                }
            }

            Msg::DropletDeleted(id) => {
                if let Screen::Main(main) = &mut self.screen {
                    main.droplets.deleting.remove(&id);
                    main.droplets.refresh_countdown = 0;
                }
            }

            Msg::DropletDeleteFailed { id, error } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.droplets.deleting.remove(&id);
                    self.notification = Some((format!("Delete failed: {error}"), 50));
                }
            }

            Msg::ConfigGithubCheck { success, message } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.config.github.status = if success {
                        KeyStatus::Ok
                    } else {
                        KeyStatus::Error
                    };
                    main.config.github.message = Some(message);
                }
            }

            Msg::ConfigDoCheck { success, message } => {
                if let Screen::Main(main) = &mut self.screen {
                    main.config.digitalocean.status = if success {
                        KeyStatus::Ok
                    } else {
                        KeyStatus::Error
                    };
                    main.config.digitalocean.message = Some(message);
                }
            }
        }
    }

    // ── Background Task Spawning ────────────────────────────────────────────

    fn spawn_refresh_droplets(&self, tx: &Sender<Msg>) {
        let tx = tx.clone();
        let cfg = config::load();
        std::thread::spawn(move || {
            if let Some(api_key) = cfg.do_api_key {
                match digitalocean::list_droplets(&api_key) {
                    Ok(droplets) => {
                        tx.send(Msg::DropletsLoaded(droplets)).ok();
                    }
                    Err(_) => {
                        tx.send(Msg::DropletsLoaded(vec![])).ok();
                    }
                }
            } else {
                tx.send(Msg::DropletsLoaded(vec![])).ok();
            }
        });
    }

    fn spawn_config_github_check(&self, tx: &Sender<Msg>) {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let cfg = config::load();
            let (ok, msg) = match cfg.github_ssh_key_path {
                Some(path) if std::path::Path::new(&path).exists() => {
                    ssh::test_github_ssh_key(&path)
                }
                _ => (false, "No key configured".to_string()),
            };
            tx.send(Msg::ConfigGithubCheck {
                success: ok,
                message: msg,
            })
            .ok();
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
            tx.send(Msg::ConfigDoCheck {
                success: ok,
                message: msg,
            })
            .ok();
        });
    }
}
