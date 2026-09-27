# Build and publish the cloud container

For automatic builds and registry publishing from GitHub, see [Docker CI/CD setup](github-actions.md).

## Published image

The first cloud image was built and published on 2026-09-27 UTC:

```text
plantjournaldaac2bd8.azurecr.io/plant-journal:20260927T024414Z
```

Immutable image reference:

```text
plantjournaldaac2bd8.azurecr.io/plant-journal@sha256:722cfbf9bda0eb27e44313f8346c42983af59d6810c0df65c2ff58f235a31c50
```

- Registry: `plantjournaldaac2bd8`, Basic tier, `centralus`, resource group `2027_group`.
- Platform: `linux/amd64`.
- ACR build `cj1`: succeeded; 42 tests passed, live Azure SQL test ignored.
- ACR runtime check `cj2`: succeeded; the published image returned `plant-journal 0.1.0`.
- That initial publication only pushed the image. The GitHub workflow now also
  deploys successful `main` builds to the `plant-journal` App Service once its
  identities and runtime settings are configured.

To publish a new version to the same registry:

```sh
bash deploy/publish-acr.sh plantjournaldaac2bd8
```

## Container contents

The multistage `Dockerfile` builds and tests the Rust service on Linux using the
committed lockfile. The final Debian image contains the release binary, public CA
certificates, timezone data, and `deploy/config.cloud.toml`. It runs as UID/GID
10001, listens on port 3000, and handles SIGTERM directly. Templates, JavaScript,
CSS, and migrations are compiled into the binary.

The `.dockerignore` uses a build-input allowlist. Local `.env` files, `config.toml`,
data/photos, backups, Git metadata, and `target/` are excluded. Do not pass database
credentials as build arguments; supply them only when running the container.

## Publish using Azure Container Registry Tasks

No local Docker installation is needed. Sign in, select the intended subscription,
and find the registry:

```sh
az login --scope https://management.core.windows.net//.default
az acr list --query '[].{name:name,resourceGroup:resourceGroup}' --output table
bash deploy/publish-acr.sh YOUR_REGISTRY_NAME
```

The script builds `linux/amd64`, runs the Rust tests during the build, pushes
`plant-journal:<UTC timestamp>`, runs the published image with `--version`, then
prints the image reference and digest. An explicit second argument selects a tag:

```sh
bash deploy/publish-acr.sh YOUR_REGISTRY_NAME release-001
```

Use a fresh tag for each build. The script uses the current Azure subscription and
an existing registry; it does not create infrastructure or deploy the website. Its
first request validates registry access. A successful cached `az account show`
does not prove that Azure accepts the session: `AADSTS9002313` requires signing in
again before retrying. Registry permissions must allow ACR Tasks builds/runs and
publishing images. For ABAC-enabled registries, consult the source-registry
identity requirements in [Microsoft's ACR Tasks guide](https://learn.microsoft.com/en-us/azure/container-registry/container-registry-quickstart-task-cli).

The underlying build command is:

```sh
az acr build --registry YOUR_REGISTRY_NAME \
  --image plant-journal:release-001 --platform linux/amd64 --file Dockerfile .
```

## Runtime settings

The image defaults to `/app/config.cloud.toml` through `PLANT_CONFIG`. Set these
environment variables in App Service (or use Key Vault references):

| Setting | Value |
| --- | --- |
| `AZURE_SQL_SERVER` | Your SQL server hostname, without a URL scheme |
| `AZURE_SQL_DATABASE` | `ripsql` (also the cloud TOML default) |
| `AZURE_SQL_USERNAME` | Your runtime SQL user |
| `AZURE_SQL_PASSWORD` | The user's password, supplied as a runtime secret |
| `WEBSITES_ENABLE_APP_SERVICE_STORAGE` | `true`, for persistent `/home` storage in App Service |

Configure App Service's container target port as **3000**. Configure required
sign-in and owner-only access before exposing the site: application routes still
assume authentication is handled by the hosting platform.

Photos are under `/home/plant-journal/photos`. Provide persistent storage writable
by UID/GID 10001; a host mount replaces the directory permissions from the image.
Copy existing photo files separately when moving the journal. Azure SQL contains
their metadata, not the images. See [App Service custom-container storage](https://learn.microsoft.com/en-us/azure/app-service/configure-custom-container).

`GET /healthz` is a process liveness endpoint; it does not issue SQL queries. Normal
startup already requires successful database schema verification. Run the image
with `--check-database` and runtime credentials for a separate read-only SQL check.

Cloud configuration has `database.migrate = false`. Apply migrations explicitly
with `--migrate` using a migration-capable identity before starting the web service.
The publication script never connects to SQL or runs migrations.

## Hardware behavior

`automation_enabled = false` prevents this instance from spawning sensor, photo,
or equipment workers. Sensor and camera adapters are disabled. Schedule/override
mutations return HTTP 503 explaining that equipment control is unavailable on the
web-only instance, rather than accepting commands that cannot run. Journal and
history APIs remain available. Existing Pi configurations retain their default
`automation_enabled = true` behavior.

This image does not add a Pi agent, photo upload protocol, or remote equipment
control. Run one cloud web instance initially. Concurrent Pi/cloud use requires a
deliberate shared-photo and device integration design; do not treat this image as
that integration.

## Optional local Docker build

If Docker is installed, the equivalent local build and credential-free smoke test
are:

```sh
docker build --platform linux/amd64 -t plant-journal:local .
docker run --rm --platform linux/amd64 plant-journal:local --version
```

For Azure publishing from a Mac, ACR Tasks builds directly on Linux and avoids
local architecture/emulation issues.
