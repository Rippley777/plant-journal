'use strict';
let currentSeeds = [], currentAttempts = [];
let vaultSearch = '';
const vaultCount = (n, word) => `${n} ${word}${n===1?'':'s'}`;
const packetUrl = id => '/seeds?seed=' + encodeURIComponent(id);
const packetLabel = seed => seed.name + (seed.packet_code ? ' · ' + seed.packet_code : '');
// Calendar dates must not shift when the garden or browser uses another timezone.
const vaultDate = date => new Intl.DateTimeFormat('en-US', {year:'numeric',month:'short',day:'numeric',timeZone:'UTC'}).format(new Date(date+'T12:00:00Z'));
async function loadVault() {
  [currentSeeds,currentAttempts] = await Promise.all([api('/seeds'),api('/germination-attempts')]);
}
function seedEditor(id) {
  const seed = currentSeeds.find(s => s.id === id);
  modal(seed ? 'Edit seeds' : 'Add seeds', formWrap(`
    <div class="grid two-col vault-fields"><label>Name<input name="name" required maxlength="120" value="${esc(seed?.name)}" placeholder="e.g. Tomato"></label><label>Variety<input name="variety" maxlength="160" value="${esc(seed?.variety)}" placeholder="e.g. Cherokee Purple"></label></div>
    <div class="grid two-col vault-fields"><label>Quantity on hand<input name="quantity" type="number" required min="0" max="1000000000" step="1" value="${seed?.quantity ?? 1}"></label><label>Unit<select name="unit"><option value="seeds" ${seed?.unit==='seeds'?'selected':''}>Seeds</option><option value="packets" ${(!seed || seed.unit==='packets')?'selected':''}>Packets</option></select></label></div>
    <div class="grid two-col vault-fields"><label>Supplier<input name="supplier" maxlength="160" value="${esc(seed?.supplier)}"></label><label>Purchase year<input name="purchase_year" type="number" min="1900" max="2100" step="1" value="${esc(seed?.purchase_year)}"></label></div>
    <div class="grid two-col vault-fields"><label>Breeder<input name="breeder" maxlength="160" value="${esc(seed?.breeder)}" placeholder="Who bred this seed line?"></label><label>Acquired on<input type="date" name="acquired_on" value="${esc(seed?.acquired_on)}"></label></div>
    <label>Packet / lot label<input name="packet_code" maxlength="160" value="${esc(seed?.packet_code)}" placeholder="e.g. Spring order · packet 01"></label>
    ${strainPicker(seed?.strain_id)}
    <label>Storage location<input name="storage_location" maxlength="160" value="${esc(seed?.storage_location)}" placeholder="e.g. Fridge, seed box"></label>
    <label for="packet-notes">Notes</label><textarea id="packet-notes" name="notes" maxlength="10000" placeholder="Anything to remember about this packet…">${esc(seed?.notes)}</textarea><p class="helper">Stock is updated manually. Germination records and plant links do not change this quantity.</p>`), async data => {
    await api('/seeds' + (seed ? '/' + seed.id : ''), seed ? 'PUT' : 'POST', {
      strain_id: await resolveStrainName(data), name: data.get('name'), variety: data.get('variety'), quantity: Number(data.get('quantity')),
      unit: data.get('unit'), supplier: data.get('supplier'), breeder:data.get('breeder'), acquired_on:data.get('acquired_on')||null, packet_code:data.get('packet_code'),
      purchase_year: data.get('purchase_year') ? Number(data.get('purchase_year')) : null,
      storage_location: data.get('storage_location'), notes: data.get('notes')
    });
    notice('Seed vault saved.');
  });
}
async function seedsPage() {
  heading('Every packet has a beginning. Follow what grows from it.', button('+ Add seeds', 'seed'));
  await loadVault();
  currentPhotos=await api('/photos');
  const id=new URLSearchParams(location.search).get('seed');
  if(id) {
    const seed=currentSeeds.find(s=>s.id===id);
    if(seed) return packetDetail(seed);
    $('#content').innerHTML=empty('Packet not found','It may have been removed or belong to another garden.','<a href="/seeds" class="button secondary">Back to seed vault</a>');return;
  }
  $('h1').textContent='Seed vault';
  if(!currentSeeds.length) {
    $('#content').innerHTML=empty('Your next season starts here','Keep track of seed packets, breeders, and the plants they become.',button('+ Add your first seeds','seed'));return;
  }
  $('#content').innerHTML=`<section class="card card-body vault-banner"><div><p class="eyebrow">FROM PACKET TO PLANT</p><h2>A little library of beginnings.</h2><p class="muted">${currentSeeds.length} records · ${currentSeeds.filter(s=>s.quantity>0).length} in stock · ${plants.filter(p=>p.seed_id).length} linked plants</p></div><span class="vault-mark" aria-hidden="true">❧</span></section><div class="toolbar section-space"><label for="vault-search">Find a packet<input id="vault-search" type="search" value="${esc(vaultSearch)}" placeholder="Name, breeder, lot, or strain…"></label></div><p id="vault-count" class="helper" role="status"></p><div id="vault-cards" class="grid three-col section-space"></div>`;
  $('#vault-search').addEventListener('input',e=>{vaultSearch=e.target.value;renderVaultCards();});
  renderVaultCards();
}
function packetMetadata(seed) {
  return `${seed.packet_code?`<p>Packet / lot: ${esc(seed.packet_code)}</p>`:''}${seed.breeder?`<p>Breeder: ${esc(seed.breeder)}</p>`:''}${seed.supplier?`<p>Supplier: ${esc(seed.supplier)}</p>`:''}${seed.acquired_on?`<p>Acquired: ${esc(vaultDate(seed.acquired_on))}</p>`:seed.purchase_year?`<p>Purchased: ${seed.purchase_year}</p>`:''}${seed.storage_location?`<p>Stored: ${esc(seed.storage_location)}</p>`:''}`;
}
function stockBadge(seed) {return `<span class="badge ${seed.quantity===0?'warn':''}">${seed.quantity===0?'Out of stock':`${seed.quantity} ${esc(seed.quantity===1?seed.unit.slice(0,-1):seed.unit)}`}</span>`;}
function packetPhotos(seed) {const photos=currentPhotos.filter(p=>p.seed_ids.includes(seed.id));return photos.length?`<div class="grid section-space">${photoCards(photos)}</div>`:'';}
function renderVaultCards() {
  const query=vaultSearch.trim().toLowerCase();
  const visible=currentSeeds.filter(s=>[s.name,s.variety,s.breeder,s.supplier,s.packet_code,s.storage_location,strainById(s.strain_id)?.name].join(' ').toLowerCase().includes(query));
  $('#vault-count').textContent=`${visible.length} of ${currentSeeds.length} records shown`;
  $('#vault-cards').innerHTML=visible.length?visible.map(seed=>`<article class="card card-body vault-card"><div class="row spread"><h2><a href="${packetUrl(seed.id)}">${esc(seed.name)}</a></h2>${stockBadge(seed)}</div><p class="muted">${esc(seed.variety||'No variety recorded')}</p>${strainTags(seed.strain_id)}${packetMetadata(seed)}<p class="helper">${currentAttempts.filter(a=>a.seed_id===seed.id).length} attempts · ${plants.filter(p=>p.seed_id===seed.id).length} plants</p><a class="button secondary small" href="${packetUrl(seed.id)}">Open packet ↗</a><div class="row section-space">${button('Add photo','upload-seed',seed.id,'secondary small')}${button('Edit','seed',seed.id,'secondary small')}${button('Delete','delete-seed',seed.id,'secondary small')}</div>${packetPhotos(seed)}</article>`).join(''):empty('No matching packets','Try another name, breeder, or lot label.');
}
function packetDetail(seed) {
  $('h1').textContent=seed.name;
  heading(seed.variety||'One packet. Many possibilities.',button('Edit packet','seed',seed.id,'secondary'));
  const attempts=currentAttempts.filter(a=>a.seed_id===seed.id), linked=plants.filter(p=>p.seed_id===seed.id);
  const completed=attempts.filter(a=>a.seeds_germinated!==null);
  const sown=completed.reduce((n,a)=>n+a.seeds_sown,0), germinated=completed.reduce((n,a)=>n+a.seeds_germinated,0);
  $('#content').innerHTML=`<div class="toolbar"><a class="muted" href="/seeds">← Seed vault</a>${stockBadge(seed)}</div><div class="grid two-col"><section class="card card-body"><p class="eyebrow">PACKET RECORD</p><h2>Origins & storage</h2>${strainTags(seed.strain_id)}${packetMetadata(seed)}<p class="entry-text">${esc(seed.notes||'No packet notes yet.')}</p><div class="row">${button('Add photo','upload-seed',seed.id,'secondary small')}${button('Delete packet','delete-seed',seed.id,'secondary small')}</div>${packetPhotos(seed)}</section><section class="card card-body"><p class="eyebrow">GERMINATION RECORD</p><h2>${sown?Math.round(germinated/sown*100)+'% germinated':'Results still to come'}</h2><p>${sown?`${germinated} of ${sown} seeds across ${vaultCount(completed.length,'completed attempt')}.`:'Record an attempt, then add the result when you know it.'}</p><p class="muted">${vaultCount(attempts.length-completed.length,'pending attempt')} · ${vaultCount(linked.length,'linked plant')}</p><p class="helper">Pending attempts are excluded from the rate. Stock quantities are updated manually.</p>${button('+ Record attempt','germination',seed.id)}</section></div><section class="section-space"><h2>Germination history</h2><div class="grid two-col section-space">${attempts.length?attempts.map(a=>`<article id="attempt-${a.id}" class="card card-body"><div class="row spread"><h3>${esc(vaultDate(a.started_on))}</h3><span class="badge ${a.seeds_germinated===null?'warn':''}">${a.seeds_germinated===null?'Pending':`${a.seeds_germinated} / ${a.seeds_sown} germinated`}</span></div><p>${a.seeds_sown} seeds sown · ${vaultCount(plants.filter(p=>p.germination_id===a.id).length,'linked plant')}</p><p class="entry-text">${esc(a.notes)}</p><div class="row">${button('Add plant from attempt','attempt-plant',a.id,'secondary small')}${button('Edit attempt','germination',a.id,'secondary small')}${button('Delete attempt','delete-attempt',a.id,'secondary small')}</div></article>`).join(''):'<p class="muted">No attempts recorded for this packet.</p>'}</div></section><section class="card card-body section-space"><div class="row spread"><h2>Plants from this packet</h2>${button('+ Add plant from packet','packet-plant',seed.id,'secondary small')}</div>${linked.length?`<ul class="strain-records">${linked.map(p=>{const a=attempts.find(a=>a.id===p.germination_id);return `<li><a href="/plants?plant=${encodeURIComponent(p.id)}">♧ ${esc(p.name)}</a><span class="muted">${p.archived?'Archived · ':''}${a?'Sown '+esc(vaultDate(a.started_on)):'Packet linked'}</span></li>`;}).join('')}</ul>`:'<p class="muted">Add a plant here, or choose this packet under Seed origin when editing an existing plant.</p>'}</section>`;
}
function attemptEditor(id) {
  const attempt=currentAttempts.find(a=>a.id===id), seed=currentSeeds.find(s=>s.id===(attempt?.seed_id||id));
  modal(attempt?'Edit germination attempt':'Record germination attempt',formWrap(`<p class="helper">${esc(packetLabel(seed))}</p><label>Started on<input name="started_on" type="date" required value="${esc(attempt?.started_on||dateKey(Date.now()/1000))}"></label><div class="grid two-col vault-fields"><label>Seeds sown<input name="seeds_sown" type="number" min="1" max="1000000000" step="1" required value="${attempt?.seeds_sown||1}"></label><label>Seeds germinated<input name="seeds_germinated" type="number" min="0" max="1000000000" step="1" value="${attempt?.seeds_germinated??''}" placeholder="Pending"></label></div><p class="helper">Leave germinated blank while waiting. Enter zero if none germinated, or the final count to complete the attempt. Stock quantities stay unchanged.</p><label for="attempt-notes">Attempt notes</label><textarea id="attempt-notes" name="notes" maxlength="10000">${esc(attempt?.notes)}</textarea>`),async data=>{
    await api('/seeds/'+seed.id+'/attempts'+(attempt?'/'+attempt.id:''),attempt?'PUT':'POST',{started_on:data.get('started_on'),seeds_sown:Number(data.get('seeds_sown')),seeds_germinated:data.get('seeds_germinated')===''?null:Number(data.get('seeds_germinated')),notes:data.get('notes')});notice('Germination attempt saved.');
  });
}
function deleteAttempt(id) {
  const attempt=currentAttempts.find(a=>a.id===id);
  modal('Delete germination attempt?',formWrap('<p>This removes the recorded attempt. Unlink any plants from this attempt first; their packet links can be kept.</p>','Delete permanently'),async()=>{await api('/seeds/'+attempt.seed_id+'/attempts/'+id,'DELETE');notice('Attempt removed.');});
}
function attemptOptions(seed,selected) {
  return '<option value="">No attempt recorded</option>'+currentAttempts.filter(a=>a.seed_id===seed).map(a=>`<option value="${a.id}" ${a.id===selected?'selected':''}>${esc(vaultDate(a.started_on))} · ${a.seeds_sown} sown · ${a.seeds_germinated===null?'pending':a.seeds_germinated+' germinated'} · ${a.id.slice(0,6)}</option>`).join('');
}
function originPicker(source) {
  return `<fieldset class="vault-origin"><legend>Seed origin</legend><label for="plant-packet">Seed packet</label><select id="plant-packet" name="seed_id"><option value="">No packet linked</option>${currentSeeds.map(s=>`<option value="${s.id}" ${s.id===source.seed_id?'selected':''}>${esc(packetLabel(s))}${s.quantity===0?' · out of stock':''}</option>`).join('')}</select><label for="plant-attempt">Germination attempt</label><select id="plant-attempt" name="germination_id" ${source.seed_id?'':'disabled'}>${attemptOptions(source.seed_id,source.germination_id)}</select><p class="helper">Link a packet and, optionally, the attempt this plant came from. Its origin stays with its history when archived.</p></fieldset>`;
}
function bindOriginPicker() {
  $('#plant-packet').addEventListener('change',e=>{const seed=e.target.value;$('#plant-attempt').innerHTML=attemptOptions(seed);$('#plant-attempt').disabled=!seed;});
}
function originSection(plant) {
  if(!plant.seed_id)return '';
  const seed=currentSeeds.find(s=>s.id===plant.seed_id), attempt=currentAttempts.find(a=>a.id===plant.germination_id);
  if(!seed)return '';
  return `<section class="card card-body section-space"><p class="eyebrow">FROM THE SEED VAULT</p><h2>Seed origin</h2><p><a href="${packetUrl(seed.id)}">❧ ${esc(packetLabel(seed))} ↗</a></p>${seed.breeder?`<p class="muted">Breeder: ${esc(seed.breeder)}</p>`:''}${attempt?`<p><a href="${packetUrl(seed.id)}#attempt-${attempt.id}">Germination attempt · ${esc(vaultDate(attempt.started_on))} ↗</a></p>`:'<p class="muted">No germination attempt linked.</p>'}</section>`;
}
function deleteSeed(id) {
  const seed = currentSeeds.find(s => s.id === id);
  modal('Delete seeds?', formWrap(`<p>Remove ${esc(seed.name)} from your seed vault? Packets with germination history or linked plants must be kept until those records are removed. This cannot be undone.</p>`, 'Delete permanently'), async () => {
    await api('/seeds/' + id, 'DELETE');if(new URLSearchParams(location.search).get('seed')===id)history.replaceState({},'', '/seeds');notice('Seeds removed.');
  });
}
