# Fieldnotes · Plant Journal

A Rust application for a Raspberry Pi 4 grow space: individual plant journals, a combined calendar, daily photographs, environmental history, and local light/fan controls. The Pi hosts the interface and hardware controls; journal data can live in hosted Azure SQL, with SQLite available for offline development. The sPlant watering kit keeps its own timer; journal watering entries describe care you confirm yourself.

## Try it on your computer

Requires Rust **1.86 or newer** and a C compiler. This quick start uses offline SQLite development mode; no cloud account, Node.js, separate database server, or physical hardware is needed.

```sh
cargo run --locked
```

Open **http://127.0.0.1:3000** to sign in or create an account. Each account starts with a private garden; owners can add registered collaborators in Settings. No example plants or journal records are inserted automatically. Local simulated hardware belongs to the original garden; new gardens start with equipment records only. See [account setup and migration](docs/accounts.md) to activate the original owner account.

In this offline mode, the SQLite database and photos are created in `data/`. Assets and templates are compiled into the binary. Do not run multiple service instances against the same data directory.

**For hosted Azure SQL:** follow [Azure SQL setup and migration](docs/azure-sql.md) using `config.azure.example.toml`. The Pi deployment example selects Azure SQL; photos remain local. Existing SQLite journals can be copied with `--import-sqlite` without changing the source.

**For cloud web hosting:** [build and publish the Linux container to Azure Container Registry](docs/azure-container.md). The included cloud configuration disables hardware automation; it does not replace the Pi controller.

To configure offline development:

```sh
cp config.example.toml config.toml
PLANT_CONFIG=config.toml cargo run --locked
```

Application pages and APIs require a session. Use HTTPS with `secure_cookies = true` (the default) for hosted access. The example development configuration uses `secure_cookies = false` for local HTTP. Mutating API requests require JSON (or binary photo uploads) and reject cross-origin browser requests. [Account setup](docs/accounts.md) covers owner activation, recovery, collaboration, and MVP limits.

## What is included

- Public email/password signup, login/logout, garden selection, and owner-managed collaborators.
- Garden isolation for journal records, photos, equipment, schedules, and settings.

- Plant profiles, notes, species/variety, and archival that preserves history.
- **Seed vault** with packet / lot labels, breeders, acquisition dates, suppliers, storage locations, photos, and quantities in seeds or packets. Open a packet to record germination attempts and see every plant grown from it, including archived plants. A blank germination result is pending; zero records a failed attempt. Germination rates use completed attempts only. Stock quantities remain manually controlled.
- Dated notes, watering, feeding, pruning, and repotting entries linked to one or several plants; edit and delete support.
- Month calendar with selected-day details and plant/event filters. Shared environment and equipment events appear under **All plants**; a plant filter shows explicitly linked events.
- **Add photo** on any plant or seed record uploads a JPEG, PNG, GIF, or WebP file up to 10 MB without a camera. Plant uploads appear in their photo history; seed uploads appear on their inventory card. All uploads also appear in the photo journal and calendar. Deleting a seed retains its photos in the photo journal.
- Daily grow-space photos and **Capture now**, with editable plant associations and explicit deletion. Daily/manual captures initially link to all active plants. A single image is shared by all its plant links.
- Temperature and humidity charts (24 hours, 7 days, or 30 days), daily calendar averages, adapter labels, and stale-reading indicators.
- Local Shelly RPC and simulated outlets, daily on/off windows, overnight schedules, timed overrides, and **Resume schedule**.
- UTC storage, configurable IANA timezone, persistent execution records, component health, and structured service logs.

Open **Collection** for variety and strain cards and interactive ancestry graphs.
At signup or when creating a garden, optionally choose **Vegetables (20)**,
**Herbs (16)**, **Flowers (12)**, **Fruit & berries (12)**, **Houseplants (8)**,
and **Cannabis (152)**. Mix collections or start empty. Use **Add starter collections**
in Collection or Settings to add more later. Imports preserve existing cards and
edits, and deleted cards are not recreated. Filter mixed collections by plant type.

