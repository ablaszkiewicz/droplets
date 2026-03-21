use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Result};

use super::config;

pub fn generate_github_ssh_key() -> Result<(String, String)> {
    let dir = config::config_dir();
    let key_path = dir.join("github_key");
    let pub_path = dir.join("github_key.pub");

    // Remove existing
    let _ = std::fs::remove_file(&key_path);
    let _ = std::fs::remove_file(&pub_path);

    let output = Command::new("ssh-keygen")
        .args([
            "-t",
            "ed25519",
            "-f",
            key_path.to_str().unwrap(),
            "-N",
            "",
            "-C",
            "droplets-github",
        ])
        .output()?;

    if !output.status.success() {
        bail!(
            "ssh-keygen failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let pub_key = std::fs::read_to_string(&pub_path)?.trim().to_string();
    let path_str = key_path.to_str().unwrap().to_string();

    let mut cfg = config::load();
    cfg.github_ssh_key_path = Some(path_str.clone());
    config::save(&cfg);

    Ok((path_str, pub_key))
}

pub fn generate_droplet_ssh_key() -> Result<(String, String)> {
    let dir = config::config_dir();
    let key_path = dir.join("droplet_key");
    let pub_path = dir.join("droplet_key.pub");

    if key_path.exists() {
        let pub_key = std::fs::read_to_string(&pub_path)?.trim().to_string();
        let path_str = key_path.to_str().unwrap().to_string();
        let mut cfg = config::load();
        cfg.droplet_ssh_key_path = Some(path_str.clone());
        config::save(&cfg);
        return Ok((path_str, pub_key));
    }

    let output = Command::new("ssh-keygen")
        .args([
            "-t",
            "ed25519",
            "-f",
            key_path.to_str().unwrap(),
            "-N",
            "",
            "-C",
            "droplets-tool",
        ])
        .output()?;

    if !output.status.success() {
        bail!(
            "ssh-keygen failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let pub_key = std::fs::read_to_string(&pub_path)?.trim().to_string();
    let path_str = key_path.to_str().unwrap().to_string();

    let mut cfg = config::load();
    cfg.droplet_ssh_key_path = Some(path_str.clone());
    config::save(&cfg);

    Ok((path_str, pub_key))
}

pub fn ensure_droplet_ssh_key_on_do(api_key: &str) -> Result<i64> {
    use super::digitalocean;

    let (_, pub_key) = generate_droplet_ssh_key()?;

    let cfg = config::load();
    if let Some(id) = cfg.do_ssh_key_id {
        return Ok(id);
    }

    let do_key = digitalocean::upload_ssh_key(api_key, "droplets-tool", &pub_key)?;

    let mut cfg = config::load();
    cfg.do_ssh_key_id = Some(do_key.id);
    config::save(&cfg);

    Ok(do_key.id)
}

pub fn test_github_ssh_key(key_path: &str) -> (bool, String) {
    let result = Command::new("ssh")
        .args([
            "-i",
            key_path,
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "IdentitiesOnly=yes",
            "-T",
            "git@github.com",
        ])
        .output();

    match result {
        Ok(output) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            );
            if let (Some(start), Some(end)) = (text.find("Hi "), text.find("! You've")) {
                let username = &text[start + 3..end];
                (true, format!("Authenticated as {username}"))
            } else {
                (false, format!("Authentication failed: {}", text.trim()))
            }
        }
        Err(e) => (false, e.to_string()),
    }
}

pub fn test_do_api_key(api_key: &str) -> (bool, String) {
    match super::digitalocean::test_account(api_key) {
        Ok(email) => (true, format!("Authenticated as {email}")),
        Err(e) => (false, e.to_string()),
    }
}

// ── Clipboard / Terminal ────────────────────────────────────────────────────

pub fn copy_to_clipboard(text: &str) -> bool {
    let child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .spawn();

    match child {
        Ok(mut child) => {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(text.as_bytes());
            }
            child.wait().map(|s| s.success()).unwrap_or(false)
        }
        Err(_) => false,
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Strip ANSI escape sequences and control characters from a line.
fn sanitize_line(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut in_escape = false;
    for c in line.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
            continue;
        }
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        if c == '\r' || (c.is_control() && c != '\t') {
            continue;
        }
        result.push(c);
    }
    result
}

// ── Provisioning commands ───────────────────────────────────────────────────

/// Run a command on a remote droplet via SSH (no logging).
fn ssh_run(droplet_key: &str, ip: &str, cmd: &str) -> Result<String> {
    let output = Command::new("ssh")
        .stdin(Stdio::null())
        .arg("-i")
        .arg(droplet_key)
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("ConnectTimeout=15")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg(format!("root@{ip}"))
        .arg(cmd)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!("{}{}", stderr, stdout);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run a command on a remote droplet via SSH, streaming stdout+stderr line by line.
fn ssh_run_logged(
    droplet_key: &str,
    ip: &str,
    cmd: &str,
    on_line: &dyn Fn(&str),
) -> Result<String> {
    use std::io::BufRead;

    on_line(&format!("$ ssh root@{ip} '{cmd}'"));

    let mut child = Command::new("ssh")
        .stdin(Stdio::null())
        .arg("-i")
        .arg(droplet_key)
        .arg("-o")
        .arg("StrictHostKeyChecking=accept-new")
        .arg("-o")
        .arg("ConnectTimeout=15")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ServerAliveInterval=30")
        .arg("-o")
        .arg("ServerAliveCountMax=10")
        .arg(format!("root@{ip}"))
        // Wrap in subshell so 2>&1 captures stderr from the entire command,
        // including compound commands like `cmd1 || true`
        .arg(format!("( {cmd} ) 2>&1"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Read local stderr in a background thread (SSH connection errors go here)
    let stderr_handle = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr);
        let mut lines = Vec::new();
        for line in reader.lines().flatten() {
            lines.push(line);
        }
        lines
    });

    let mut reader = std::io::BufReader::new(stdout);
    let mut all_output = String::new();

    // Read byte-by-byte into segments split on \n or \r, so we capture
    // progress output from git/apt/curl that uses \r to update in-place.
    let mut buf = Vec::with_capacity(1024);
    loop {
        use std::io::Read;
        let mut byte = [0u8; 1];
        match reader.read(&mut byte) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let b = byte[0];
                if b == b'\n' || b == b'\r' {
                    if !buf.is_empty() {
                        let raw = String::from_utf8_lossy(&buf).to_string();
                        let clean = sanitize_line(&raw);
                        if !clean.trim().is_empty() {
                            on_line(&clean);
                        }
                        all_output.push_str(&raw);
                        all_output.push('\n');
                        buf.clear();
                    }
                } else {
                    buf.push(b);
                }
            }
            Err(_) => break,
        }
    }
    // Flush remaining
    if !buf.is_empty() {
        let raw = String::from_utf8_lossy(&buf).to_string();
        let clean = sanitize_line(&raw);
        if !clean.trim().is_empty() {
            on_line(&clean);
        }
        all_output.push_str(&raw);
        all_output.push('\n');
    }

    // Collect local stderr lines (SSH connection errors) and add to output
    let stderr_lines = stderr_handle.join().unwrap_or_default();
    for line in &stderr_lines {
        let clean = sanitize_line(line);
        if !clean.trim().is_empty() {
            on_line(&clean);
        }
        all_output.push_str(line);
        all_output.push('\n');
    }

    let status = child.wait()?;
    if !status.success() {
        bail!(
            "exit {}: {}",
            status.code().unwrap_or(-1),
            all_output.trim()
        );
    }

    Ok(all_output)
}

