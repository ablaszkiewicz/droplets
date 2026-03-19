use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::types::{DropletInfo, SnapshotInfo};

const BASE: &str = "https://api.digitalocean.com/v2";

fn client(api_key: &str) -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {api_key}").parse().unwrap(),
            );
            h.insert(
                reqwest::header::CONTENT_TYPE,
                "application/json".parse().unwrap(),
            );
            h
        })
        .build()
        .unwrap()
}

fn check(resp: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status().as_u16();
    let body = resp.text().unwrap_or_default();
    let msg: String = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
        .unwrap_or(body);
    bail!("{status}: {msg}");
}

// ── List ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DropletsResp {
    droplets: Vec<ApiDroplet>,
}

#[derive(Deserialize)]
struct DropletResp {
    droplet: ApiDroplet,
}

#[derive(Deserialize)]
struct ApiDroplet {
    id: i64,
    name: String,
    status: String,
    region: ApiRegion,
    size_slug: String,
    created_at: String,
    #[serde(default)]
    networks: ApiNetworks,
}

#[derive(Deserialize, Default)]
struct ApiNetworks {
    #[serde(default)]
    v4: Vec<ApiNetwork>,
}

#[derive(Deserialize)]
struct ApiNetwork {
    ip_address: String,
    #[serde(rename = "type")]
    net_type: String,
}

#[derive(Deserialize)]
struct ApiRegion {
    slug: String,
}

impl ApiDroplet {
    fn into_info(self) -> DropletInfo {
        let ip = self
            .networks
            .v4
            .iter()
            .find(|n| n.net_type == "public")
            .map(|n| n.ip_address.clone());
        DropletInfo {
            id: self.id,
            name: self.name,
            status: self.status,
            ip,
            region: self.region.slug,
            size: self.size_slug,
            created_at: self.created_at,
        }
    }
}

pub fn list_droplets(api_key: &str) -> Result<Vec<DropletInfo>> {
    let resp = client(api_key).get(format!("{BASE}/droplets")).send()?;
    let resp = check(resp)?;
    let body: DropletsResp = resp.json().context("parse droplets")?;
    Ok(body.droplets.into_iter().map(|d| d.into_info()).collect())
}

pub fn get_droplet(api_key: &str, id: i64) -> Result<DropletInfo> {
    let resp = client(api_key)
        .get(format!("{BASE}/droplets/{id}"))
        .send()?;
    let resp = check(resp)?;
    let body: DropletResp = resp.json().context("parse droplet")?;
    Ok(body.droplet.into_info())
}

pub fn create_droplet(
    api_key: &str,
    name: &str,
    region: &str,
    size: &str,
    ssh_key_ids: &[i64],
    image: &str,
) -> Result<DropletInfo> {
    // If image is a numeric snapshot ID, pass as number; otherwise as string slug
    let image_value: serde_json::Value = if let Ok(id) = image.parse::<i64>() {
        serde_json::Value::Number(id.into())
    } else {
        serde_json::Value::String(image.to_string())
    };

    let resp = client(api_key)
        .post(format!("{BASE}/droplets"))
        .json(&serde_json::json!({
            "name": name,
            "region": region,
            "size": size,
            "image": image_value,
            "ssh_keys": ssh_key_ids,
        }))
        .send()?;
    let resp = check(resp)?;
    let body: DropletResp = resp.json().context("parse created droplet")?;
    Ok(body.droplet.into_info())
}

pub fn rename_droplet(api_key: &str, id: i64, new_name: &str) -> Result<()> {
    let resp = client(api_key)
        .post(format!("{BASE}/droplets/{id}/actions"))
        .json(&serde_json::json!({
            "type": "rename",
            "name": new_name,
        }))
        .send()?;
    check(resp)?;
    Ok(())
}

pub fn delete_droplet(api_key: &str, id: i64) -> Result<()> {
    let resp = client(api_key)
        .delete(format!("{BASE}/droplets/{id}"))
        .send()?;
    check(resp)?;
    Ok(())
}

// ── SSH Keys ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SshKeysResp {
    ssh_keys: Vec<ApiSshKey>,
}

