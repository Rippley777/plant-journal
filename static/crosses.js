'use strict';
let crossPlans = [], crossStatus = 'all';
const crossById = id => crossPlans.find(plan => plan.id === id);
function strainViews(planner) {
  return `<nav class="collection-tabs" aria-label="Strain views"><a href="/strains" ${!planner?'class="active" aria-current="page"':''}>Collection cards</a><a href="/strains?view=crosses" ${planner?'class="active" aria-current="page"':''}>Cross planner</a></nav>`;
}
function crossParents(plan) {
  return [plan.parent_one_id,plan.parent_two_id].map(id=>strainById(id)?.name || 'Unknown parent').join(' × ');
}
async function crossPlansPage() {
  crossPlans = await api('/cross-plans');
  $('h1').textContent='Cross planner';
  heading('A place for the hybrids you imagine.',button('+ Plan a cross','cross-plan'));
  const completed=crossPlans.filter(plan=>plan.converted_strain_id).length;
  $('#content').innerHTML=`${strainViews(true)}<section class="collection-banner"><div><p class="eyebrow">FROM IDEA TO COLLECTION</p><h2>Plan the next branch.</h2><p>Choose two parent strains and keep your ideas together. When you have the cross, create its strain card with the parents and notes you recorded.</p></div><div class="collection-counts"><div><strong>${crossPlans.length-completed}</strong><span>PLANNED</span></div><div><strong>${completed}</strong><span>CREATED</span></div></div></section><div class="toolbar"><p class="helper">Plans are shared within this garden. Planning does not collect either parent.</p><div><label for="cross-status">Show crosses</label><select id="cross-status">${[['all','All crosses'],['planned','Planned'],['created','Created strains']].map(([id,label])=>`<option value="${id}" ${crossStatus===id?'selected':''}>${label}</option>`).join('')}</select></div></div><div id="cross-plans" class="grid cross-grid"></div>`;
  $('#cross-status').addEventListener('change',event=>{crossStatus=event.target.value;renderCrossPlans();});
  renderCrossPlans();
}
function renderCrossPlans() {
  const visible=crossPlans.filter(plan=>crossStatus==='all' || Boolean(plan.converted_strain_id)===(crossStatus==='created'));
  $('#cross-plans').innerHTML=visible.length?visible.map(plan=>{
    const completed=Boolean(plan.converted_strain_id);
    return `<article class="card cross-plan"><div class="card-header"><span class="eyebrow">${completed?'A NEW BRANCH':'ON THE DRAWING BOARD'}</span><span class="badge">${completed?'Created':'Planned'}</span></div><div class="card-body"><div class="cross-parents">${[plan.parent_one_id,plan.parent_two_id].map((id,index)=>`${index?'<span class="cross-symbol" aria-label="crossed with">×</span>':''}<a class="cross-parent" href="${strainUrl(id)}"><small>Parent ${index+1}</small><strong>${esc(strainById(id)?.name)}</strong></a>`).join('')}</div><div class="cross-branch" aria-hidden="true">↓</div><div class="cross-offspring"><h2>${esc(plan.name)}</h2><p class="helper">${completed?'Created '+esc(formatDate(plan.converted_at,{year:'numeric'})):'Planned cross · '+esc(plan.species || 'Species not recorded')}</p></div>${plan.breeder?`<p class="helper section-space">Breeder / origin: ${esc(plan.breeder)}</p>`:''}<p class="profile-description cross-notes">${esc(plan.notes || 'No notes yet.')}</p><div class="row cross-actions">${completed?`<a class="button small" href="${strainUrl(plan.converted_strain_id)}">View strain ↗</a>`:button('Create strain','convert-cross',plan.id,'small')+button('Edit plan','cross-plan',plan.id,'secondary small')}${button('Delete plan','delete-cross',plan.id,'danger small')}</div></div></article>`;
  }).join(''):empty(crossPlans.length?'No crosses in this view':'Your next cross starts here',crossPlans.length?'Try another filter.':'Add the parent strains to your collection, then pair them in a plan.',button('+ Plan a cross','cross-plan','','secondary'));
}
function crossDetailsFields(plan,conversion=false) {
  return `<label>${conversion?'Strain name':'Working name'}<input name="name" value="${esc(plan?.name)}" maxlength="120" required placeholder="Give this cross a name"></label><div class="grid two-col cross-fields"><label>Species<input name="species" maxlength="160" value="${esc(plan?.species)}" placeholder="e.g. Cannabis or Tomato"></label><label>Breeder / origin<input name="breeder" maxlength="160" value="${esc(plan?.breeder)}"></label></div>`;
}
function crossEditor(id) {
  const plan=crossById(id);
  if (!strains.length) {
    notice('Add parent strains to your collection before planning a cross.');
    return;
  }
  const options=selected=>`<option value="">Choose a strain</option>${strains.map(strain=>`<option value="${strain.id}" ${strain.id===selected?'selected':''}>${esc(strain.name)}</option>`).join('')}`;
  modal(plan?'Edit cross plan':'Plan a cross',formWrap(`${crossDetailsFields(plan)}<div class="grid two-col cross-fields"><div class="stack"><label for="cross-parent-one">Parent 1</label><select id="cross-parent-one" name="parent_one_id" required>${options(plan?.parent_one_id)}</select></div><div class="stack"><label for="cross-parent-two">Parent 2</label><select id="cross-parent-two" name="parent_two_id" required>${options(plan?.parent_two_id)}</select></div></div><p class="helper">Choose strains already in your collection. Parent order does not imply sex; the same strain can fill both slots.</p><div class="cross-preview" id="cross-preview" aria-live="polite"></div><label for="cross-plan-notes">Plan notes</label><textarea id="cross-plan-notes" name="notes" maxlength="10000" placeholder="What makes this pairing interesting?">${esc(plan?.notes)}</textarea>`,'Save plan'),async data=>{
    await api('/cross-plans'+(plan?'/'+plan.id:''),plan?'PUT':'POST',Object.fromEntries(data));
    crossStatus='all';history.replaceState(null,'','/strains?view=crosses');notice('Cross plan saved.');
  });
  const preview=()=>{
    const form=$('form',$('#editor'));
    const parents=['parent_one_id','parent_two_id'].map(name=>strainById(form.elements[name].value)?.name || 'Choose a parent');
    $('#cross-preview').textContent=parents.join(' × ')+' → '+(form.elements.name.value.trim() || 'Your planned cross');
  };
  $('form',$('#editor')).addEventListener('input',preview);preview();
}
function convertCross(id) {
  const plan=crossById(id);if(!plan || plan.converted_strain_id)return;
  modal('Create strain from plan',formWrap(`<p class="helper">Use this when you have this cross. The strain will be marked collected, with both parents linked in its ancestry. The original plan stays in your planner.</p><p class="cross-preview">${esc(crossParents(plan))}</p>${crossDetailsFields(plan,true)}<label for="cross-strain-notes">Strain notes</label><textarea id="cross-strain-notes" name="notes" maxlength="10000">${esc(plan.notes)}</textarea>`,'Create strain'),async data=>{
    const result=await api('/cross-plans/'+id+'/convert','POST',Object.fromEntries(data));
    history.replaceState(null,'',strainUrl(result.id));notice('Strain created. Your card is collected.');
  });
}
function deleteCross(id) {
  const plan=crossById(id);if(!plan)return;
  modal('Delete cross plan?',formWrap(`<p>Delete the plan for ${esc(plan.name)}?${plan.converted_strain_id?' Its created strain stays in your collection.':''}</p>`,'Delete plan'),async()=>{
    await api('/cross-plans/'+id,'DELETE');notice('Cross plan deleted.');
  });
}