// ── Provision marker system ─────────────────────────────────────────────────

fn provision_marker_filename(step_idx: usize) -> String {
    format!("step-{step_idx}.done")
}

fn write_provision_marker(droplet_key: &str, ip: &str, step_idx: usize) -> Result<()> {
    let fname = provision_marker_filename(step_idx);
    ssh_run(
        droplet_key,
        ip,
        &format!("mkdir -p /root/.droplets && touch /root/.droplets/{fname}"),
    )?;
    Ok(())
}

pub fn check_provision_markers(
    droplet_key: &str,
    ip: &str,
    total_steps: usize,
) -> Result<Vec<bool>> {
    let output = ssh_run(droplet_key, ip, "ls /root/.droplets/ 2>/dev/null || echo ''")?;
    let mut results = vec![false; total_steps];
    for i in 0..total_steps {
        let fname = provision_marker_filename(i);
        if output.contains(&fname) {
            results[i] = true;
        }
    }
    // Pull (step 10) used to leave no marker; treat as done if Flox (11) already completed.
    if total_steps > 11 && !results[10] && results[11] {
        results[10] = true;
    }
    // Flox (11): treat as done if tmux hogli (12) already completed.
    if total_steps > 12 && !results[11] && results[12] {
        results[11] = true;
    }
    Ok(results)
}

