# Strain collection and lineage

Use **Strains** in the sidebar to browse your garden's card binder. Unowned and
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
  Strains and collection status belong to the selected garden and follow its
  collaborator permissions; other gardens cannot access or link them.

## Starter binder

On the first application startup after schema migration, the existing original
garden owned by `ally.rippley@gmail.com` gets 52 starter cards. Other gardens and
accounts are not prepopulated. This is a curated starting set of familiar cannabis
names and ancestry records, not an exhaustive or ranked worldwide popularity list.

The starter records are in [starter-strains.json](../resources/starter-strains.json).
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
acquisitions during this initial import. The import runs once in a transaction;
restarts do not reset statuses, overwrite edits, or recreate deleted cards.

## Database upgrade

Schema version 5 adds `strains`, `strain_catalog_imports`, and nullable `strain_id`
columns on `plants` and `seeds`. A strain has two optional references to parent
strains rather than storing its family tree as a text field. Existing data is
retained. SQLite migrates on startup. For Azure SQL deployments with automatic
migration disabled, run `cargo run --locked -- --migrate` using your migration
identity before starting the upgraded app. See [Azure SQL upgrade notes](azure-sql.md#strain-collection-and-ancestry-schema-version-5).

The API exposes `GET/POST /api/v1/strains` and `PUT/DELETE /api/v1/strains/{id}`.
Plant and seed writes accept an optional `strain_id`. Strain writes include `name`,
`status` (`unowned`, `wanted`, `collected`), and optional `species`, `breeder`,
`notes`, `parent_one_id`, `parent_two_id`, `lineage_note`, and `source_url`.
