'use strict';
// Shared by signup, new-garden forms, and the collection importer.
window.CatalogChoices = (() => {
  const escape = value => String(value ?? '').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  function markup(catalogs, imported = []) {
    return `<fieldset class="catalog-choices"><legend>Starter collections (optional)</legend><p class="helper">Choose any mix, or leave everything unchecked to start empty. Cards begin unowned; adding a plant or seeds to a card marks it collected.</p><div class="catalog-choice-grid">${catalogs.map(c=>`<label class="catalog-choice ${imported.includes(c.id)?'is-imported':''}"><input type="checkbox" name="catalogs" value="${escape(c.id)}" data-count="${c.count}" aria-labelledby="catalog-label-${escape(c.id)}" aria-describedby="catalog-description-${escape(c.id)}" ${imported.includes(c.id)?'disabled checked':''}><span><span class="catalog-choice-heading"><strong id="catalog-label-${escape(c.id)}">${escape(c.name)}</strong><span class="badge">${imported.includes(c.id)?'Added':c.count+' cards'}</span></span><span class="helper" id="catalog-description-${escape(c.id)}">${escape(c.description)}</span><span class="catalog-examples">${c.examples.map(escape).join(' · ')}</span></span></label>`).join('')}</div><p class="helper catalog-selection-count" role="status">No collections selected.</p></fieldset>`;
  }
  function bind(root) {
    const update = () => {
      const boxes=[...root.querySelectorAll('input[name="catalogs"]:checked:not(:disabled)')];
      const count=boxes.reduce((n,b)=>n+Number(b.dataset.count),0);
      root.querySelector('.catalog-selection-count').textContent=count?`${boxes.length} selected · up to ${count} cards to add.`:'No new collections selected. You can add more later.';
    };
    root.querySelectorAll('input[name="catalogs"]').forEach(input=>input.addEventListener('change',update));update();
  }
  return {markup,bind};
})();

async function catalogEditor() {
  const [catalogs, imported] = await Promise.all([api('/catalogs'),api('/catalogs/imports')]);
  modal('Add starter collections',formWrap(`${CatalogChoices.markup(catalogs,imported)}<p class="helper">Shared with this garden. Existing cards and edits are kept. Each collection is added once, so deleted cards stay deleted.</p>`,'Add collections'),async data=>{
    const selected=data.getAll('catalogs');
    if(!selected.length)throw new Error('Choose a collection to add.');
    const result=await api('/catalogs/import','POST',{catalogs:selected});
    notice(`${result.added} ${result.added===1?'card':'cards'} added to your collection.`);
  });
  CatalogChoices.bind($('#dialog-body'));
}
