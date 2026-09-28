# Strain collection and lineage

Use **Collection** in the sidebar to browse your garden's card binder. Unowned and
wanted cards have gray artwork; collected cards show their full color. Search
names, species, breeders, and parent names, or filter by collection status.
Card artwork is an original botanical emblem, not a photograph of the cultivar.

- **Add a strain** records its name, optional species and breeder, notes, and
  collection status. Choose Wanted to keep a wishlist; choose Collected for a
  strain you have. Each name must be unique within its garden, ignoring case and
  repeated whitespace. Use distinct names for distinct lines or phenotypes.
- Plant and seed editors have a **Strain** field. Select a known name or type a
  new one to create a card. A linked plant or positive seed quantity unlocks it.
  A zero-quantity seed packet can have a strain without unlocking the card.
- Collection history persists after archiving a plant, using up seeds, or removing
  an inventory link. You can edit the card's status again when no active plant or
  positive seed quantity still links to it. Collected means recorded acquisition,
  not necessarily current stock.
- Open a card to see its linked plants/seeds, descendants, and **Ancestry** graph.
  Choose up to four generations, zoom, fit, or scroll. Select an ancestor to
  explore earlier generations. Plant profiles show lineage tags and the same graph.
- Add parent records first, then choose **Parent 1** and **Parent 2** in the strain
  editor. One or both may be unknown. Parent order does not imply sex, and the same
  strain may occupy both slots. Shared ancestors are valid; ancestry loops are
  rejected, including concurrent edits.
- Deleting a strain requires removing its plant, seed, and descendant links first.
  Cross plans also hold references to their parents and created strains; delete
  the plan first if you need to remove one of those records.
  Strains and collection status belong to the selected garden and follow its
  collaborator permissions; other gardens cannot access or link them.

## Cross planner

Open **Collection → Cross planner → Plan a cross**. Choose two strains already in the
garden's collection, give the plan a working name, and add optional species,
breeder, and notes. Parent order does not imply sex, and the same strain may fill
both slots. Save and edit plans without adding strain cards or collecting parents.

When you have the cross, choose **Create strain**. Review its final name and notes;
the app creates a collected card with both recorded parents in its ancestry. The
original plan stays linked to the created strain. Conversion happens once, even
if a request is retried. Duplicate strain names must be changed before conversion.
Converted plans remain as history; edit the strain record for subsequent changes.
Deleting a plan keeps its parents and any created strain.

Use **Show crosses** to view planned, created, or all crosses. Plans belong to the
selected garden and follow the same collaborator access as strain records. A plan
records intended parentage; it does not predict the traits of the resulting cross.

## Optional starter collections

Signup and **New garden** offer six optional packs. All are unchecked initially.
Choose any combination, or start empty and use **Collection → Add starter
collections** later. Settings has the same shortcut. The **Plant type** filter
narrows mixed collections to tomatoes, basil, strawberries, cannabis, and more.

| Pack | Cards |
| --- | ---: |
| Vegetables | 20 |
| Herbs | 16 |
| Flowers | 12 |
| Fruit & berries | 12 |
| Houseplants | 8 |
| Cannabis | 152 |

The 68 new entries are in [garden-catalogs.json](../resources/garden-catalogs.json).
Names include the crop to distinguish unrelated varieties with the same name.
The existing `species` field holds the plant type used by the filter. Each new
entry links to its catalog or horticultural reference; no care descriptions,
photos, or undocumented parentage are copied into the app.

