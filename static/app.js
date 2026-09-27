'use strict';
const $ = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => [...root.querySelectorAll(s)];
const esc = v => String(v ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const page = document.body.dataset.page;
let plants = [], summary = {}, currentEntries = [], currentPhotos = [], equipment = {}, calendarEvents = [];
let selectedDay = '', month = '', selectedPlant = new URLSearchParams(location.search).get('plant') || '';
const names = ids => ids.map(id => plants.find(p => p.id === id)?.name || 'Archived plant');
const badges = ids => `<div class="plant-tags">${names(ids).map(n => `<span class="badge">${esc(n)}</span>`).join('')}</div>`;
const icons = {note:'≡',watering:'♧',feeding:'◇',pruning:'✂',repotting:'♧',photo:'▧',environment:'◌',device:'⌁',failure:'!',plant:'♧',system:'◈'};
const api = async (path, method = 'GET', body) => {
  const response = await fetch('/api/v1' + path, {method, headers: body === undefined ? {} : {'Content-Type':'application/json'}, body:body === undefined ? undefined : JSON.stringify(body)});
  if (!response.ok) {
    let message = await response.text();
    try { message = JSON.parse(message).error || message; } catch (_) {}
    throw new Error(message || `Request failed (${response.status})`);
  }
  return response.status === 204 || response.status === 202 ? null : response.json();
};
const formatDate = (timestamp, options = {}) => new Intl.DateTimeFormat('en-US', {timeZone:summary.settings?.timezone || 'America/Chicago', month:'short',day:'numeric', ...options}).format(new Date(timestamp * 1000));
const formatTime = timestamp => formatDate(timestamp, {hour:'numeric',minute:'2-digit'});
const dateKey = timestamp => {
  const parts = new Intl.DateTimeFormat('en-US', {timeZone:summary.settings?.timezone || 'America/Chicago',year:'numeric',month:'2-digit',day:'2-digit'}).formatToParts(new Date(timestamp*1000));
  return ['year','month','day'].map(k => parts.find(p => p.type === k).value).join('-');
};
const localInput = timestamp => {const d = new Date(timestamp*1000);return new Date(d.getTime()-d.getTimezoneOffset()*60000).toISOString().slice(0,16);};
function notice(message, error = false) {const el=$('#notice');el.textContent=message;el.className=error?'error':'';el.hidden=false;}
function empty(title, detail, action = '') {return `<div class="empty"><span class="sprout">♧</span><h3>${esc(title)}</h3><p>${esc(detail)}</p>${action}</div>`;}
function button(label, action, id='', style='') {return `<button type="button" class="${style}" data-action="${action}" data-id="${esc(id)}">${label}</button>`;}
function plantOptions(selected = '', all = true) {return `${all?'<option value="">All plants</option>':''}${plants.map(p=>`<option value="${p.id}" ${p.id===selected?'selected':''}>${esc(p.name)}${p.archived?' · archived':''}</option>`).join('')}`;}
function picker(ids = [], includeArchived = false) {const available=plants.filter(p=>includeArchived||!p.archived||ids.includes(p.id));return `<label>Linked plants</label><div class="plant-picker">${available.length?available.map(p=>`<label class="check-label"><input type="checkbox" name="plant_ids" value="${p.id}" ${ids.includes(p.id)?'checked':''}>${esc(p.name)}${p.archived?' (archived)':''}</label>`).join(''):'<span class="helper">Add a plant first to link an entry.</span>'}</div>`;}
function timeline(events, detail = false) {return events.length?`<ul class="timeline">${events.map(e=>`<li><span class="event-icon">${icons[e.kind]||'·'}</span><div><p class="event-title">${esc(e.title)}</p>${detail?`<p class="event-detail">${esc(e.detail)}</p>${badges(e.plant_ids || [])}`:''}<div class="event-time">${esc(formatTime(e.occurred_at))} · ${esc(e.kind)}</div>${e.kind==='photo'?`<a class="muted" href="/api/v1/photos/${e.entity_id}/image" target="_blank" rel="noopener">Open photo ↗</a>`:''}</div></li>`).join('')}</ul>`:empty('A quiet day','Notes, photos, and equipment activity will appear here.');}
function modal(title, contents, submit) {
  $('#dialog-title').textContent=title;$('#dialog-body').innerHTML=contents;const dialog=$('#editor');dialog.showModal();
  const form=$('form',dialog);
  if(form) form.addEventListener('submit', async event=>{
    event.preventDefault();const submitButton=$('button[type=submit]',form);submitButton.disabled=true;
    try {await submit(new FormData(form),form);dialog.close();await load();}
    catch(error){$('.form-error',form).textContent=error.message;}
    finally{submitButton.disabled=false;}
  });
}
function formWrap(contents, label='Save') {return `<form class="form-grid">${contents}<div class="form-error" role="alert"></div><div class="row"><button type="submit">${label}</button><button type="button" class="secondary" data-action="close">Cancel</button></div></form>`;}
function plantEditor(id) {
  const plant=plants.find(p=>p.id===id);
  modal(plant?'Edit plant':'Welcome a new plant',formWrap(`<label>Name<input name="name" required maxlength="120" value="${esc(plant?.name)}" placeholder="e.g. The little monstera"></label><label>Species or variety<input name="species" maxlength="160" value="${esc(plant?.species)}" placeholder="Monstera deliciosa"></label><label>Plant notes<textarea name="notes" maxlength="10000" placeholder="Where it lives, when you brought it home, things to remember…">${esc(plant?.notes)}</textarea></label>${plant?`<label class="check-label"><input type="checkbox" name="archived" ${plant.archived?'checked':''}>Archive plant (keep its history)</label>`:''}`),async data=>{
    await api('/plants'+(plant?'/'+plant.id:''),plant?'PUT':'POST',{name:data.get('name'),species:data.get('species'),notes:data.get('notes'),archived:data.has('archived')});notice(plant?'Plant updated.':'Your plant is ready for its first entry.');
  });
}
let currentSeeds = [];
function seedEditor(id) {
  const seed = currentSeeds.find(s => s.id === id);
  modal(seed ? 'Edit seeds' : 'Add seeds', formWrap(`
    <div class="grid two-col"><label>Name<input name="name" required maxlength="120" value="${esc(seed?.name)}" placeholder="e.g. Tomato"></label><label>Variety<input name="variety" maxlength="160" value="${esc(seed?.variety)}" placeholder="e.g. Cherokee Purple"></label></div>
    <div class="grid two-col"><label>Quantity on hand<input name="quantity" type="number" required min="0" max="1000000000" step="1" value="${seed?.quantity ?? 1}"></label><label>Unit<select name="unit"><option value="seeds" ${seed?.unit==='seeds'?'selected':''}>Seeds</option><option value="packets" ${(!seed || seed.unit==='packets')?'selected':''}>Packets</option></select></label></div>
    <div class="grid two-col"><label>Supplier<input name="supplier" maxlength="160" value="${esc(seed?.supplier)}"></label><label>Purchase year<input name="purchase_year" type="number" min="1900" max="2100" step="1" value="${esc(seed?.purchase_year)}"></label></div>
    <label>Storage location<input name="storage_location" maxlength="160" value="${esc(seed?.storage_location)}" placeholder="e.g. Fridge, seed box"></label>
    <label>Notes<textarea name="notes" maxlength="10000" placeholder="Sowing instructions, germination results, or anything to remember…">${esc(seed?.notes)}</textarea></label>`), async data => {
    await api('/seeds' + (seed ? '/' + seed.id : ''), seed ? 'PUT' : 'POST', {
      name: data.get('name'), variety: data.get('variety'), quantity: Number(data.get('quantity')),
      unit: data.get('unit'), supplier: data.get('supplier'),
      purchase_year: data.get('purchase_year') ? Number(data.get('purchase_year')) : null,
      storage_location: data.get('storage_location'), notes: data.get('notes')
    });
    notice('Seed inventory saved.');
  });
}
async function seedsPage() {
  heading('What’s tucked away for your next growing season.', button('+ Add seeds', 'seed'));
  currentSeeds = await api('/seeds');
  $('#content').innerHTML = currentSeeds.length ? `<div class="grid three-col">${currentSeeds.map(seed => `
    <article class="card card-body"><div class="row spread"><h2>${esc(seed.name)}</h2><span class="badge ${seed.quantity===0?'warn':''}">${seed.quantity===0?'Out of stock':`${seed.quantity} ${esc(seed.unit)}`}</span></div>
    <p class="muted">${esc(seed.variety || 'No variety recorded')}</p>
    ${seed.supplier?`<p>Supplier: ${esc(seed.supplier)}</p>`:''}
    ${seed.purchase_year?`<p>Purchased: ${seed.purchase_year}</p>`:''}
    ${seed.storage_location?`<p>Stored: ${esc(seed.storage_location)}</p>`:''}
    ${seed.notes?`<p class="entry-text">${esc(seed.notes)}</p>`:''}
    <div class="row section-space">${button('Edit','seed',seed.id,'secondary small')}${button('Delete','delete-seed',seed.id,'secondary small')}</div></article>`).join('')}</div>` : empty('Your next season starts here', 'Keep track of seed packets, varieties, and what you have left.', button('+ Add your first seeds', 'seed'));
}
function deleteSeed(id) {
  const seed = currentSeeds.find(s => s.id === id);
  modal('Delete seeds?', formWrap(`<p>Remove ${esc(seed.name)} from your seed inventory? This cannot be undone.</p>`, 'Delete permanently'), async () => {
    await api('/seeds/' + id, 'DELETE');notice('Seeds removed.');
  });
}

function entryEditor(id) {
  const entry=currentEntries.find(e=>e.id===id);const initialIds=entry?.plant_ids || (selectedPlant?[selectedPlant]:[]);
  modal(entry?'Edit journal entry':'A note from the grow space',formWrap(`<div class="grid two-col"><label>Entry type<select name="kind">${['note','watering','feeding','pruning','repotting'].map(k=>`<option ${entry?.kind===k?'selected':''} value="${k}">${k[0].toUpperCase()+k.slice(1)}</option>`).join('')}</select></label><label>When<input type="datetime-local" name="when" required value="${localInput(entry?.occurred_at || Date.now()/1000)}"></label></div><p class="helper">Enter time in this browser’s timezone. History displays in ${esc(summary.settings.timezone)}.</p>${picker(initialIds,true)}<label>Observation<textarea name="body" required maxlength="20000" placeholder="A new leaf, a little water, a change worth remembering…">${esc(entry?.body)}</textarea></label><p class="helper">Watering entries record care you confirm. The sPlant timer operates independently.</p>`),async data=>{
    await api('/entries'+(entry?'/'+entry.id:''),entry?'PUT':'POST',{kind:data.get('kind'),occurred_at:Math.floor(new Date(data.get('when')).getTime()/1000),body:data.get('body'),plant_ids:data.getAll('plant_ids')});notice('Journal entry saved.');
  });
}
function photoEditor(id) {
  const photo=currentPhotos.find(p=>p.id===id);
  modal('Link this photo to plants',formWrap(picker(photo.plant_ids,true)),async data=>{await api('/photos/'+id,'PUT',{plant_ids:data.getAll('plant_ids')});notice('Photo links updated.');});
}
async function confirmDelete(kind,id) {
  const label=kind==='photos'?'photo and its image file':'journal entry';
  modal('Delete this '+(kind==='photos'?'photo':'entry')+'?',formWrap(`<p>This removes the ${label} from the journal and calendar.</p>`,'Delete permanently'),async()=>{await api('/'+kind+'/'+id,'DELETE');notice('Deleted.');});
}
function deviceEditor(id) {
  const device=equipment.devices?.find(d=>d.id===id);
  modal(device?'Edit outlet':'Connect an outlet',formWrap(`<label>Name<input name="name" required maxlength="120" value="${esc(device?.name)}" placeholder="Tent light"></label><div class="grid two-col"><label>Equipment<select name="role"><option value="light" ${device?.role==='light'?'selected':''}>Light</option><option value="fan" ${device?.role==='fan'?'selected':''}>Fan</option></select></label><label>Connection<select name="adapter"><option value="simulated" ${device?.adapter==='simulated'?'selected':''}>Simulated outlet</option><option value="shelly" ${device?.adapter==='shelly'?'selected':''}>Shelly local RPC</option></select></label></div><label>Local address<input name="address" value="${esc(device?.address)}" placeholder="http://192.168.1.50"></label><label>Switch channel<input name="channel" type="number" min="0" max="3" value="${device?.channel||0}"></label><p class="helper">For Shelly, confirm the outlet is rated for the equipment’s load and supports local RPC. Address is unused for a simulated outlet. Changing a connection disables its automation and leaves the previous outlet’s state unchanged.</p>`,device?'Save outlet':'Add outlet'),async data=>{
    await api('/devices'+(device?'/'+device.id:''),device?'PUT':'POST',{name:data.get('name'),role:data.get('role'),adapter:data.get('adapter'),address:data.get('address'),channel:Number(data.get('channel'))});notice(device?'Outlet configuration saved.':'Outlet added. Configure a schedule when ready.');
  });
}
function overrideEditor(id,on) {
  const device=equipment.devices.find(d=>d.id===id);
  modal(`${on?'Turn on':'Turn off'} · ${device.name}`,formWrap(`<label>Override duration (minutes)<input type="number" name="minutes" value="60" min="1" max="1440" required></label><p class="helper">When the override ends, the schedule resumes. With no enabled schedule, the outlet returns to off.</p>`,'Apply override'),async data=>{
    await api('/overrides/'+id,'PUT',{on,minutes:Number(data.get('minutes'))});notice('Override queued. Device status refreshes on the next control cycle (about 10 seconds).');
  });
}
function heading(subtitle, actions='') {$('#subtitle').textContent=subtitle;$('#page-actions').innerHTML=actions;}
async function dashboard() {
  heading('Small observations. A fuller picture.',button('+ Add an entry','entry'));
  const [events,photos,deviceData]=await Promise.all([api('/calendar?month='+month),api('/photos'),api('/devices')]);
  currentPhotos=photos;const reading=summary.latest_reading;
  $('#content').innerHTML=`${summary.sensor_adapter==='simulated'||summary.camera_adapter==='simulated'?'<div class="notice">You’re exploring with simulated hardware. Sensor readings and camera images are clearly marked; connect your Pi hardware through the configuration file.</div>':''}<div class="grid stats"><div class="stat"><label>Growing here</label><span class="value">${summary.active_plants}</span><small>plants in your care</small></div><div class="stat"><label>Temperature</label><span class="value">${reading?reading.temperature_c.toFixed(1)+'°':'—'}</span><small>celsius · ${summary.sensor_stale?'stale or unavailable':esc(summary.sensor_adapter)}</small></div><div class="stat"><label>Humidity</label><span class="value">${reading?reading.humidity_percent.toFixed(0)+'%':'—'}</span><small>relative humidity · ${summary.sensor_stale?'awaiting a reading':'latest reading'}</small></div><div class="stat"><label>Little moments</label><span class="value">${summary.photos}</span><small>photos in your journal</small></div></div>
  <div class="grid two-col"><section class="card"><div class="card-header"><h2>A view of your grow space</h2><a href="/photos">All photos ↗</a></div>${photos.length?`<a href="/api/v1/photos/${photos[0].id}/image" target="_blank" rel="noopener"><img class="photo-large" src="/api/v1/photos/${photos[0].id}/image" alt="Latest grow-space capture"></a><div class="caption"><span>${esc(formatTime(photos[0].captured_at))}</span><span class="badge">${esc(photos[0].source)}</span></div>`:empty('Let’s start watching it grow','Take your first photo to begin a visual record.',button('Capture a moment','capture','','secondary'))}</section><section class="card"><div class="card-header"><h2>Recently in the journal</h2><a href="/calendar">Calendar ↗</a></div><div class="card-body">${timeline(events.filter(e=>e.kind!=='environment').slice(-5).reverse())}</div></section></div>
  <div class="grid two-col section-space"><section class="card"><div class="card-header"><h2>In your care</h2><a href="/plants">Your plants ↗</a></div><div class="card-body">${plants.filter(p=>!p.archived).length?plants.filter(p=>!p.archived).slice(0,5).map(p=>`<div class="health-row row spread"><a href="/plants?plant=${p.id}">${esc(p.name)}</a><span class="muted">${esc(p.species||'Growing at its own pace')}</span></div>`).join(''):empty('Room to grow','Add your first plant and give its story a home.',button('Add a plant','plant','','secondary'))}</div></section><section class="card"><div class="card-header"><h2>Behind the leaves</h2><a href="/equipment">Equipment ↗</a></div><div class="card-body">${deviceData.devices.length?deviceData.devices.map(d=>`<div class="health-row row spread"><span>${esc(d.name)}</span>${stateBadge(d)}</div>`).join(''):'<p class="muted">No outlets connected yet. Add a simulated outlet to try schedules and overrides.</p>'}<p class="helper section-space">Your sPlant watering timer runs independently. Record confirmed watering in the journal.</p></div></section></div>`;
}
async function plantsPage() {
  const plant=plants.find(p=>p.id===selectedPlant);
  if(plant) {
    heading(plant.species||'A growing story.',button('Edit plant','plant',plant.id,'secondary'));
    $('h1').textContent=plant.name;
    const [entries,photos]=await Promise.all([api('/entries?plant='+plant.id),api('/photos?plant='+plant.id)]);currentEntries=entries;currentPhotos=photos;
    $('#content').innerHTML=`<div class="toolbar"><a href="/plants" class="muted">← All plants</a>${plant.archived?'<span class="badge warn">Archived · history retained</span>':button('+ Add an entry','entry')}</div><section class="card card-body"><h2>Plant notes</h2><p class="profile-description">${esc(plant.notes||'The story starts here.')}</p><p class="muted">Added ${esc(formatDate(plant.created_at,{year:'numeric'}))}</p></section><div class="grid two-col section-space"><section><h2>Journal</h2><div class="section-space">${journalCards(entries)}</div></section><section><h2>Photo history</h2><div class="grid section-space">${photoCards(photos)}</div></section></div>`;return;
  }
  selectedPlant='';heading('Every plant has a story. Keep yours here.',button('+ Add a plant','plant'));
  $('#content').innerHTML=plants.length?`<div class="grid three-col">${plants.map(p=>`<article class="card plant-card ${p.archived?'archived':''}"><div class="plant-art" aria-hidden="true">♧</div><div class="card-body"><div class="row spread"><h3><a href="/plants?plant=${p.id}">${esc(p.name)}</a></h3>${p.archived?'<span class="badge">Archived</span>':''}</div><p class="muted">${esc(p.species||'A new addition')}</p><div class="row spread"><a class="muted" href="/plants?plant=${p.id}">View plant journal ↗</a>${button('Edit','plant',p.id,'secondary small')}</div></div></article>`).join('')}</div>`:empty('Your grow space starts here','Add a plant to begin collecting notes, care, and photos.',button('+ Add your first plant','plant'));
}
function journalCards(entries) {return entries.length?entries.map(e=>`<article class="card journal-entry"><div class="row spread"><span class="badge">${icons[e.kind]||'·'} ${esc(e.kind)}</span><span class="muted">${esc(formatTime(e.occurred_at))}</span></div><p class="entry-text">${esc(e.body)}</p>${badges(e.plant_ids)}<div class="row section-space">${button('Edit','entry',e.id,'secondary small')}${button('Delete','delete-entry',e.id,'secondary small')}</div></article>`).join(''):empty('A fresh page','Write down what you notice, or record a little care.',button('Add an entry','entry','','secondary'));}
async function journalPage() {
  heading('The everyday details that tell a bigger story.',button('+ Add an entry','entry'));
  currentEntries=await api('/entries'+(selectedPlant?'?plant='+selectedPlant:''));
  $('#content').innerHTML=`<div class="toolbar"><select id="plant-filter" aria-label="Filter by plant">${plantOptions(selectedPlant)}</select><span class="muted">${currentEntries.length} entries</span></div>${journalCards(currentEntries)}`;
  $('#plant-filter').addEventListener('change',e=>{selectedPlant=e.target.value;run(journalPage);});
}
async function calendarPage() {
  heading('A month of growth, gathered in one place.',button('+ Add an entry','entry'));
  const oldKind=$('#event-filter')?.value || '';
  $('#content').innerHTML=`<div class="toolbar"><div class="row">${button('←','prev-month','','secondary small')}<input id="month-picker" type="month" aria-label="Month" value="${month}">${button('→','next-month','','secondary small')}</div><div class="row"><select id="plant-filter" aria-label="Filter by plant">${plantOptions(selectedPlant)}</select><select id="event-filter" aria-label="Filter by event type"><option value="">All events</option>${Object.keys(icons).map(k=>`<option value="${k}" ${oldKind===k?'selected':''}>${k[0].toUpperCase()+k.slice(1)}</option>`).join('')}</select></div></div><div class="calendar-layout"><section class="card"><div class="calendar-grid" id="calendar-grid"></div></section><section class="card"><div class="card-header"><h2 id="day-title">Day details</h2></div><div class="card-body" id="day-events"></div></section></div><p class="helper section-space">${esc(summary.settings.timezone)} · Plant filters show linked events. Shared environment and equipment events appear under “All plants.”</p>`;
  $('#month-picker').addEventListener('change',e=>{if(e.target.value){month=e.target.value;selectedDay='';run(calendarPage);}});
  $('#plant-filter').addEventListener('change',e=>{selectedPlant=e.target.value;run(calendarGrid);});
  $('#event-filter').addEventListener('change',()=>run(calendarGrid));
  await calendarGrid();
}
async function calendarGrid() {
  const q=new URLSearchParams({month});if(selectedPlant)q.set('plant',selectedPlant);if($('#event-filter').value)q.set('kind',$('#event-filter').value);
  calendarEvents=await api('/calendar?'+q);
  const [year,m]=month.split('-').map(Number);const start=new Date(Date.UTC(year,m-1,1)).getUTCDay();const count=new Date(Date.UTC(year,m,0)).getUTCDate();
  const today=dateKey(Date.now()/1000);if(!selectedDay.startsWith(month))selectedDay=today.startsWith(month)?today:month+'-01';
  let html=['SUN','MON','TUE','WED','THU','FRI','SAT'].map(d=>`<div class="weekday">${d}</div>`).join('')+Array.from({length:start},()=>'<div class="day blank"></div>').join('');
  for(let day=1;day<=count;day++) {
    const key=month+'-'+String(day).padStart(2,'0');const events=calendarEvents.filter(e=>dateKey(e.occurred_at)===key);
    html+=`<button type="button" data-action="day" data-id="${key}" class="day ${key===today?'today':''} ${key===selectedDay?'selected':''}" aria-label="${key}, ${events.length} events"><span class="day-number">${day}</span>${events.slice(0,2).map(e=>`<span class="day-event ${e.kind}">${icons[e.kind]||'·'} ${esc(e.title)}</span>`).join('')}${events.length>2?`<span class="muted">+${events.length-2}</span>`:''}</button>`;
  }
  $('#calendar-grid').innerHTML=html;showDay(selectedDay);
}
function showDay(day) {selectedDay=day;$$('.day[data-id]').forEach(el=>el.classList.toggle('selected',el.dataset.id===day));$('#day-title').textContent=new Intl.DateTimeFormat('en-US',{month:'long',day:'numeric',timeZone:'UTC'}).format(new Date(day+'T12:00:00Z'));$('#day-events').innerHTML=timeline(calendarEvents.filter(e=>dateKey(e.occurred_at)===day),true);}
function photoCards(photos) {return photos.length?photos.map(p=>`<article class="card photo-card"><a href="/api/v1/photos/${p.id}/image" target="_blank" rel="noopener"><img src="/api/v1/photos/${p.id}/image" alt="Grow space photographed ${esc(formatTime(p.captured_at))}" loading="lazy"></a><div class="card-body"><div class="row spread"><span class="muted">${esc(formatTime(p.captured_at))}</span><span class="badge">${esc(p.source)}</span></div><div class="section-space">${badges(p.plant_ids)}</div><div class="row section-space">${button('Link plants','photo-links',p.id,'secondary small')}${button('Delete','delete-photo',p.id,'secondary small')}</div></div></article>`).join(''):empty('Watch the changes unfold','Daily photos will build a visual history. You can capture a moment now.',button('Capture now','capture','','secondary'));}
async function photosPage() {
  heading('The changes you might otherwise miss.',button('▧ Capture now','capture'));
  currentPhotos=await api('/photos'+(selectedPlant?'?plant='+selectedPlant:''));
  $('#content').innerHTML=`<div class="toolbar"><select id="plant-filter" aria-label="Filter photos by plant">${plantOptions(selectedPlant)}</select><span class="muted">Daily capture ${summary.settings.photo_enabled?'at '+esc(summary.settings.photo_time):'disabled'} · <a href="/settings">Settings ↗</a></span></div><div class="grid three-col">${photoCards(currentPhotos)}</div>`;
  $('#plant-filter').addEventListener('change',e=>{selectedPlant=e.target.value;run(photosPage);});
}
function chart(readings,key,label,unit) {
  if(!readings.length)return empty('Waiting for readings','Connect a sensor or use the simulated adapter to see environmental history.');
  const values=readings.map(r=>r[key]);let low=Math.min(...values),high=Math.max(...values);const pad=Math.max((high-low)*.15,1);low-=pad;high+=pad;
  const start=readings[0].recorded_at,end=Math.max(start+1,readings.at(-1).recorded_at);
  const x=t=>50+(t-start)/(end-start)*630,y=v=>185-(v-low)/(high-low)*160;
  // Preserve gaps over five minutes instead of implying uninterrupted measurements.
  let previous;const path=readings.map(r=>{const command=previous===undefined||r.recorded_at-previous>300?'M':'L';previous=r.recorded_at;return `${command}${x(r.recorded_at).toFixed(1)},${y(r[key]).toFixed(1)}`;}).join(' ');
  const lines=Array.from({length:4},(_,i)=>{const value=low+(high-low)*i/3;return `<line x1="50" y1="${y(value)}" x2="680" y2="${y(value)}"/><text x="2" y="${y(value)+4}">${value.toFixed(1)}</text>`;}).join('');
  return `<svg class="chart" viewBox="0 0 700 210" role="img" aria-label="${esc(label)} from ${values[0].toFixed(1)} to ${values.at(-1).toFixed(1)} ${unit}; range ${Math.min(...values).toFixed(1)}–${Math.max(...values).toFixed(1)}"><title>${esc(label)} (${unit})</title>${lines}<path d="${path}"/><circle cx="${x(end)}" cy="${y(values.at(-1))}" r="3" fill="#57754b"/></svg><div class="chart-caption"><span>${esc(formatTime(start))}</span><span>${esc(formatTime(end))}</span></div>`;
}
async function environmentPage() {
  heading('Get to know the conditions around your plants.');
  const hours=Number($('#reading-range')?.value||24);const readings=await api('/readings?from='+Math.floor(Date.now()/1000-hours*3600));
  $('#content').innerHTML=`<div class="toolbar"><span class="badge ${summary.sensor_stale?'warn':''}">${esc(summary.sensor_adapter)} · ${summary.sensor_stale?'stale / unavailable':'reporting'}</span><select id="reading-range" aria-label="Reading range">${[[24,'Last 24 hours'],[168,'Last 7 days'],[720,'Last 30 days']].map(([v,l])=>`<option value="${v}" ${v===hours?'selected':''}>${l}</option>`).join('')}</select></div><div class="grid"><section class="card"><div class="card-header"><h2>Temperature</h2><span class="muted">°C</span></div><div class="card-body">${chart(readings,'temperature_c','Temperature','°C')}</div></section><section class="card"><div class="card-header"><h2>Relative humidity</h2><span class="muted">% RH</span></div><div class="card-body">${chart(readings,'humidity_percent','Relative humidity','%')}</div></section></div><p class="helper section-space">${readings.length} samples · Every minute · Readings older than five minutes are marked stale. Monitoring does not control the fan.</p>`;
  $('#reading-range').addEventListener('change',()=>run(environmentPage));
}
function stateBadge(d) {return `<span class="badge ${d.reported_on===null?'warn':''}">${d.reported_on===null?'Unknown':d.reported_on?'Output on':'Output off'}</span>`;}
async function equipmentPage() {
  heading('Simple routines, with room to step in.',button('+ Add outlet','device'));
  equipment=await api('/devices');
  $('#content').innerHTML=`<div class="notice">The sPlant pump stays on its own timer. Outlets here control lights and fans only. Reported output state doesn’t independently confirm equipment operation.</div><div class="toolbar"><span class="muted">Commands apply on the next control cycle.</span>${button('Refresh status','refresh','','secondary small')}</div><div class="grid two-col">${equipment.devices.length?equipment.devices.map(d=>{
    const s=equipment.schedules.find(s=>s.device_id===d.id);const o=equipment.overrides.find(o=>o.device_id===d.id&&o.expires_at>Date.now()/1000);
    return `<section class="card"><div class="card-header"><h2>${esc(d.name)}</h2><span class="badge">${esc(d.adapter)} · ${esc(d.role)}</span></div><div class="card-body"><div data-device-health="${d.id}"><div class="row spread">${stateBadge(d)}<span class="muted">Last command: ${d.commanded_on===null?'none':d.commanded_on?'on':'off'}</span></div><p class="helper">${d.checked_at?'Checked '+esc(formatTime(d.checked_at)):'Waiting for first check'}</p>${d.last_error?`<p class="form-error">${esc(d.last_error)}</p>`:''}</div>${o?`<p class="muted">Override ${o.on_state?'on':'off'} until ${esc(formatTime(o.expires_at))}</p>`:''}<div class="row section-space">${button('On…','on',d.id,'secondary small')}${button('Off…','off',d.id,'secondary small')}${button('Resume schedule','resume',d.id,'secondary small')}${button('Edit outlet','device',d.id,'secondary small')}</div><form class="device-controls stack" data-schedule="${d.id}"><label class="check-label"><input type="checkbox" name="enabled" ${s.enabled?'checked':''}>Enable daily schedule</label><div class="schedule-row"><label>On at<input type="time" name="start" value="${esc(s.start_time)}" required></label><label>Off at<input type="time" name="end" value="${esc(s.end_time)}" required></label></div><p class="helper">${esc(summary.settings.timezone)} · Overnight windows supported. Disabling a schedule leaves the outlet in its current state.</p><div class="form-error" role="alert"></div><button type="submit" class="secondary small">Save schedule</button></form></div></section>`;
  }).join(''):empty('Make a little routine','Add a simulated outlet to explore schedules before connecting equipment.',button('Add an outlet','device','','secondary'))}</div>`;
  $$('[data-schedule]').forEach(form=>form.addEventListener('submit',async event=>{
    event.preventDefault();const data=new FormData(form);const id=form.dataset.schedule;const submit=$('button[type=submit]',form);submit.disabled=true;
    try{await api('/schedules/'+id,'PUT',{device_id:id,enabled:data.has('enabled'),start_time:data.get('start'),end_time:data.get('end')});notice('Schedule saved. It will apply on the next control cycle.');}
    catch(e){$('.form-error',form).textContent=e.message;}finally{submit.disabled=false;}
  }));
}
async function settingsPage() {
  heading('A few things to make this space yours.');const s=summary.settings;
  $('#content').innerHTML=`<div class="grid two-col"><section class="card"><div class="card-header"><h2>Time & daily photos</h2></div><div class="card-body"><form id="settings-form" class="form-grid"><label>Timezone<input name="timezone" value="${esc(s.timezone)}" required placeholder="America/Chicago"></label><p class="helper">Use an IANA timezone. It applies to schedules and the calendar; timestamps are stored in UTC.</p><label class="check-label"><input type="checkbox" name="photo_enabled" ${s.photo_enabled?'checked':''}>Take one photo every day</label><label>Capture time<input type="time" name="photo_time" value="${esc(s.photo_time)}" required></label><p class="helper">Choose a time when the grow lights are on. Daily photos link to all currently active plants; links can be edited later. Missed captures are skipped.</p><div class="form-error" role="alert"></div><button type="submit">Save settings</button></form></div></section><section class="card"><div class="card-header"><h2>Connections & health</h2></div><div class="card-body"><div class="health-row row spread"><span>Journal database</span><span class="badge">${summary.database_backend==='azure_sql'?'Azure SQL':'SQLite (local)'}</span></div><div class="health-row row spread"><span>Sensor</span><span class="badge">${esc(summary.sensor_adapter)}</span></div><div class="health-row row spread"><span>Camera</span><span class="badge">${esc(summary.camera_adapter)}</span></div>${summary.health.map(h=>`<div class="health-row"><div class="row spread"><span>${esc(h.component)}</span><span class="badge ${h.last_error?'error':''}">${h.last_error?'Needs attention':'Last operation succeeded'}</span></div><p class="helper">${h.last_error?esc(h.last_error):'Last success '+esc(formatTime(h.last_success))}</p></div>`).join('')}<p class="helper section-space">Hardware adapters are configured in the service’s TOML file. See the included README for Pi setup and backup/restore instructions.</p><p class="helper section-space">Single owner · ${summary.database_backend==='azure_sql'?'Azure SQL journal · Photos on this device':'Local SQLite journal'}</p></div></section></div>`;
  $('#settings-form').addEventListener('submit',async event=>{
    event.preventDefault();const form=event.target;const data=new FormData(form);const submit=$('button[type=submit]',form);submit.disabled=true;
    try{await api('/settings','PUT',{timezone:data.get('timezone'),photo_enabled:data.has('photo_enabled'),photo_time:data.get('photo_time')});notice('Settings saved.');await load();}catch(e){$('.form-error',form).textContent=e.message;}finally{submit.disabled=false;}
  });
}
async function load() {
  [plants,summary]=await Promise.all([api('/plants'),api('/summary')]);
  $('#storage-note').textContent=summary.database_backend==='azure_sql'?'Azure SQL journal · Local photos':'Local journal · Local photos';
  if(!month)month=dateKey(Date.now()/1000).slice(0,7);
  $('#today').textContent=formatDate(Date.now()/1000,{weekday:'long',year:'numeric'});
  await ({dashboard,plants:plantsPage,seeds:seedsPage,journal:journalPage,calendar:calendarPage,photos:photosPage,environment:environmentPage,equipment:equipmentPage,settings:settingsPage}[page] || dashboard)();
  $('#content').setAttribute('aria-busy','false');
}
async function run(fn) {try{await fn();}catch(error){notice(error.message,true);$('#content').setAttribute('aria-busy','false');}}
document.addEventListener('click',event=>{
  const target=event.target.closest('[data-action]');if(!target)return;
  const {action,id}=target.dataset;
  run(async()=>{
    if(action==='plant')plantEditor(id);
    if(action==='seed')seedEditor(id);
    if(action==='delete-seed')deleteSeed(id);
    if(action==='entry')entryEditor(id);
    if(action==='photo-links')photoEditor(id);
    if(action==='delete-entry')await confirmDelete('entries',id);
    if(action==='delete-photo')await confirmDelete('photos',id);
    if(action==='close')$('#editor').close();
    if(action==='device')deviceEditor(id);
    if(action==='on'||action==='off')overrideEditor(id,action==='on');
    if(action==='resume'){await api('/overrides/'+id,'DELETE');notice('Override ended. Waiting for device confirmation.');await equipmentPage();}
    if(action==='refresh')await load();
    if(action==='capture') {
      target.disabled=true;target.textContent='Capturing…';
      try{await api('/photos/capture','POST',{});notice('A new moment added to your journal.');await load();}
      finally{target.disabled=false;target.textContent='Capture now';}
    }
    if(action==='day')showDay(id);
    if(action==='prev-month'||action==='next-month') {const [y,m]=month.split('-').map(Number);const d=new Date(Date.UTC(y,m-1+(action==='next-month'?1:-1),1));month=d.toISOString().slice(0,7);selectedDay='';await calendarPage();}
  });
});
$('#close-dialog').addEventListener('click',()=>$('#editor').close());
$$('[data-nav]').forEach(el=>{el.classList.toggle('active',el.dataset.nav===page);if(el.dataset.nav===page)el.setAttribute('aria-current','page');});
run(load);

setInterval(()=>{
  if(document.hidden || $('#editor').open)return;
  if(page==='equipment') run(async()=>{
    equipment=await api('/devices');
    for(const d of equipment.devices){
      const el=$(`[data-device-health="${d.id}"]`);
      if(el)el.innerHTML=`<div class="row spread">${stateBadge(d)}<span class="muted">Last command: ${d.commanded_on===null?'none':d.commanded_on?'on':'off'}</span></div><p class="helper">${d.checked_at?'Checked '+esc(formatTime(d.checked_at)):'Waiting for first check'}</p>${d.last_error?`<p class="form-error">${esc(d.last_error)}</p>`:''}`;
    }
  });
},15000);
setInterval(()=>{if(!document.hidden&&!$('#editor').open&&['dashboard','environment'].includes(page))run(load);},60000);
