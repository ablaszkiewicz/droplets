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

pub fn get_public_key(key_path: &str) -> Result<String> {
    let pub_path = format!("{key_path}.pub");
    let content = std::fs::read_to_string(&pub_path)?;
    Ok(content.trim().to_string())
}
