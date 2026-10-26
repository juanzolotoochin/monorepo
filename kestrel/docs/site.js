import { marked } from 'marked';
import mermaid from 'mermaid';
import './site.css';

const documents = import.meta.glob('./**/*.md', { query: '?raw', import: 'default', eager: true });
const pages = Object.fromEntries(Object.entries(documents).map(([path, text]) => [path.slice(2), text]));
const imageFiles = import.meta.glob('./**/*.png', { query: '?url', import: 'default', eager: true });
const images = Object.fromEntries(Object.entries(imageFiles).map(([path, url]) => [path.slice(2), url]));
const groups = [
  ['Start here', ['README.md', 'architecture.md', 'asset-walkthrough.md', 'architecture-reference.md']],
  ['Systems', ['controller-support.md', 'asset-system.md', 'destructible-assets.md', 'character-definition.md', 'character-animation.md', 'service-layers.md']],
  ['Operations', ['flaky-tests.md']],
];

const title = path => pages[path]?.match(/^#\s+(.+)$/m)?.[1] || path.split('/').pop().replace(/\.md$/, '').replaceAll('-', ' ');
const params = new URLSearchParams(location.search);
let current = params.get('doc') || 'README.md';

marked.setOptions({ gfm: true, breaks: false });
mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: 'neutral' });

const app = document.querySelector('#app');
app.innerHTML = `<aside class="sidebar"><a class="brand" href="?doc=README.md"><span class="mark">K</span><span>Kestrel <small>Design docs</small></span></a><nav aria-label="Documents"></nav><p class="sidebar-note">Source: <code>kestrel/docs</code><br>Run with <code>bazel run //kestrel/docs</code></p></aside><main><header class="topbar"><button id="menu" aria-label="Toggle navigation">☰</button><span id="crumb"></span><a id="source" target="_blank" rel="noopener">View source</a></header><article id="content"></article></main>`;
const nav = app.querySelector('nav');
const content = app.querySelector('#content');
let sourceUrl;

for (const [label, paths] of groups) {
  const available = paths.filter(path => path in pages);
  if (!available.length) continue;
  const section = document.createElement('section');
  const heading = document.createElement('h2');
  heading.textContent = label;
  section.append(heading);
  for (const path of available) {
    const link = document.createElement('a');
    link.href = `?doc=${encodeURIComponent(path)}`;
    link.dataset.doc = path;
    link.textContent = title(path);
    section.append(link);
  }
  nav.append(section);
}

function resolveLink(href) {
  if (!href || href.startsWith('#') || /^[a-z]+:/i.test(href)) return href;
  const [path, hash = ''] = href.split('#');
  const resolved = new URL(path, `https://docs.local/${current}`).pathname.slice(1);
  if (resolved.endsWith('.md') && resolved in pages) return `?doc=${encodeURIComponent(resolved)}${hash ? `#${hash}` : ''}`;
  if (resolved in images) return images[resolved];
  return `/${resolved}${hash ? `#${hash}` : ''}`;
}

async function render(path, push = false) {
  if (!(path in pages)) path = 'README.md';
  current = path;
  if (push) history.pushState(null, '', `?doc=${encodeURIComponent(path)}`);
  document.title = `${title(path)} · Kestrel`;
  app.querySelector('#crumb').textContent = path;
  if (sourceUrl) URL.revokeObjectURL(sourceUrl);
  sourceUrl = URL.createObjectURL(new Blob([pages[path]], { type: 'text/markdown' }));
  app.querySelector('#source').href = sourceUrl;
  app.querySelector('#source').download = path.split('/').pop();
  nav.querySelectorAll('a[data-doc]').forEach(link => link.classList.toggle('active', link.dataset.doc === path));
  content.innerHTML = marked.parse(pages[path]);
  content.querySelectorAll('a[href]').forEach(link => { link.href = resolveLink(link.getAttribute('href')); });
  content.querySelectorAll('img[src]').forEach(img => { img.src = resolveLink(img.getAttribute('src')); });
  const diagrams = [...content.querySelectorAll('pre > code.language-mermaid')];
  for (const [index, code] of diagrams.entries()) {
    const frame = document.createElement('div');
    frame.className = 'diagram';
    try {
      const result = await mermaid.render(`diagram-${path.replace(/[^a-z0-9]/gi, '-')}-${index}`, code.textContent);
      frame.innerHTML = result.svg;
    } catch (error) {
      frame.textContent = `Diagram error: ${error.message}`;
    }
    code.parentElement.replaceWith(frame);
  }
  app.classList.remove('nav-open');
  if (location.hash) document.getElementById(location.hash.slice(1))?.scrollIntoView();
  else scrollTo(0, 0);
}

app.addEventListener('click', event => {
  const link = event.target.closest('a[data-doc]');
  if (!link) return;
  event.preventDefault();
  render(link.dataset.doc, true);
});
app.querySelector('#menu').addEventListener('click', () => app.classList.toggle('nav-open'));
addEventListener('popstate', () => render(new URLSearchParams(location.search).get('doc')));
render(current);
