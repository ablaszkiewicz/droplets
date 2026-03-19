# droplets

TUI for managing DigitalOcean droplets with automated provisioning.

## DigitalOcean API Token

Generate a personal access token at https://cloud.digitalocean.com/account/api/tokens

### Required scopes

The token needs the following permissions:

- **Droplet** — Create, Read, Delete
- **Image** — Create
- **Snapshot** — Read, Delete
- **SSH Key** — Read, Create
- **Account** — Read

The simplest option is to create a **Full Access** token.
