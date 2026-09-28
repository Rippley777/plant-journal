'use strict';
const signup = location.pathname === '/signup';
const form = document.querySelector('#auth-form');
if (signup) {
  document.querySelector('#auth-title').textContent = 'Start your garden';
  document.querySelector('#auth-intro').textContent = 'Create an account for your plants, photos, and shared gardens.';
  document.querySelector('#auth-submit').textContent = 'Create account';
  document.querySelector('#auth-switch').textContent = 'Already have an account? Sign in';
  document.querySelector('#auth-switch').href = '/login';
  document.querySelector('#password-help').hidden = false;
  form.elements.password.autocomplete = 'new-password';
  form.elements.password.minLength = 12;
  document.body.classList.add('signup-page');
  const choices = document.createElement('div');
  choices.id = 'signup-catalogs';
  choices.innerHTML = '<p class="helper" role="status">Loading starter collections…</p>';
  document.querySelector('.form-error').before(choices);
  const submit = document.querySelector('#auth-submit');
  submit.disabled = true;
  fetch('/api/v1/catalogs').then(async response => {
    if(!response.ok)throw new Error('Collections unavailable');
    choices.innerHTML = CatalogChoices.markup(await response.json());
    CatalogChoices.bind(choices);
  }).catch(() => {
    choices.innerHTML = '<p class="helper" role="status">Starter collections are unavailable right now. You can start empty and add them from your collection later.</p>';
  }).finally(() => {submit.disabled = false;});
}
form.addEventListener('submit', async event => {
  event.preventDefault();
  const button = document.querySelector('#auth-submit');
  button.disabled = true;
  button.textContent = signup ? 'Creating your garden…' : 'Signing in…';
  document.querySelector('.form-error').textContent = '';
  try {
    const response = await fetch('/api/v1/auth/' + (signup ? 'signup' : 'login'), {
      method: 'POST', headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({email: form.elements.email.value, password: form.elements.password.value, ...(signup ? {catalogs:new FormData(form).getAll('catalogs')} : {})})
    });
    const result = await response.json();
    if (!response.ok) throw new Error(result.error || 'Unable to sign in');
    sessionStorage.setItem('garden_id', result.garden_id);
    location.assign('/');
  } catch (error) { document.querySelector('.form-error').textContent = error.message; }
  finally { button.disabled = false; button.textContent = signup ? 'Create account' : 'Sign in'; }
});