References include [Johnny's Selected Seeds](https://www.johnnyseeds.com/herbs/basil/),
[UC Agriculture and Natural Resources](https://ucanr.edu/sites/default/files/2026-04/2026%20Plant%20Sale%20List%20Veggies.pdf),
[West Coast Seeds](https://www.westcoastseeds.com/products/little-gem),
[University of Minnesota Extension](https://extension.umn.edu/garden-and-home/yard-and-garden/gardening-in-minnesota/growing-strawberries-in-the-home-garden),
and the [Royal Horticultural Society](https://www.rhs.org.uk/plants/epipremnum).
The cards are a curated starting library, not a guarantee of suitability for a
particular climate. Breeder and ancestry fields stay empty where undocumented.

Each pack imports once per garden in a transaction. Imports skip existing names
without changing notes, status, or parent links. Repeating a request does not
restore deleted cards. Newly imported cards start unowned; link your plants and
seed packets explicitly or mark a card collected. Imports do not guess ownership
from general crop names or create inventory. A selection that would exceed the
1,000-card garden limit is rejected in full. Garden collaborators share the same
collection and can add packs using their existing editing permissions.

## Original owner's starter binder

On the first application startup after schema migration, the existing original
garden owned by `ally.rippley@gmail.com` gets 152 starter cards. Other gardens and
accounts receive only the packs they explicitly choose. This is a curated starting set of familiar cannabis
names and ancestry records, not an exhaustive or ranked worldwide popularity list.

The original 52 records are in [starter-strains.json](../resources/starter-strains.json);
the 100 additional records are in [expanded-strains.json](../resources/expanded-strains.json).
The expansion includes the ten seed lines shown in the supplied order, with
breeders recorded separately. Those lines have no assumed parentage. A product
listing is not evidence of ownership, so each card starts unowned unless linked
to an existing plant or positive-quantity seed inventory.

Names were selected using Leafly's [top strains collection](https://www.leafly.com/news/strains-products/top-100-marijuana-strains)
and [2025 sales review](https://www.leafly.com/news/strains-products/best-selling-weed-strains),
checked September 27, 2026. Blue Dream's recorded ancestry also has a
[dedicated source](https://www.leafly.com/strains/blue-dream). Each starter card has
its source link. No photos, descriptions, potency figures, or effects were copied.

Only selected, explicitly reported two-parent relationships are prefilled. Unknown,
ambiguous, and complex multigeneration origins stay unfilled; an empty parent slot
does not imply a strain has no ancestry. Reported cultivar lineage may differ from
your seed supplier's line or individual cut. The editor's source and lineage notes
let you keep that distinction with the record.

Starter cards begin unowned unless an existing plant's name/species or a seed's
name/variety exactly matches a catalog name or listed alias after case/whitespace
normalization. Conflicting matches are skipped. Existing strain links are preserved,
and zero seed quantity does not unlock a card. Archived plants count as historical
acquisitions during this initial import. Each catalog version imports once in a transaction; the 100 new cards are also
added to existing collections without resetting statuses, overwriting edits, or
recreating deleted cards.

## Database upgrade

Schema version 9 adds `garden_catalog_imports` to remember each chosen pack. It
recognizes prior cannabis imports so deleted cards stay deleted. Older SQLite
imports without this table remain supported. Schema version 7 adds `cross_plans` with garden-scoped parent links and an optional
reference to the created strain. Schema version 6 tracks the catalog import version so existing gardens receive the
new cards once. Schema version 5 adds `strains`, `strain_catalog_imports`, and nullable `strain_id`
columns on `plants` and `seeds`. A strain has two optional references to parent
strains rather than storing its family tree as a text field. Existing data is
retained. SQLite migrates on startup. For Azure SQL deployments with automatic
migration disabled, run `cargo run --locked -- --migrate` using your migration
identity before starting the upgraded app. See [Azure SQL upgrade notes](azure-sql.md#strain-collection-and-ancestry-schema-version-5).

The API exposes `GET/POST /api/v1/strains` and `PUT/DELETE /api/v1/strains/{id}`.
Plant and seed writes accept an optional `strain_id`. Strain writes include `name`,
`status` (`unowned`, `wanted`, `collected`), and optional `species`, `breeder`,
`notes`, `parent_one_id`, `parent_two_id`, `lineage_note`, and `source_url`.

Cross plans use `GET/POST /api/v1/cross-plans` and `PUT/DELETE /api/v1/cross-plans/{id}`.
Plan writes require `name`, `parent_one_id`, and `parent_two_id`, with optional
`species`, `breeder`, and `notes`. `POST /api/v1/cross-plans/{id}/convert` accepts a
JSON object with optional overrides for name, species, breeder, and notes. It
returns the strain `id` (201 when created; 200 when already converted).


`GET /api/v1/catalogs` is public and exposes only built-in pack metadata.
`GET /api/v1/catalogs/imports` and `POST /api/v1/catalogs/import` require membership
in the selected garden. The POST body is `{ "catalogs": ["vegetables", "herbs"] }`;
valid IDs are `vegetables`, `herbs`, `flowers`, `fruit`, `houseplants`, `cannabis`.
The response is `{ "added": 36, "imported": ["vegetables", "herbs"] }` on an empty
garden. Repeated imports return zero additions. Signup and garden creation accept
the same optional `catalogs` array.