In **Seed vault**, add one record per packet or lot you want to track separately.
Open its card to record attempts (date, seeds sown, final germinated count, notes).
**Add plant from attempt** carries the packet, attempt, and linked strain into the
new plant form. Existing plants can be linked through **Edit plant → Seed origin**.
Plant pages link back to their source packet. Packets and attempts with linked
history cannot be deleted; set stock to zero when a packet is used up.

Link plants and seeds to existing strains, or type a new strain name in their forms.
Cards can be unowned, wanted, or collected; linked plants and seeds on hand unlock
cards automatically. The original owner's garden starts with 152 curated cannabis
strains and selected documented parentage. See [strain collection and lineage](docs/strains.md)
for collection rules, sources, and database upgrade details.

Use **Collection → Cross planner** to save ideas with two parent strains and notes.
When you have the cross, choose **Create strain** to turn the plan into a collected
card with its recorded ancestry. The original plan stays linked to that card.

Choose **Settings → Appearance → Theme** for seven looks: the default **Fieldnotes**,
**Night Arcade**, **Herbarium**, **Seed Catalog ’79**, **Solarpunk Greenhouse**,
**Alchemy Lab**, and **Neon Genetics**. Each includes matching strain cards and
ancestry graphs. Changes apply instantly and are remembered in this browser across
gardens and visits, including the sign-in page.

All automation starts disabled. Settings initially use `America/Chicago` and a noon photo time. Choose a photo time during your lights-on window. Camera capture never switches the grow light on automatically.

## Raspberry Pi installation

Use Raspberry Pi OS Lite **64-bit**, a suitable Pi 4 power supply, and reliable storage. Keep the computer and electrical switching equipment away from water. The application does not require a separate microcontroller.

First configure your database and credentials using [the Azure SQL guide](docs/azure-sql.md). The supplied deployment TOML selects Azure SQL and the service reads `/etc/plant-journal/azure.env`. To keep an offline SQLite deployment, explicitly change `[database] backend` to `"sqlite"`.

On the Pi, install Rust 1.86+ using your preferred supported toolchain, then build the repository **on the Pi** (a macOS binary will not run on Linux):

```sh
sudo apt update
sudo apt install -y build-essential pkg-config ffmpeg v4l-utils sqlite3
cargo build --release --locked
sudo useradd --system --user-group --home-dir /var/lib/plant-journal --shell /usr/sbin/nologin plant-journal
sudo install -m 755 target/release/plant-journal /usr/local/bin/plant-journal
sudo install -d -m 750 /etc/plant-journal
sudo install -m 640 -o root -g plant-journal deploy/config.toml /etc/plant-journal/config.toml
sudo install -m 644 deploy/plant-journal.service /etc/systemd/system/plant-journal.service
sudo systemctl daemon-reload
# For Azure mode, create /etc/plant-journal/azure.env before starting the service:
# sudo install -m 600 -o root -g root deploy/azure.env.example /etc/plant-journal/azure.env
# sudoedit /etc/plant-journal/azure.env
sudo systemctl enable --now plant-journal
```

If the service user already exists, skip `useradd`. Open `http://<pi-address>:3000`. The service initially uses simulated hardware, so you can confirm the UI before changing adapters.

```sh
sudo systemctl status plant-journal
sudo journalctl -u plant-journal -f
```

Edit `/etc/plant-journal/config.toml` and restart the service to change hardware adapters. UI settings (timezone, photos, equipment, schedules) persist in the configured database. Dependency versions are committed in `Cargo.lock`; use `--locked` for reproducible builds. The lockfile pins a compatible `yoke-derive` patch for Rust 1.86.

### DHT11 / DHT22 starter-kit sensor

Check the sensor's printed model and its module pinout before wiring. A starter-kit description alone does not identify the part. Use a module compatible with the Pi's **3.3V GPIO logic**; do not feed a 5V data signal into a GPIO. Bare sensors may need a pull-up to 3.3V. Follow the identified module's datasheet.

For a DHT11/DHT22 DATA line on **BCM GPIO4**, add this to `/boot/firmware/config.txt` (choose a different unused GPIO if necessary), then reboot:

```ini
dtoverlay=dht11,gpiopin=4
```

Discover the actual IIO device; do not assume its index will be zero:

```sh
cat /sys/bus/iio/devices/iio:device*/name
cat /sys/bus/iio/devices/iio:device0/in_temp_input
cat /sys/bus/iio/devices/iio:device0/in_humidityrelative_input
```

