# Dashboard access and password setup

RamShield can hash an operator-supplied dashboard password at startup. Keep the
plaintext password outside the repository and outside `config.baseline.toml`.

## 1. Create a private environment file

For a systemd installation:

```bash
sudo install -o root -g root -m 600 /dev/null /etc/ramshield/dashboard.env
sudoedit /etc/ramshield/dashboard.env
```

Add these lines, replacing the placeholder with a unique password:

```dotenv
RAMSHIELD_DASHBOARD__ENABLED=true
RAMSHIELD_DASHBOARD__ADMIN_PASSWORD=REPLACE_WITH_A_LONG_RANDOM_PASSWORD
```

Generate a suitable password locally, for example:

```bash
openssl rand -base64 32
```

Paste the generated value after `RAMSHIELD_DASHBOARD__ADMIN_PASSWORD=`.
Do not commit the real environment file or share its contents.

## 2. Load it from the systemd service

Create a service drop-in (adjust the unit name if your installation uses a
different one):

```bash
sudo systemctl edit ramshield
```

Add:

```ini
[Service]
EnvironmentFile=/etc/ramshield/dashboard.env
```

Then reload systemd and restart RamShield:

```bash
sudo systemctl daemon-reload
sudo systemctl restart ramshield
sudo systemctl status ramshield --no-pager
```

The environment variables are read during configuration loading, so restarting
the service is required after changing this file.

## 3. Sign in

With the default loopback bind, open:

`http://127.0.0.1:9999/login`

The password is hashed with Argon2 by the configuration loader; operators do
not need to generate a hash manually. The plaintext password is not written to
the TOML config by this mechanism.

For a local non-systemd run, export the same variables in the shell before
starting RamShield.

## Network exposure

- Keep `dashboard.http_addr` on `127.0.0.1:9999` unless remote access is
  required.
- RamShield does **not** provide a built-in TLS listener. For remote access,
  terminate HTTPS at a trusted reverse proxy and configure the dashboard's
  TLS/cookie and trusted-proxy settings to match that deployment.
- A non-loopback dashboard bind requires an admin password hash and the
  external TLS boundary to be configured. Do not expose the dashboard over
  plain HTTP.
- `RAMSHIELD_DASHBOARD__ADMIN_PASSWORD_HASH` is also supported when an
  operator already has an Argon2 PHC hash. If both password variables are
  supplied, the hash override takes precedence; prefer setting only one.
- Environment files protect secrets from accidental repository disclosure, but
  still restrict host access: root and sufficiently privileged processes can
  read the file.

See [Configuration](CONFIGURATION.md) for the full dashboard settings.