// ── Provisioning steps ──────────────────────────────────────────────────────

/// Step 0: Copy the GitHub SSH key to the droplet.
pub fn provision_transport_github_key(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    let cfg = config::load();
    let github_key_path = cfg
        .github_ssh_key_path
        .ok_or_else(|| anyhow::anyhow!("No GitHub SSH key configured"))?;

    let private_key = std::fs::read_to_string(&github_key_path)?;
    let pub_path = format!("{github_key_path}.pub");
    let public_key = std::fs::read_to_string(&pub_path)?;

    let escaped_priv = private_key.replace('\'', "'\\''");
    let escaped_pub = public_key.trim().replace('\'', "'\\''");

    let script = format!(
        "mkdir -p ~/.ssh && \
         echo '{escaped_priv}' > ~/.ssh/github_key && \
         chmod 600 ~/.ssh/github_key && \
         echo '{escaped_pub}' > ~/.ssh/github_key.pub && \
         chmod 644 ~/.ssh/github_key.pub && \
         echo 'Host github.com\n  IdentityFile ~/.ssh/github_key\n  StrictHostKeyChecking accept-new' > ~/.ssh/config && \
         chmod 600 ~/.ssh/config"
    );

    ssh_run_logged(droplet_key, ip, &script, on_log)?;
    write_provision_marker(droplet_key, ip, 0)?;
    Ok(())
}

/// Step 1: Verify the GitHub SSH key works from the droplet.
pub fn provision_verify_github_key(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    // ssh -T git@github.com exits with code 1 even on success, so use || true
    let output = ssh_run_logged(
        droplet_key,
        ip,
        "ssh -o StrictHostKeyChecking=accept-new -T git@github.com || true",
        on_log,
    )?;
    if !output.contains("successfully authenticated") && !output.contains("You've successfully") {
        bail!("GitHub SSH verification failed: {}", output.trim());
    }
    write_provision_marker(droplet_key, ip, 1)?;
    Ok(())
}

/// Step 2: Install Docker.
pub fn provision_install_docker(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    ssh_run_logged(droplet_key, ip, "curl -fsSL https://get.docker.com | sh", on_log)?;
    write_provision_marker(droplet_key, ip, 2)?;
    Ok(())
}

/// Step 3: Verify Docker.
pub fn provision_verify_docker(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    let output = ssh_run_logged(droplet_key, ip, "docker --version && docker ps", on_log)?;
    if !output.contains("Docker version") {
        bail!("Docker version check failed");
    }
    write_provision_marker(droplet_key, ip, 3)?;
    Ok(())
}

/// Step 4: Install Flox.
pub fn provision_install_flox(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    ssh_run_logged(
        droplet_key,
        ip,
        "wget -q https://downloads.flox.dev/by-env/stable/deb/flox-1.10.0.x86_64-linux.deb && \
         sudo apt install -y ./flox-1.10.0.x86_64-linux.deb",
        on_log,
    )?;
    write_provision_marker(droplet_key, ip, 4)?;
    Ok(())
}

/// Step 5: Verify Flox.
pub fn provision_verify_flox(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    let output = ssh_run_logged(droplet_key, ip, "flox --version", on_log)?;
    if output.trim().is_empty() {
        bail!("Flox not found");
    }
    write_provision_marker(droplet_key, ip, 5)?;
    Ok(())
}

/// Step 6: Install build-essential.
pub fn provision_install_build_essential(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    ssh_run_logged(
        droplet_key,
        ip,
        "sudo apt install -y build-essential",
        on_log,
    )?;
    write_provision_marker(droplet_key, ip, 6)?;
    Ok(())
}