Set `[sensor] adapter = "iio"` and `path` to the verified device directory. The adapter reads temperature in millidegrees Celsius and humidity in thousandths of a percent, converts both, and rejects malformed/nonfinite/out-of-range values. Missing or failed readings retain the last successful sample; samples become stale after five minutes. DHT timing and module reliability must be verified on the actual Pi; the adapter does not silently substitute simulated data when real hardware fails.

Read-only monitoring is intentional. The sensor never controls the fan in this version. Reference: [Raspberry Pi overlay documentation](https://github.com/raspberrypi/firmware/blob/master/boot/overlays/README).

### Razer Kiyo / USB webcam

Test the Kiyo as a Linux V4L2 device; no claim is made that every Kiyo revision or firmware supports the same formats. Raspberry Pi Camera Module CSI capture and old phone cameras are outside this version.

```sh
v4l2-ctl --list-devices
ls -l /dev/v4l/by-id/
v4l2-ctl --device /dev/video0 --list-formats-ext
ffmpeg -f v4l2 -input_format mjpeg -video_size 1280x720 -i /dev/video0 -frames:v 1 test.jpg
```

Use a format and size listed by your camera. Select its stable `/dev/v4l/by-id/...-video-index0` path in the config, set `[camera] adapter = "v4l2"`, and restart. The systemd service has `video` group access. Adjust any physical ring light before leaving the camera in the grow space.

Capture writes a temporary image, syncs it, and renames it before committing a photo record. A failed capture removes partial files and reports failure. If a finalized image’s database commit cannot be confirmed, the image is retained for reconciliation; the server may have committed despite a lost acknowledgement. There is no automatic photo deletion. Monitor available storage with `df -h /var/lib/plant-journal`; full storage is reported as an error. Backup and cleanup should be part of normal operation.

Reference: [FFmpeg V4L2 documentation](https://ffmpeg.org/ffmpeg-devices.html#video4linux2_002c-v4l2).

### Lights and fan through local outlets

Use enclosed outlets whose specifications cover the **actual LED load/inrush and fan motor load**. A general wattage rating alone does not establish motor or LED-driver suitability. Verify your inline fan's model/rating before connecting it. This app provides on/off switching, not speed control or dimming.

The Shelly adapter supports devices with local Gen2+ RPC methods `Switch.GetStatus` and `Switch.Set`. Configure the outlet's Wi-Fi through its own setup flow, reserve its local IP in your router, and enter `http://<outlet-address>` and switch channel (usually `0`) under Equipment. The outlet itself does not require cloud access; Azure database mode still needs internet connectivity. This adapter currently requires local RPC without authentication; it does **not** implement Shelly Digest authentication. Authenticated outlets fail visibly instead of appearing connected.

Add and test a **simulated** outlet first. For a physical outlet, verify status and short manual on/off overrides with the intended load before enabling schedules. Set and verify the outlet's own power-on behavior in its configuration. The Pi cannot guarantee switching during a network outage or power failure; a device may retain its last state. Schedule operation depends on a working Pi, local network, and correct clock. In SQLite mode, test with the WAN disconnected while keeping LAN/Wi-Fi and power available. In Azure mode, a database outage suspends database-backed control; read the [availability behavior](docs/azure-sql.md#availability-and-recovery) before enabling equipment.

Reference: [Shelly Switch RPC](https://shelly-api-docs.shelly.cloud/gen2/ComponentsAndServices/Switch/).

### Existing sPlant pump

Leave the sPlant kit on its own timer. The matching LCD kit's manual describes onboard programming and USB power, not an external command interface. A switched USB supply is not treated as a reliable watering command. No pump role is exposed in the equipment API. Watering entries are manual observations and never automatically generated from an assumed pump schedule.

## Scheduling and recovery behavior

- Daily windows are **start-inclusive, end-exclusive** in the configured timezone. An end earlier than the start means an overnight window. Equal times are rejected.
- Overrides last 1–1440 minutes, defaulting to 60; elapsed time is measured using UTC timestamps. They survive service restarts. At expiry or **Resume schedule**, the configured schedule resumes. Without an enabled schedule, the outlet returns to off.
- Disabling a schedule does not immediately change the outlet's state. Use an off override to request off. Expired overrides are retained until the intended state is successfully confirmed.
- The controller reads the outlet, persists an intent before a switching command, then reads it again for confirmation. **Commanded** and **reported** states are separate. Errors mark reported state unknown. Even confirmed output is not proof that equipment is physically operating.
- On restart, current intended state is reconciled; missed transitions are never replayed. At daylight-saving changes, switching follows current local wall time.
- A daily photo is attempted only during its configured minute. A missed minute, including a nonexistent DST time, is skipped. A persistent claim allows at most one scheduled attempt per local date, even if the clock goes backward or the process restarts. Failed attempts require **Capture now** or the following day's run; they do not retry automatically. Changing timezone does not erase previous date claims.
- Sensor polling, photos, and equipment control use independent tasks. Camera errors do not stop schedules. Component failures are recorded on change rather than once per polling cycle.
- Back up and stop older application instances before the multi-user upgrade; older binaries do not enforce garden isolation. New SQLite installations apply embedded migrations automatically. Embedded backend-specific migrations run automatically at startup unless Azure migrations are explicitly disabled. Back up before upgrading.

## Backup and restore

**Azure mode:** use [Azure database and local photo backup guidance](docs/azure-sql.md#backups). The commands below back up a **SQLite installation only**; copying the Pi’s directory does not back up a hosted Azure SQL database.

Back up **both the database and photos together**, and keep the configuration file. Stop the service for the whole copy so the database cannot reference photos that were omitted. Stopping the service leaves outlets in their last state; choose an appropriate time for this maintenance.

From a directory with space for the backup:

```sh
sudo systemctl stop plant-journal
sudo tar -czf plant-journal-backup.tar.gz -C /var/lib plant-journal -C /etc/plant-journal config.toml
sudo systemctl start plant-journal
```

Check the tar command succeeded before moving on. The directory backup includes any SQLite `-wal`/`-shm` files, images, and persistent photo-run claims. Do not copy only a live `.sqlite3` file while WAL writes are active. For development, stop `cargo run` and copy the entire `data/` directory plus your TOML file.

To restore, extract into a staging directory first, stop the service, and retain the existing directory as a rollback copy:

```sh
mkdir restore-staging
sudo tar -xzf plant-journal-backup.tar.gz -C restore-staging
sudo systemctl stop plant-journal
sudo mv /var/lib/plant-journal /var/lib/plant-journal.before-restore
sudo cp -a restore-staging/plant-journal /var/lib/plant-journal
sudo chown -R plant-journal:plant-journal /var/lib/plant-journal
sudo install -m 640 -o root -g plant-journal restore-staging/config.toml /etc/plant-journal/config.toml
sudo systemctl start plant-journal
```

Use a fresh rollback-directory name if `.before-restore` already exists. Confirm plant entries, at least one full-size photo, settings, and outlet status. Existing enabled schedules resume immediately on startup; inspect the restored database/configuration before starting if that is not intended. Keep the rollback copy until validation is complete.

## API

All endpoints are under `/api/v1`. Mutations use JSON; timestamps are Unix seconds in UTC. IDs are opaque UUID strings. List responses are JSON arrays unless noted. Validation errors return `400`, missing records `404`, failed hardware captures `502`, and unexpected service/storage failures `500`, with an `error` string. Axum also returns `413` for oversized requests and `422` for malformed typed inputs. No permissive CORS headers are set.

| Endpoint | Behavior |
| --- | --- |
| `GET /catalogs` | Public starter collection names, IDs, counts, and examples |
| `GET /catalogs/imports` | Collection IDs already added to the selected garden |
| `POST /catalogs/import` | Add `{catalogs:["vegetables","herbs"]}` once per garden; preserve existing cards and edits |
| `GET /summary` | Counts, last reading/staleness, health, configured adapter modes, settings |
| `GET, POST /seeds` | List inventory / create `{name,variety?,strain_id?,quantity,unit,supplier?,breeder?,acquired_on?,packet_code?,purchase_year?,storage_location?,notes?}`; unit is `seeds` or `packets` |
| `PUT, DELETE /seeds/{id}` | Replace packet / delete only if no attempts or plants reference it |
| `GET /germination-attempts` | List attempts in the active garden, newest start date first |
| `POST /seeds/{id}/attempts` | Record `{started_on,seeds_sown,seeds_germinated?,notes?}`; dates are `YYYY-MM-DD`, null result means pending |
| `PUT, DELETE /seeds/{id}/attempts/{attempt}` | Replace an attempt / delete only after unlinking its plants |
| `GET, POST /plants` | List all plants / create `{name,species?,strain_id?,seed_id?,germination_id?,notes?,archived?}`; an attempt must belong to the chosen packet |
| `GET, PUT /plants/{id}` | Read / replace profile; archive through `archived` |
| `GET, POST /entries` | Optional `?plant=id`; create `{kind,body,occurred_at,plant_ids}` |
| `PUT, DELETE /entries/{id}` | Replace / delete entry and its calendar event |
| `GET /calendar?month=YYYY-MM` | Events and daily environmental summaries; optional `plant` and `kind` |
| `GET /photos` | Photos with plant and seed associations; optional `plant` and `seed` filters |
| `POST /photos/upload?plant={id}` or `?seed={id}` | Raw image body, `Content-Type: application/octet-stream`, maximum 10 MB; exactly one owner required |
| `POST /photos/capture` | `{}` links active plants; `{plant_ids:[...]}` uses explicit links |
| `PUT, DELETE /photos/{id}` | Replace `{plant_ids:[...]}` / delete record and image |
| `GET /photos/{id}/image` | Full image; simulated images are labeled SVGs |
| `GET /readings` | Optional `from,to`; maximum 31-day interval, default last 24 hours |
| `GET, POST /devices` | Status/schedules/overrides object / create `{name,role,adapter,address?,channel?}` |
| `PUT /devices/{id}` | Edit configuration; changing the connection clears overrides and disables its schedule |
| `PUT /schedules/{id}` | `{device_id,enabled,start_time,end_time}` using `HH:MM` |
| `PUT, DELETE /overrides/{id}` | `{on,minutes?}` / resume schedule; accepted commands return `202` |
| `GET, PUT /settings` | `{timezone,photo_enabled,photo_time}` |

Plant-linked care requires at least one plant. Archival retains links and permits historical edits. Photos may have no plant links. Sensor/camera configuration belongs in TOML rather than the API. Device roles are restricted to `light` and `fan`; adapters are `simulated` or `shelly`.

## Validation

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
node --check static/app.js  # optional development check; Node is not needed to run the app
```

The integration suite uses temporary SQLite databases, simulated/failing adapters, injected UTC times, and a local mock Shelly server. It covers journal persistence and archival, calendar timezones, shared photo links, duplicate prevention across restarts, DST, overrides, outlet timeouts, failed sensor reads/captures/writes, and database/photo restoration.

Additional database tests cover the shared execution layer, parameter binding, transaction rollback, and SQLite import. An opt-in [Azure SQL contract test](docs/azure-sql.md#tests) requires a dedicated hosted test database.

Physical Pi sensor, Kiyo, and outlet checks must be performed on the actual equipment. Automated tests cannot verify GPIO wiring, camera format support, load ratings, or the physical operation of connected equipment.

### Optional browser workflow test

With Google Chrome installed, install Playwright in a temporary location and run the self-contained browser test:

```sh
npm install --prefix /tmp/plant-journal-browser --no-save --package-lock=false playwright
cargo build --locked
PLAYWRIGHT_MODULE=/tmp/plant-journal-browser/node_modules/playwright node tests/browser.mjs
```

The test launches its own service with an isolated temporary database, exercises desktop and mobile workflows, checks for browser errors and horizontal overflow, saves screenshots to its temporary directory, and shuts down the service afterward. It does not modify your normal journal. Set `PLANT_ARTIFACTS` to choose a screenshot directory.

## License

[MIT NON-AI License](LICENSE). This custom, source-available license permits use, modification, and redistribution subject to its terms, but **prohibits all AI/ML use of the code**, including training, inference, AI integrations, and supplying the code to AI coding tools, unless separately authorized in writing by the applicable copyright holder(s). It is not the standard MIT License or an OSI-approved open-source license.

Third-party components and assets retain their own licenses. Previously granted licenses are not retroactively revoked. See the license file for the full terms.