#[derive(Deserialize)]
struct SshKeyResp {
    ssh_key: ApiSshKey,
}

#[derive(Deserialize, Clone)]
pub struct ApiSshKey {
    pub id: i64,
    pub name: String,
    pub public_key: String,
}

pub fn list_ssh_keys(api_key: &str) -> Result<Vec<ApiSshKey>> {
    let resp = client(api_key)
        .get(format!("{BASE}/account/keys"))
        .send()?;
    let resp = check(resp)?;
    let body: SshKeysResp = resp.json().context("parse ssh keys")?;
    Ok(body.ssh_keys)
}

pub fn upload_ssh_key(api_key: &str, name: &str, public_key: &str) -> Result<ApiSshKey> {
    // Check if key with this name already exists
    let existing = list_ssh_keys(api_key)?;
    if let Some(key) = existing.iter().find(|k| k.name == name) {
        return Ok(key.clone());
    }

    let resp = client(api_key)
        .post(format!("{BASE}/account/keys"))
        .json(&serde_json::json!({
            "name": name,
            "public_key": public_key,
        }))
        .send()?;

    if resp.status().as_u16() == 422 {
        // Key with same fingerprint exists, find it
        let existing = list_ssh_keys(api_key)?;
        if let Some(key) = existing
            .iter()
            .find(|k| k.public_key.trim() == public_key.trim())
        {
            return Ok(key.clone());
        }
    }

    let resp = check(resp)?;
    let body: SshKeyResp = resp.json().context("parse ssh key")?;
    Ok(body.ssh_key)
}

// ── Account test ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AccountResp {
    account: AccountInfo,
}

#[derive(Deserialize)]
struct AccountInfo {
    email: String,
}

pub fn test_account(api_key: &str) -> Result<String> {
    let resp = client(api_key)
        .get(format!("{BASE}/account"))
        .send()?;
    let resp = check(resp)?;
    let body: AccountResp = resp.json().context("parse account")?;
    Ok(body.account.email)
}

// ── Snapshots ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SnapshotsResp {
    snapshots: Vec<ApiSnapshot>,
}

#[derive(Deserialize)]
struct ApiSnapshot {
    id: serde_json::Value, // DO returns this as int or string depending on endpoint
    name: String,
    created_at: String,
    size_gigabytes: f64,
    #[serde(default)]
    regions: Vec<String>,
}

impl ApiSnapshot {
    fn id_string(&self) -> String {
        match &self.id {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    }
}

pub fn list_snapshots(api_key: &str) -> Result<Vec<SnapshotInfo>> {
    let resp = client(api_key)
        .get(format!("{BASE}/snapshots?resource_type=droplet&per_page=200"))
        .send()?;
    let resp = check(resp)?;
    let body: SnapshotsResp = resp.json().context("parse snapshots")?;
    Ok(body
        .snapshots
        .into_iter()
        .map(|s| {
            let id_str = s.id_string();
            SnapshotInfo {
                id: id_str,
                name: s.name,
                created_at: s.created_at,
                size_gigabytes: s.size_gigabytes,
                regions: s.regions,
            }
        })
        .collect())
}

pub fn create_droplet_snapshot(api_key: &str, droplet_id: i64, name: &str) -> Result<()> {
    let resp = client(api_key)
        .post(format!("{BASE}/droplets/{droplet_id}/actions"))
        .json(&serde_json::json!({
            "type": "snapshot",
            "name": name,
        }))
        .send()?;
    check(resp)?;
    Ok(())
}

pub fn delete_snapshot(api_key: &str, snapshot_id: &str) -> Result<()> {
    let resp = client(api_key)
        .delete(format!("{BASE}/snapshots/{snapshot_id}"))
        .send()?;
    check(resp)?;
    Ok(())
}

pub fn rename_snapshot(api_key: &str, snapshot_id: &str, new_name: &str) -> Result<()> {
    let resp = client(api_key)
        .put(format!("{BASE}/images/{snapshot_id}"))
        .json(&serde_json::json!({
            "name": new_name,
        }))
        .send()?;
    check(resp)?;
    Ok(())
}