/// Step 7: Verify build-essential.
pub fn provision_verify_build_essential(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    let output = ssh_run_logged(droplet_key, ip, "gcc --version", on_log)?;
    if !output.contains("gcc") {
        bail!("build-essential not installed properly");
    }
    write_provision_marker(droplet_key, ip, 7)?;
    Ok(())
}

/// Step 8: Clone PostHog repo.
pub fn provision_clone_posthog(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    ssh_run_logged(
        droplet_key,
        ip,
        "cd /root && git clone --filter=blob:none https://github.com/PostHog/posthog",
        on_log,
    )?;
    write_provision_marker(droplet_key, ip, 8)?;
    Ok(())
}

/// Step 9: Verify PostHog clone.
pub fn provision_verify_posthog_clone(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    let output = ssh_run_logged(
        droplet_key,
        ip,
        "test -d /root/posthog/.git && echo 'posthog-clone-ok'",
        on_log,
    )?;
    if !output.contains("posthog-clone-ok") {
        bail!("PostHog repo not found at /root/posthog");
    }
    write_provision_marker(droplet_key, ip, 9)?;
    Ok(())
}

/// Step 10: Pull latest main (only if current branch is master/main).
/// Skips if the user has switched to a different branch (work in progress).
pub fn provision_pull_latest_main(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    ssh_run_logged(
        droplet_key,
        ip,
        "cd /root/posthog && BRANCH=$(git rev-parse --abbrev-ref HEAD) && \
         if [ \"$BRANCH\" = \"master\" ] || [ \"$BRANCH\" = \"main\" ]; then \
           echo \"On branch $BRANCH, pulling latest...\" && \
           git fetch origin master && git reset --hard origin/master; \
         else \
           echo \"On branch $BRANCH, skipping pull (not on master)\"; \
         fi",
        on_log,
    )?;
    write_provision_marker(droplet_key, ip, 10)?;
    Ok(())
}

/// Step 11: Activate Flox environment in PostHog dir.
pub fn provision_flox_activate(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
) -> Result<()> {
    ssh_run_logged(
        droplet_key,
        ip,
        "cd /root/posthog && flox activate -- echo 'Flox environment ready'",
        on_log,
    )?;
    write_provision_marker(droplet_key, ip, 11)?;
    Ok(())
}

/// Step 12: Ensure detached tmux session `hogli` running hogli under Flox (idempotent).
///
/// `flox activate -- hogli …` does not work: `hogli` is only on PATH after interactive
/// activation finishes. We run `flox activate` in the pane, poll output until Flox reports
/// readiness (`You are now using the environment`), then send `hogli start --phrocs`.
///
/// When `force_recreate` is true (manual “Re-run pull + Flox + tmux”), an existing `hogli`
/// session is killed first so the pane is rebuilt from scratch.
pub fn provision_ensure_tmux_hogli(
    droplet_key: &str,
    ip: &str,
    on_log: &dyn Fn(&str),
    force_recreate: bool,
) -> Result<()> {
    let kill_existing = if force_recreate {
        r#"if tmux has-session -t hogli 2>/dev/null; then
  echo 'removing existing tmux session hogli'
  tmux kill-session -t hogli
fi

"#
    } else {
        ""
    };

    let script = format!(
        r##"if ! command -v tmux >/dev/null 2>&1; then
  export DEBIAN_FRONTEND=noninteractive && apt-get update -qq && apt-get install -y -qq tmux
fi
{}if tmux has-session -t hogli 2>/dev/null; then
  echo 'tmux session hogli already exists'
else
  tmux new-session -d -s hogli -c /root/posthog
  tmux send-keys -t hogli 'flox activate' Enter
  i=0
  while [ "$i" -lt 600 ]; do
    if tmux capture-pane -t hogli -p -S - 2>/dev/null | tail -n 120 | grep -q 'You are now using the environment'; then
      break
    fi
    sleep 1
    i=$((i + 1))
  done
  if [ "$i" -ge 600 ]; then
    echo 'timed out waiting for flox activate (expected: You are now using the environment)' >&2
    exit 1
  fi
  tmux send-keys -t hogli 'hogli start --phrocs; exec bash' Enter
  echo 'started tmux session hogli'
fi
"##,
        kill_existing
    );

    ssh_run_logged(droplet_key, ip, &script, on_log)?;
    write_provision_marker(droplet_key, ip, 12)?;
    Ok(())
}
