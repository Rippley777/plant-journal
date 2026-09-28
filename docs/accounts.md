# Accounts and shared gardens

Anyone can sign up with an email and a password of 12–128 characters. Signup
creates a private garden. The optional starter collection picker offers vegetables,
herbs, flowers, fruit and berries, houseplants, and cannabis. Nothing is selected
by default. Choose any combination or start with an empty collection. The same
picker is available when creating another garden; collections belong to that
garden, not every garden the account joins.

Use **Collection → Add starter collections** or the Settings shortcut to add more.
Cards begin unowned, while existing cards retain their status and notes. A linked
plant or positive seed quantity marks a card collected. Choosing a starter pack
does not add plants or seed stock. Collections import once, so retrying never
restores cards you deliberately removed. Account, garden, and starter-card creation
commit together. The API accepts an optional `catalogs` array on signup and garden
creation; omitting it or sending `[]` creates an empty collection.

Users may create more gardens and switch between gardens
from the sidebar. Each garden has one owner; only that owner can add or remove
registered collaborators under Settings. All collaborators can edit records and
equipment settings. Permission levels and ownership transfer are not part of this MVP.

## Activate the original owner

Back up the database and photos. Stop older web and Pi instances before upgrading:
older binaries do not understand garden membership. Migration 4 assigns every
existing record to **Ally’s garden**, owned by **ally.rippley@gmail.com**. That
account is reserved and cannot be claimed through public signup.

SQLite applies the migration at startup. For Azure SQL, run the new binary with
`--migrate` using a migration-capable database identity before starting the service.
Use the same `PLANT_CONFIG` and exported database environment as your installation.
Then run this locally in an interactive terminal:

```sh
cargo run --locked -- --set-password ally.rippley@gmail.com
```

For an installed binary, use `plant-journal --set-password ally.rippley@gmail.com`.
The command prompts twice without echoing the password. It does not start hardware
workers, and it revokes any existing sessions for the account. Do not put the
password in a command argument, source control, or a chat message. Sign in at
`/login` after setting it.

The same administrator-only command resets an existing user's password. Automated
email verification and self-service password recovery are not implemented; the
login page directs users to the administrator. Signup email addresses are therefore
unverified. Before adding a collaborator, confirm with that person that the
registered account is theirs. No invitation email is sent.

## Sessions and hosting

Passwords use Argon2id with random salts. Sessions use random 256-bit tokens;
only SHA-256 token hashes are stored in the database. Sessions expire after seven
days, survive app restarts, and are revoked by logout or administrator password
reset. Membership is checked on every authenticated request. Photo downloads
require membership in the photo's garden, including when opened in another tab.

Cookies are HttpOnly and SameSite=Lax, with Secure enabled by default. Require
HTTPS for hosted use. Only for local HTTP development, set `secure_cookies = false`
at the top level of your TOML file. Forwarding headers do not disable Secure.
Mutating browser requests reject cross-origin origins and require JSON or binary
photo content types.

Login/signup attempts are limited to 10 per email and 100 total per five-minute
window, per process. At most two password operations run concurrently per process.
For multiple web instances, apply a shared rate limit at the ingress as well.
Sessions are shared through the database. Photo files still require shared,
persistent storage if multiple web instances serve the same database.

## Equipment scope

Equipment, schedules, overrides, timezone, plants, seeds, photos, journal entries,
and calendar records belong to a garden. Users cannot link records across gardens.
API requests use the selected garden from their validated session or an explicit
`X-Garden-ID` header, whose membership is checked. The UI uses the header so a
switch in one browser tab cannot redirect writes from another tab.

The existing local controller remains bound to Ally’s original garden. It samples
sensors, captures photos, and reconciles only that garden's devices. Other gardens
can save equipment records, schedules, and overrides, but these do not contact
hardware. Connecting separate remote Pis is deferred. Cloud instances with
`automation_enabled = false` never start local workers.

## Validation

`cargo test --locked` includes public signup, reserved-owner protection, login,
logout, expired/forged sessions, garden isolation, cross-garden link rejection,
collaboration, equipment access, membership removal, password reset, and populated
legacy migration checks. The browser test covers optional starter choices, mixed collections, plant-type filtering, signup/login/logout, adding and
removing collaborators, garden switching, and the existing journal workflows.
The live Azure SQL contract test remains opt-in and needs a dedicated test database.
