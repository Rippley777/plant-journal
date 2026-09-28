'use strict';
let strains = [];
let strainSearch = '', strainStatus = 'all';
const strainById = id => strains.find(s => s.id === id);
const strainKey = value => value.trim().replace(/\s+/g, ' ').toLowerCase();
const strainParents = strain => [strain?.parent_one_id, strain?.parent_two_id].filter(Boolean).map(strainById).filter(Boolean);
const strainUrl = id => '/strains?strain=' + encodeURIComponent(id);
const strainStatusLabel = status => ({collected:'Collected',wanted:'Wanted',unowned:'Not collected'}[status] || 'Not collected');
function strainPicker(id) {
  return `<label for="record-strain">Strain</label><input id="record-strain" name="strain" list="strain-options" maxlength="120" value="${esc(strainById(id)?.name || '')}" placeholder="Choose a strain or type a new name" autocomplete="off"><datalist id="strain-options">${strains.map(s => `<option value="${esc(s.name)}"></option>`).join('')}</datalist><p class="helper">Choose an existing strain or type a new one to add its card. Leave blank if unknown. Manage parents in <a href="/strains">Strain collection ↗</a>.</p>`;
}
async function resolveStrainName(data) {
  const name = String(data.get('strain') || '').trim();
  if (!name) return null;
  const found = strains.find(s => strainKey(s.name) === strainKey(name));
  if (found) return found.id;
  const input = {name, species:data.get('species') || '', status:'unowned'};
  const result = await api('/strains', 'POST', input);
  strains.push({...input, id:result.id});
  return result.id;
}
function strainTags(id) {
  const strain = strainById(id);
  if (!strain) return '';
  return `<div class="strain-tags"><a class="badge strain-tag" href="${strainUrl(id)}">◇ ${esc(strain.name)}</a>${strainParents(strain).map(p => `<a class="badge" href="${strainUrl(p.id)}" title="Parent strain">↳ ${esc(p.name)}</a>`).join('')}</div>`;
}
function strainArt(strain) {
  // Original, deterministic botanical emblems; no remote images or invented strain photos.
  const mark = [...strain.name].reduce((n,c) => (n * 31 + c.codePointAt(0)) >>> 0, 0);
  const petals = [5,7,9][mark % 3];
  return `<svg class="strain-emblem" viewBox="0 0 200 180" aria-hidden="true"><circle class="emblem-orbit" cx="100" cy="87" r="69"/><path class="emblem-orbit" d="M100 7L179 47V127L100 167L21 127V47Z"/><g class="emblem-leaves">${Array.from({length:petals},(_,i) => `<path transform="rotate(${i * 360 / petals} 100 87)" d="M100 87Q73 57 100 26Q127 57 100 87Z"/>`).join('')}</g><circle class="emblem-core" cx="100" cy="87" r="14"/><path class="emblem-spark" d="M23 18v12m-6-6h12m146 126v12m-6-6h12"/></svg>`;
}
function strainTone(strain) {
  return 'tone-' + ([...strain.name].reduce((n,c) => n + c.codePointAt(0), 0) % 5);
}
function strainCard(strain) {
  const number = strain.id.slice(0,6).toUpperCase();
  const parents = strainParents(strain);
  return `<a class="strain-card ${strainTone(strain)} ${strain.status === 'collected' ? 'is-collected' : 'is-locked'}" href="${strainUrl(strain.id)}" aria-label="${esc(strain.name)} · ${strainStatusLabel(strain.status)}"><div class="strain-card-top"><span>FIELDNOTES / ${number}</span><span>${strain.status === 'collected' ? '✦' : '◇'}</span></div><div class="strain-card-art">${strainArt(strain)}<span class="strain-card-seal">${strain.status === 'wanted' ? 'WISHLIST' : strain.status === 'collected' ? 'IN YOUR COLLECTION' : 'UNDISCOVERED'}</span></div><div class="strain-card-copy"><p class="strain-card-species">${esc(strain.species || 'Plant strain')}</p><h2>${esc(strain.name)}</h2><p class="strain-card-parents">${parents.length ? parents.map(p => esc(p.name)).join(' × ') : 'An ancestry waiting to unfold'}</p><div class="strain-card-bottom"><span>${strainStatusLabel(strain.status)}</span><span>View lineage ↗</span></div></div></a>`;
}
async function strainsPage() {
  if (new URLSearchParams(location.search).get('view') === 'crosses') return crossPlansPage();
  const id = new URLSearchParams(location.search).get('strain');
  const selected = strainById(id);
  if (id && !selected) {
    heading('This strain is not in the selected garden.');
    $('#content').innerHTML=empty('Strain not found','It may have been removed or belong to another garden.','<a class="button secondary" href="/strains">Back to collection</a>');return;
  }
  if (selected) return strainDetail(selected);
  $('h1').textContent='Strain collection';
  heading('A living collection. Every strain has a story.',button('+ Add a strain','strain'));
  const collected=strains.filter(s => s.status==='collected').length, wanted=strains.filter(s => s.status==='wanted').length;
  $('#content').innerHTML=`<section class="collection-banner"><div><p class="eyebrow">THE GENETICS BINDER</p><h2>Grow your collection.</h2><p>Keep the classics. Discover the connections. Make room for your own hybrids.</p><div class="collection-progress"><progress value="${collected}" max="${Math.max(strains.length,1)}" aria-label="Strains collected"></progress><span>${collected} of ${strains.length} collected</span></div></div><div class="collection-counts"><div><strong>${collected}</strong><span>COLLECTED</span></div><div><strong>${wanted}</strong><span>ON YOUR WISHLIST</span></div></div></section><div class="toolbar collection-toolbar"><label class="collection-search" for="strain-search">Find a strain<input id="strain-search" type="search" value="${esc(strainSearch)}" placeholder="Search names, breeders, or parents…"></label><div><label for="strain-status">Show cards</label><select id="strain-status">${[['all','All strains'],['collected','Collected'],['wanted','Wanted'],['unowned','Not collected']].map(([v,t])=>`<option value="${v}" ${strainStatus===v?'selected':''}>${t}</option>`).join('')}</select></div></div><p class="helper" id="strain-count" role="status"></p><div class="strain-grid section-space" id="strain-cards"></div><p class="helper section-space">Cards unlock when you add a linked plant, add seeds you have on hand, or mark a strain collected. Your collection keeps its history when seeds run out. Collections are shared within this garden.</p>`;
  $('#strain-search').addEventListener('input',e=>{strainSearch=e.target.value;renderStrainCards();});
  $('#content').insertAdjacentHTML('afterbegin', strainViews(false));
  $('#strain-status').addEventListener('change',e=>{strainStatus=e.target.value;renderStrainCards();});
  renderStrainCards();
}
function renderStrainCards() {
  const query=strainKey(strainSearch);
  const visible=strains.filter(s => (strainStatus==='all' || s.status===strainStatus) && strainKey([s.name,s.species,s.breeder,...strainParents(s).map(p=>p.name)].join(' ')).includes(query));
  $('#strain-count').textContent=`${visible.length} ${visible.length===1?'card':'cards'} shown`;
  $('#strain-cards').innerHTML=visible.length?visible.map(strainCard).join(''):empty(strains.length?'No matching cards':'Your collection starts here',strains.length?'Try another name or card filter.':'Add a strain you have or one you want to find.',button('+ Add a strain','strain','','secondary'));
}
function sourceLink(url) {
  try {const parsed=new URL(url);if(['http:','https:'].includes(parsed.protocol))return `<a class="muted" href="${esc(parsed.href)}" target="_blank" rel="noopener noreferrer">Lineage / catalog source ↗</a>`;} catch (_) {}
  return '';
}
async function strainDetail(strain) {
  $('h1').textContent=strain.name;
  heading(strain.species || 'One branch of a growing family.',button('Edit strain','strain',strain.id,'secondary'));
  const seeds=await api('/seeds');
  const linkedPlants=plants.filter(p=>p.strain_id===strain.id), linkedSeeds=seeds.filter(s=>s.strain_id===strain.id);
  const children=strains.filter(s=>s.parent_one_id===strain.id || s.parent_two_id===strain.id);
  $('#content').innerHTML=`<div class="toolbar"><a class="muted" href="/strains">← All strain cards</a><span class="badge">${strainStatusLabel(strain.status)}</span></div><div class="strain-detail-layout"><div>${strainCard(strain)}</div><section class="card card-body stack"><div><p class="eyebrow">STRAIN RECORD</p><h2>${strainParents(strain).length ? 'Connected by their roots.' : 'Start a family story.'}</h2>${strainTags(strain.id)}</div>${strain.breeder?`<p class="helper">Breeder / origin: ${esc(strain.breeder)}</p>`:''}<p class="profile-description">${esc(strain.notes || 'Add notes about this strain, its origins, and what makes your particular line unique.')}</p><div class="row">${strain.status!=='collected'?button('✦ Mark collected','collect-strain',strain.id):''}${strain.status==='unowned'?button('♡ Add to wishlist','want-strain',strain.id,'secondary'):''}${strain.status==='wanted'?button('Remove from wishlist','unwant-strain',strain.id,'secondary'):''}${button('Edit parents & details','strain',strain.id,'secondary')}</div><p class="helper">Collected cards stay unlocked as part of your history. A linked active plant or seeds on hand always counts as collected.</p>${strain.lineage_note?`<p class="helper">${esc(strain.lineage_note)}</p>`:''}${sourceLink(strain.source_url)}</section></div>${lineageSection(strain.id)}<div class="grid two-col section-space"><section class="card card-body"><h2>In your garden</h2>${linkedPlants.length || linkedSeeds.length?`<ul class="strain-records">${linkedPlants.map(p=>`<li><a href="/plants?plant=${encodeURIComponent(p.id)}">♧ ${esc(p.name)}</a><span class="muted">${p.archived?'Archived plant':'Plant'}</span></li>`).join('')}${linkedSeeds.map(s=>`<li><a href="/seeds">❧ ${esc(s.name)}</a><span class="muted">${s.quantity} ${esc(s.unit)}</span></li>`).join('')}</ul>`:'<p class="muted">Choose this strain when adding or editing a plant or seed packet.</p>'}</section><section class="card card-body"><h2>Next generation</h2>${children.length?`<div class="strain-tags section-space">${children.map(s=>`<a class="badge strain-tag" href="${strainUrl(s.id)}">${esc(s.name)} ↗</a>`).join('')}</div>`:'<p class="muted">Strains that list this one as a parent will appear here.</p>'}</section></div><div class="section-space">${button('Delete strain','delete-strain',strain.id,'secondary small')}</div>`;
  bindLineage(strain.id);
}
function strainEditor(id) {
  const strain=strainById(id);
  // Exclude this strain and its descendants from parent choices.
  const excluded=new Set(id?[id]:[]);
  let changed=true;
  while(changed){changed=false;for(const s of strains){if(!excluded.has(s.id) && (excluded.has(s.parent_one_id)||excluded.has(s.parent_two_id))){excluded.add(s.id);changed=true;}}}
  const options=selected=>'<option value="">Unknown / not recorded</option>'+strains.filter(s=>!excluded.has(s.id)).map(s=>`<option value="${s.id}" ${s.id===selected?'selected':''}>${esc(s.name)}</option>`).join('');
  modal(strain?'Edit strain':'Add a strain',formWrap(`<label>Strain name<input name="name" required maxlength="120" value="${esc(strain?.name)}" placeholder="Give this strain a name"></label><div class="grid two-col"><label>Species<input name="species" maxlength="160" value="${esc(strain?.species)}" placeholder="e.g. Cannabis or Tomato"></label><label>Breeder / origin<input name="breeder" maxlength="160" value="${esc(strain?.breeder)}"></label></div><label for="strain-state">Collection status</label><select id="strain-state" name="status">${[['collected','Collected — I have this strain'],['wanted','Wanted — on my wishlist'],['unowned','Not collected — catalog only']].map(([v,t])=>`<option value="${v}" ${(strain?.status||'collected')===v?'selected':''}>${t}</option>`).join('')}</select><div class="grid two-col"><div class="stack"><label for="parent-one">Parent 1</label><select id="parent-one" name="parent_one_id">${options(strain?.parent_one_id)}</select></div><div class="stack"><label for="parent-two">Parent 2</label><select id="parent-two" name="parent_two_id">${options(strain?.parent_two_id)}</select></div></div><p class="helper">Add parent strains to the collection first. Either parent can be unknown; parent order does not imply sex. You can use the same parent for both slots. Descendants cannot be parents.</p><label>Lineage notes<textarea name="lineage_note" maxlength="2000" placeholder="Recorded parentage, uncertain ancestry, or details about this line…">${esc(strain?.lineage_note)}</textarea></label><label>Source URL<input name="source_url" type="url" maxlength="1000" value="${esc(strain?.source_url)}" placeholder="https://…"></label><label>Strain notes<textarea name="notes" maxlength="10000">${esc(strain?.notes)}</textarea></label>`,'Save strain'),async data=>{
    const input=Object.fromEntries(data);input.parent_one_id=input.parent_one_id||null;input.parent_two_id=input.parent_two_id||null;
    await api('/strains'+(strain?'/'+strain.id:''),strain?'PUT':'POST',input);notice('Strain saved to your collection.');
  });
}
async function setStrainStatus(id,status) {
  const s=strainById(id);if(!s)return;
  const {name,species,breeder,notes,parent_one_id,parent_two_id,lineage_note,source_url}=s;
  await api('/strains/'+id,'PUT',{name,species,breeder,notes,parent_one_id,parent_two_id,lineage_note,source_url,status});
  await load();notice(strainById(id)?.status==='collected'?'Strain collected. Your card is unlocked.':'Wishlist updated.');
}
function deleteStrain(id) {
  const s=strainById(id);
  modal('Delete strain?',formWrap(`<p>Remove ${esc(s.name)} and its card? Strains linked to plants, seeds, descendants, or cross plans must be unlinked first.</p>`,'Delete strain'),async()=>{
    await api('/strains/'+id,'DELETE');history.replaceState(null,'','/strains');notice('Strain removed.');
  });
}
function lineageSection(id) {
  return `<section class="card section-space" aria-labelledby="lineage-title"><div class="card-header"><div><p class="eyebrow">FOLLOW THE ROOTS</p><h2 id="lineage-title">Ancestry</h2></div><a href="${strainUrl(id)}">Open strain ↗</a></div><div class="card-body"><div class="toolbar"><div><label for="lineage-depth">Generations</label><select id="lineage-depth"><option value="2">Parents & grandparents</option><option value="3">3 generations</option><option value="4">4 generations</option></select></div><div class="row"><button type="button" class="secondary small" id="lineage-out" aria-label="Zoom out ancestry">−</button><button type="button" class="secondary small" id="lineage-in" aria-label="Zoom in ancestry">+</button><button type="button" class="secondary small" id="lineage-fit">Fit graph</button></div></div><p class="helper">Select a strain to explore its family. Scroll the graph to follow the branches. Unknown parents stay unfilled.</p><div class="lineage-viewport section-space" tabindex="0" role="region" aria-label="Scrollable ancestry graph"><div id="lineage-graph"></div></div><div id="lineage-summary" class="helper section-space"></div></div></section>`;
}
function bindLineage(id) {
  let scale=1, naturalWidth=0, naturalHeight=0;
  const draw=()=>{
    const depth=Number($('#lineage-depth').value), nodes=[], edges=[];let leaf=0;
    const visit=(strain,level,seen)=>{
      const node={strain,x:24+level*250,y:0};nodes.push(node);
      const known=strain && !seen.has(strain.id) && (strain.parent_one_id || strain.parent_two_id);
      if(known && level<depth){
        const next=new Set(seen);next.add(strain.id);
        const parents=[strain.parent_one_id,strain.parent_two_id].map(pid=>visit(strainById(pid),level+1,next));
        node.y=(parents[0].y+parents[1].y)/2;parents.forEach(parent=>edges.push({node,parent}));
      } else {node.y=50+leaf++*100;}
      return node;
    };
    visit(strainById(id),0,new Set());
    naturalWidth=Math.max(...nodes.map(n=>n.x))+220;naturalHeight=Math.max(180,leaf*100+40);
    const lines=edges.map(({node:n,parent:p})=>`<path d="M${n.x+200},${n.y+30}H${n.x+225}V${p.y+30}H${p.x}"/>`).join('');
    const boxes=nodes.map(n=>{
      const s=n.strain;
      const name=s?.name||'Unknown parent';
      const words=name.match(/.{1,23}(?:\s|$)|.{1,23}/g)||[name];
      const label=words.map(w=>w.trim()).slice(0,2);
      if(words.length>2)label[1]=label[1].slice(0,20)+'…';
      const box=`<rect x="${n.x}" y="${n.y}" width="200" height="72" rx="7"/><text x="${n.x+13}" y="${n.y+26}">${label.map((line,i)=>`<tspan x="${n.x+13}" dy="${i?17:0}">${esc(line)}</tspan>`).join('')}</text><text class="lineage-node-status" x="${n.x+13}" y="${n.y+60}">${s?strainStatusLabel(s.status):'Not recorded'}</text>`;
      return s?`<a href="${strainUrl(s.id)}" class="lineage-node ${s.id===id?'lineage-root':''}" aria-label="Explore ${esc(s.name)} ancestry"><title>${esc(s.name)}</title>${box}</a>`:`<g class="lineage-node lineage-unknown">${box}</g>`;
    }).join('');
    $('#lineage-graph').innerHTML=`<svg class="lineage-svg" width="${naturalWidth}" height="${naturalHeight}" viewBox="0 0 ${naturalWidth} ${naturalHeight}" role="group" aria-label="Ancestry of ${esc(strainById(id)?.name)}"><g class="lineage-edges">${lines}</g>${boxes}</svg>`;
    const root=strainById(id);const parents=strainParents(root);
    $('#lineage-summary').textContent=parents.length?`${root.name}: ${parents.map(p=>p.name).join(' × ')}. Shows up to ${depth} generations; select an ancestor to go further.`:'No parents recorded yet. Edit this strain to start its ancestry graph.';
    scale=1;resize();
  };
  const resize=()=>{const svg=$('.lineage-svg');svg.setAttribute('width',Math.round(naturalWidth*scale));svg.setAttribute('height',Math.round(naturalHeight*scale));};
  $('#lineage-depth').addEventListener('change',draw);
  $('#lineage-in').addEventListener('click',()=>{scale=Math.min(2,scale+.2);resize();});
  $('#lineage-out').addEventListener('click',()=>{scale=Math.max(.3,scale-.2);resize();});
  $('#lineage-fit').addEventListener('click',()=>{scale=Math.min(1,$('.lineage-viewport').clientWidth/naturalWidth);resize();});
  draw();
}
